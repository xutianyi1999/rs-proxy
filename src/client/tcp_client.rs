use std::net::SocketAddr;

use crypto::rc4::Rc4;
use rand::Rng;
use rand::rngs::ThreadRng;
use tokio::io::{Error, ErrorKind, Result};
use tokio::net::TcpStream;

use crate::commons::tcp_comm::{proxy_tunnel, proxy_tunnel_buf};

struct TcpProxy {
  server_list: Vec<(SocketAddr, Rc4)>,
  buff_size: usize,
}

impl TcpProxy {
  fn new(server_list: Vec<(SocketAddr, Rc4)>, buff_size: usize) -> TcpProxy {
    TcpProxy { server_list, buff_size }
  }

  async fn connect(&self, source_stream: TcpStream, proxy_addr: Vec<u8>) -> Result<()> {
    let server_list = &self.server_list;

    let tuple = if server_list.is_empty() {
      return Err(Error::new(ErrorKind::Other, "Server list is empty"));
    } else if server_list.len() == 1 {
      server_list.get(0).unwrap()
    } else {
      let i: usize = rand::thread_rng().gen_range(0..server_list.len());
      server_list.get(i).unwrap()
    };

    let server_stream = TcpStream::connect((*tuple).0).await?;
    let rc4 = (*tuple).1;
    let buff_size = self.buff_size;

    if buff_size == 0 {
      proxy_tunnel(source_stream, server_stream, rc4).await
    } else {
      proxy_tunnel_buf(source_stream, server_stream, rc4, buff_size).await
    }
  }
}
