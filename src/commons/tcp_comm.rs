use crypto::rc4::Rc4;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, duplex, Result};
use tokio::net::TcpStream;

use crate::commons::crypto;

pub async fn proxy_tunnel(mut source_stream: TcpStream, mut dest_stream: TcpStream, rc4: Rc4) -> Result<()> {
  let (source_rx, source_tx) = source_stream.split();
  let (dest_rx, dest_tx) = dest_stream.split();

  tokio::select! {
    res = tunnel(source_rx, dest_tx, rc4) => res,
    res = tunnel(dest_rx, source_tx, rc4) => res
  }
}

pub async fn proxy_tunnel_buf(mut source_stream: TcpStream, mut dest_stream: TcpStream, rc4: Rc4, buff_size: usize) -> Result<()> {
  let (source_rx, mut source_tx) = source_stream.split();
  let (dest_rx, mut dest_tx) = dest_stream.split();

  let (c1, c2) = duplex(buff_size);
  let (mut c1rx, c1tx) = tokio::io::split(c1);
  let (mut c2rx, c2tx) = tokio::io::split(c2);

  let f1 = tunnel(source_rx, c1tx, rc4);
  let f2 = tokio::io::copy(&mut c2rx, &mut dest_tx);

  let f3 = tunnel(dest_rx, c2tx, rc4);
  let f4 = tokio::io::copy(&mut c1rx, &mut source_tx);

  tokio::select! {
    res = f1 => res,
    res = f2 => {
      let _ = res?;
      Ok(())
    },
    res = f3 => res,
    res = f4 => {
      let _ = res?;
      Ok(())
    },
  }
}

async fn tunnel<R, W>(mut rx: R, mut tx: W, mut rc4: Rc4) -> Result<()>
  where R: AsyncRead + Unpin,
        W: AsyncWrite + Unpin
{
  let mut out = vec![0u8; 65536];
  let mut buff = vec![0u8; 65536];

  loop {
    let slice = match rx.read(&mut buff).await {
      Ok(n) if n == 0 => return Ok(()),
      Ok(n) => &buff[..n],
      Err(e) => return Err(e)
    };

    let out = crypto(slice, &mut out, &mut rc4)?;
    tx.write_all(out).await?;
  }
}
