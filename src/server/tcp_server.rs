use std::io::{Error, ErrorKind};
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, Result};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::TlsAcceptor;

use crypto::rc4::Rc4;

use crate::commons::{MAGIC_CODE, StdResAutoConvert, TcpSocketExt};
use crate::commons::tcp_comm::{proxy_tunnel, proxy_tunnel_buf};

pub async fn start(listen_addr: SocketAddr, tls_config: ServerConfig) -> Result<()> {
  let tls_acceptor = TlsAcceptor::from(Arc::new(tls_config));
  let listener = TcpListener::bind(listen_addr).await?;
  info!("Listening on {}", listener.local_addr()?);

  while let Ok((stream, _)) = listener.accept().await {
    let inner_acceptor = tls_acceptor.clone();

    tokio::spawn(async move {
      if let Err(e) = tunnel(stream, inner_acceptor).await {
        error!("{}", e);
      }
    });
  }
  Ok(())
}

async fn tunnel(stream: TcpStream, tls_acceptor: TlsAcceptor) -> Result<()> {
  stream.set_keepalive()?;
  let mut source_stream = tls_acceptor.accept(stream).await?;

  let len = source_stream.read_u16().await?;
  let len = len as usize;

  let mut out = vec![0u8; len];
  source_stream.read_exact(&mut out).await?;

  let addr = String::from_utf8((&out[..len - 2]).to_owned()).res_auto_convert()?;
  let mut port = [0u8; 2];
  port.copy_from_slice(&out[len - 2..]);
  let port = u16::from_be_bytes(port);

  let addr = (addr, port);
  let mut dest_stream = TcpStream::connect(addr).await?;

  let _ = tokio::io::copy_bidirectional(&mut source_stream, &mut dest_stream).await?;
  Ok(())
}
