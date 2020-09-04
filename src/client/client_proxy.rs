use bytes::{Bytes, BytesMut};
use tokio::io::{Error, ErrorKind, Result};
use tokio::net::{TcpListener, TcpStream};
use tokio::prelude::*;
use tokio::sync::mpsc;

use crate::client;
use crate::client::socks5;
use crate::commons::{Address, StdResConvert};

pub async fn bind(host: &str) -> Result<()> {
  let mut tcp_listener = TcpListener::bind(host).await?;

  while let Ok((socket, _)) = tcp_listener.accept().await {
    tokio::spawn(async move {
      if let Err(e) = process(socket).await {
        error!("{}", e);
      };
    });
  };
  Ok(())
}

async fn process(mut socket: TcpStream) -> Result<()> {
  let address = socks5_decode(&mut socket).await?;
  let (mut rx, mut tx) = socket.into_split();

  let res = client::CONNECTION_POOL.lock()
    .std_res_convert(|e| e.to_string())?.get();

  let client_mux_channel = match res {
    Some(channel) => channel,
    None => return Err(Error::new(ErrorKind::Other, "Get connection error"))
  };

  let (mpsc_tx, mut mpsc_rx) = mpsc::unbounded_channel::<Bytes>();

  tokio::spawn(async move {
    while let Some(data) = mpsc_rx.recv().await {
      if let Err(e) = tx.write_all(&data).await {
        error!("{}", e);
        return;
      }
    };
  });

  let mut p2p_channel = client_mux_channel.register(address, mpsc_tx).await?;

  loop {
    let mut buff = BytesMut::with_capacity(65530);

    match rx.read_buf(&mut buff).await {
      Ok(size) => if size == 0 {
        p2p_channel.close().await?;
        break Ok(());
      },
      Err(e) => {
        p2p_channel.close().await?;
        break Err(e);
      }
    }
    p2p_channel.write(buff.freeze()).await?;
  }
}

async fn socks5_decode(socket: &mut TcpStream) -> Result<Address> {
  socks5::initial_request(socket).await?;
  let addr = socks5::command_request(socket).await?;
  Ok(addr)
}
