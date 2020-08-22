use std::rc::Rc;
use std::sync::Arc;

use bytes::{BufMut, BytesMut};
use tokio::io::{Error, ErrorKind, Result};
use tokio::net::{TcpListener, TcpStream};
use tokio::prelude::*;
use tokio::sync::Mutex;

use crate::client::client_mux::ClientMuxChannel;
use crate::client::socks5;
use crate::commons::{Address, create_channel_id};

pub async fn bind(host: &str) -> Result<()> {
  let mut tcp_listener = TcpListener::bind(host).await?;

  while let Ok((socket, address)) = tcp_listener.accept().await {
    tokio::spawn(async move {});
  };
  Ok(())
}

async fn process(mut socket: TcpStream, get_client_mux_channel: fn() -> Option<ClientMuxChannel>) -> Result<()> {
  let address = socks5_decode(&mut socket).await?;
  let (mut rx, tx) = socket.into_split();

  let client_mux_channel = match get_client_mux_channel() {
    Some(channel) => channel,
    None => return Err(Error::new(ErrorKind::Other, "Get connection error"))
  };

  let channel_id = client_mux_channel.register(address, tx).await?;

  loop {
    let mut buff = BytesMut::new();

    match rx.read_buf(&mut buff).await {
      Ok(size) => if size == 0 {
        client_mux_channel.remove(channel_id);
        return Ok(());
      },
      Err(e) => {
        client_mux_channel.remove(channel_id);
        return Err(e);
      }
    }
    client_mux_channel.write_to_remote(&buff).await?;
    buff.clear();
  }
}

async fn socks5_decode(socket: &mut TcpStream) -> Result<Address> {
  socks5::initial_request(socket).await?;
  let addr = socks5::command_request(socket).await?;
  Ok(addr)
}
