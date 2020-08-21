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
  // let address = socks5_decode(&mut socket).await?;
  let client_mux_channel = match get_client_mux_channel() {
    Some(channel) => channel,
    None => return Err(Error::new(ErrorKind::Other, "abc"))
  };

  let channel_id = create_channel_id();
  let socket = Arc::new(Mutex::new(socket));

  client_mux_channel.register(channel_id, socket.clone());

  loop {
    let mut buff = BytesMut::new();

    match socket.lock().await.read(&mut buff).await {
      Ok(size) => if size == 0 {
        // call close, return
      },
      Err(e) => {
        // return Err(e)
      }
    }
    client_mux_channel.write_to_remote(&buff).await;
    buff.clear();
  }
}

// async fn socks5_decode(socket: &mut TcpStream) -> Result<Address> {}
