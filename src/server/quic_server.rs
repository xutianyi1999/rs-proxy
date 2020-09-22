use std::convert::TryInto;
use std::net::{SocketAddr, ToSocketAddrs};

use quinn::{Connecting, Endpoint, RecvStream, SendStream};
use tokio::io::{AsyncReadExt, Error, ErrorKind, Result};
use tokio::net::TcpStream;
use tokio::stream::StreamExt;

use crate::commons::{OptionConvert, quic_config, StdResAutoConvert, StdResConvert};

pub async fn start(addr: &str, cert_path: &str, priv_key_path: &str) -> Result<()> {
  let sever_config = quic_config::configure_server(cert_path, priv_key_path).await.res_auto_convert()?;

  let mut build = Endpoint::builder();
  build.listen(sever_config);

  let local_addr: SocketAddr = addr.to_socket_addrs()?.next().option_to_res("Address error")?;

  let (_, mut incoming) = build.bind(&local_addr)
    .res_convert(|_| "Quic server bind error".to_string())?;

  info!("Server bind {}", local_addr);

  while let Some(conn) = incoming.next().await {
    tokio::spawn(async move {
      if let Err(e) = process(conn).await {
        error!("{}", e);
      }
    });
  };
  Ok(())
}

async fn process(conn: Connecting) -> Result<()> {
  let remote_addr = conn.remote_address();
  let socket = conn.await.res_convert(|_| "Connection error".to_string())?;
  info!("{} connected", remote_addr);

  let mut bi = socket.bi_streams;

  while let Some(res) = bi.next().await {
    let rxtx = match res {
      Ok(v) => v,
      Err(_) => return Err(Error::new(ErrorKind::Other, "Remote close"))
    };

    tokio::spawn(async move {
      if let Err(e) = child_process(rxtx).await {
        error!("{}", e)
      }
    });
  }
  Ok(())
}

async fn child_process(rxtx: (SendStream, RecvStream)) -> Result<()> {
  let (mut quic_tx, mut quic_rx) = rxtx;

  let len = quic_rx.read_u8().await?;
  let mut buff = vec![0u8; len as usize];
  quic_rx.read_exact(&mut buff).await.res_convert(|_| "Decode msg error".to_string())?;

  let host = String::from_utf8(buff[..buff.len() - 2].to_vec()).res_auto_convert()?;
  let port: [u8; 2] = buff[buff.len() - 2..].try_into().res_auto_convert()?;

  let mut socket = TcpStream::connect((host.as_str(), u16::from_be_bytes(port))).await?;
  let (mut tcp_rx, mut tcp_tx) = socket.split();

  let f1 = tokio::io::copy(&mut quic_rx, &mut tcp_tx);
  let f2 = tokio::io::copy(&mut tcp_rx, &mut quic_tx);

  tokio::select! {
    _ = f1 => (),
    _ = f2 => ()
  }
  Ok(())
}
