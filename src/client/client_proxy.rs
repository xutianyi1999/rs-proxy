use bytes::{Bytes, BytesMut};
use tokio::io::{Error, ErrorKind, Result};
use tokio::net::{TcpListener, TcpStream};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::prelude::*;
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::client;
use crate::client::socks5;
use crate::commons::Address;

pub async fn bind(host: &str) -> Result<()> {
  let mut tcp_listener = TcpListener::bind(host).await?;

  while let Ok((socket, _)) = tcp_listener.accept().await {
    tokio::spawn(async move {
      if let Err(e) = process(socket).await {
        eprintln!("{:?}", e);
      };
    });
  };
  Ok(())
}

async fn process(mut socket: TcpStream) -> Result<()> {
  let address = socks5_decode(&mut socket).await?;
  let (mut rx, tx) = socket.into_split();

  let res = client::CONNECTION_POOL.lock().unwrap().get();

  let client_mux_channel = match res {
    Some(channel) => channel,
    None => return Err(Error::new(ErrorKind::Other, "Get connection error"))
  };

  let (mpsc_tx, mpsc_rx) = mpsc::unbounded_channel::<Bytes>();

  tokio::spawn(async move {
    async fn rx_process(mut tx: OwnedWriteHalf, mut rx: UnboundedReceiver<Bytes>) -> Result<()> {
      while let Some(data) = rx.recv().await {
        tx.write_all(&data).await?;
      };
      Ok(())
    }
    if let Err(e) = rx_process(tx, mpsc_rx).await {
      eprintln!("{:?}", e);
    }
  });

  let mut p2p_channel = client_mux_channel.register(address, mpsc_tx).await?;

  loop {
    let mut buff = BytesMut::new();

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
