use std::net::SocketAddr;

use crypto::rc4::Rc4;
use tokio::io::{AsyncReadExt, Result};
use tokio::net::{TcpListener, TcpStream};

use crate::commons::StdResAutoConvert;
use crate::commons::tcp_comm::{proxy_tunnel, proxy_tunnel_buf};
use crate::commons::tcpmux_comm::TcpSocketExt;

pub async fn start(listen_addr: SocketAddr, rc4: Rc4, buff_size: usize) -> Result<()> {
  let listener = TcpListener::bind(listen_addr).await?;
  info!("Listening on {}", listener.local_addr()?);

  while let Ok((stream, _)) = listener.accept().await {
    tokio::spawn(async move {
      if let Err(e) = tunnel(stream, rc4, buff_size).await {
        error!("{}", e);
      }
    });
  }
  Ok(())
}

async fn tunnel(mut stream: TcpStream, mut rc4: Rc4, buff_size: usize) -> Result<()> {
  stream.set_keepalive()?;

  let len = stream.read_u16().await?;
  let len = len as usize;

  let mut in_ = vec![0u8; len];
  stream.read_exact(&mut in_).await?;
  let mut out = vec![0u8; len];

  crate::commons::crypto(&in_, &mut out, &mut rc4)?;
  let addr = String::from_utf8((&out[..len - 2]).to_owned()).res_auto_convert()?;
  let mut port = [0u8; 2];
  port.copy_from_slice(&out[len - 2..]);
  let port = u16::from_be_bytes(port);

  let addr = (addr, port);
  let dest_stream = TcpStream::connect(addr).await?;

  match buff_size {
    0 => proxy_tunnel(stream, dest_stream, rc4).await,
    _ => proxy_tunnel_buf(stream, dest_stream, rc4, buff_size).await
  }
}
