use std::net::SocketAddr;
use std::sync::Arc;

use bytes::BufMut;
use rand::Rng;
use tokio::io::{AsyncWriteExt, Error, ErrorKind, Result};
use tokio::net::TcpStream;
use tokio_rustls::rustls::ClientConfig;
use tokio_rustls::TlsConnector;
use tokio_rustls::webpki::DNSNameRef;
use yaml_rust::yaml::Array;

use crypto::rc4::Rc4;

use crate::commons::{Address, load_certs, MAGIC_CODE, OptionConvert};
use crate::commons::tcp_comm::{proxy_tunnel, proxy_tunnel_buf};
use crate::CONFIG_ERROR;

pub struct TcpProxy {
  server_list: Vec<(SocketAddr, Rc4)>,
  buff_size: usize,
}

impl TcpProxy {
  pub fn new(server_list: Vec<(SocketAddr, Rc4)>, buff_size: usize) -> TcpProxy {
    TcpProxy { server_list, buff_size }
  }

  pub async fn connect(&self, source_stream: TcpStream, proxy_addr: Address) -> Result<()> {
    let connector = TlsConnector::from();
    let server_list = &self.server_list;

    let tuple = if server_list.len() == 1 {
      server_list.get(0).unwrap()
    } else {
      let i: usize = rand::thread_rng().gen_range(0..server_list.len());
      server_list.get(i).unwrap()
    };

    let mut server_stream = TcpStream::connect((*tuple).0).await?;
    let mut rc4 = (*tuple).1;
    let buff_size = self.buff_size;

    let mut magic_code_out = [0u8; 4];
    crate::commons::crypto(&MAGIC_CODE.to_be_bytes(), &mut magic_code_out, &mut (rc4.clone()))?;
    server_stream.write_all(&magic_code_out).await?;

    let mut buff: Vec<u8> = Vec::with_capacity(proxy_addr.0.len() + 2);
    buff.put_slice(&proxy_addr.0);
    buff.put_u16(proxy_addr.1);

    let mut out = vec![0u8; buff.len()];
    crate::commons::crypto(&buff, &mut out, &mut rc4)?;

    server_stream.write_u16(out.len() as u16).await?;
    server_stream.write_all(&out).await?;

    if buff_size == 0 {
      proxy_tunnel(source_stream, server_stream, rc4).await
    } else {
      proxy_tunnel_buf(source_stream, server_stream, rc4, buff_size).await
    }
  }
}

pub struct TcpHandle {
  tcp_proxy: TcpProxy
}

impl TcpHandle {
  pub async fn new(remote_hosts: &Array, buff_size: usize) -> Result<TcpHandle> {
    if remote_hosts.is_empty() {
      return Err(Error::new(ErrorKind::Other, "Server list is empty"));
    }

    let mut hosts = Vec::with_capacity(remote_hosts.len());

    for v in remote_hosts {
      let host = v["host"].as_str().option_to_res(CONFIG_ERROR)?;
      let addr = tokio::net::lookup_host(host).await?.next().option_to_res("Target address error")?;

      let mut certs = load_certs(v["certPath"].as_str().option_to_res(CONFIG_ERROR)?)?;
      let mut tls_config = ClientConfig::new();
      tls_config.root_store.add(&certs.remove(0));
      let tls_connector = TlsConnector::from(Arc::new(tls_config));
      hosts.push((addr, rc4));
    };
    Ok(TcpHandle { tcp_proxy: TcpProxy::new(hosts, buff_size) })
  }

  pub async fn proxy(&self, stream: TcpStream, address: Address) -> Result<()> {
    self.tcp_proxy.connect(stream, address).await
  }
}
