use std::convert::TryInto;

use quinn::{Connecting, Endpoint, RecvStream, SendStream};
use tokio::io::{AsyncReadExt, Error, ErrorKind, Result};
use tokio::net::TcpStream;
use tokio::stream::StreamExt;

use crate::commons::{quic_config, StdResAutoConvert, StdResConvert};

pub async fn start(cert_path: &str, priv_key_path: &str, addr: &str) -> Result<()> {
  let sever_config = quic_config::configure_server(cert_path, priv_key_path).await.res_auto_convert()?;

  let mut build = Endpoint::builder();
  build.listen(sever_config);
  let (endpoint, mut incoming) = build.bind(&addr.parse().res_auto_convert()?)
    .res_convert(|_| "Quic server bind error".to_string())?;

  info!("Server bind {}", endpoint.local_addr()?);

  while let Some(conn) = incoming.next().await {
    tokio::spawn(async move {});
  };
  Ok(())
}

async fn process(conn: Connecting) -> Result<()> {
  let remote_addr = conn.remote_address();
  let socket = conn.await.res_convert(|e| "Connection error".to_string())?;
  info!("{} connected", remote_addr);

  let mut bi = socket.bi_streams;

  while let Some(res) = bi.next().await {
    let rxtx = match res {
      Ok(v) => v,
      Err(_) => return Err(Error::new(ErrorKind::Other, "Bi stream connection error"))
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
  let (mut tx, mut rx) = rxtx;

  let len = rx.read_u8().await.unwrap() as usize;
  let mut buff = vec![0u8; len];
  rx.read_exact(&mut buff).await;

  let host = String::from_utf8(buff[..buff.len() - 2].to_vec()).res_auto_convert()?;
  let port: [u8; 2] = buff[buff.len() - 2..].try_into().unwrap();

  let mut socket = TcpStream::connect((host.as_str(), u16::from_be_bytes(port))).await?;
  let (mut remote_rx, mut remote_tx) = socket.split();

  let f1 = tokio::io::copy(&mut rx, &mut remote_tx);
  let f2 = tokio::io::copy(&mut remote_rx, &mut tx);

  tokio::select! {
    _ = f1 => (),
    _ = f2 => ()
  }
  ;
  Ok(())
}
