use std::net::SocketAddr;

use crypto::rc4::Rc4;
use tokio::io::{AsyncReadExt, Result};
use tokio::net::{TcpListener, TcpStream};

use crate::commons::StdResAutoConvert;
use crate::commons::tcp_comm::{proxy_tunnel, proxy_tunnel_buf};
use crate::commons::tcpmux_comm::TcpSocketExt;

async fn start(listen_addr: SocketAddr, rc4: Rc4, buff_size: usize) -> Result<()> {
  let listener = TcpListener::bind(listen_addr).await?;

  while let Ok((stream, _)) = listener.accept().await {
    tokio::spawn(async move {
      if let Err(e) = tunnel(stream, rc4, buff_size).await {
        error!("{}", e);
      }
    });
  }
  Ok(())
}

async fn tunnel(mut stream: TcpStream, rc4: Rc4, buff_size: usize) -> Result<()> {
  stream.set_keepalive()?;

  let len = stream.read_u16().await?;
  let mut addr = vec![0u8; (len as usize) - 2];
  stream.read_exact(&mut addr).await?;
  let addr = String::from_utf8(addr).res_auto_convert()?;
  let port = stream.read_u16().await?;

  let addr = (addr, port);
  let dest_stream = TcpStream::connect(addr).await?;

  match buff_size {
    0 => proxy_tunnel(stream, dest_stream, rc4).await,
    _ => proxy_tunnel_buf(stream, dest_stream, rc4, buff_size).await
  }
}
