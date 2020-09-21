use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::io::{Error, ErrorKind, Result};
use tokio::net::{TcpListener, TcpStream};
use yaml_rust::Yaml;
use yaml_rust::yaml::Array;

use crate::client::quic_client::QuicChannel;
use crate::client::tcp_client::TcpMuxChannel;
use crate::commons::{Address, StdResAutoConvert};

mod tcp_client;
mod socks5;
mod quic_client;

lazy_static! {
  static ref CONNECTION_POOL: Mutex<ConnectionPool<Channel>> = Mutex::new(ConnectionPool::new());
}

enum Channel {
  Tcp(Arc<TcpMuxChannel>),
  Quic(QuicChannel),
}

pub async fn start(bind_addr: &str, remote_hosts: &Array) -> Result<()> {
  let tcp_list: Vec<&Yaml> = remote_hosts.iter()
    .filter(|e| e["protocol"].as_str().unwrap().eq("tcp"))
    .collect();

  tcp_client::start(tcp_list)?;

  let quic_list: Vec<&Yaml> = remote_hosts.iter()
    .filter(|e| e["protocol"].as_str().unwrap().eq("quic"))
    .collect();

  quic_client::start(bind_addr, quic_list).await?;
  socks5_server_bind(bind_addr).await
}

async fn socks5_server_bind(host: &str) -> Result<()> {
  let mut tcp_listener = TcpListener::bind(host).await?;

  info!("Client bind {}", tcp_listener.local_addr()?);

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
  let opt = CONNECTION_POOL.lock().res_auto_convert()?.get();

  let channel = match opt {
    Some(channel) => channel,
    None => return Err(Error::new(ErrorKind::Other, "Get connection error"))
  };

  match &*channel {
    Channel::Tcp(tcp_mux_channel) => {
      tcp_mux_channel.exec_local_inbound_handler(socket, address).await
    }
    Channel::Quic(quic_channel) => {
      quic_channel.open_bi(socket, address).await
    }
  }
}

async fn socks5_decode(socket: &mut TcpStream) -> Result<Address> {
  socks5::initial_request(socket).await?;
  let addr = socks5::command_request(socket).await?;
  Ok(addr)
}

pub struct ConnectionPool<T> {
  db: HashMap<String, Arc<T>>,
  keys: Vec<String>,
  count: usize,
}

impl<T> ConnectionPool<T> {
  pub fn new() -> ConnectionPool<T> {
    ConnectionPool { db: HashMap::new(), keys: Vec::new(), count: 0 }
  }

  pub fn put(&mut self, k: String, v: T) {
    self.keys.push(k.clone());
    self.db.insert(k, Arc::new(v));
  }

  pub fn remove(&mut self, key: &str) -> Result<()> {
    if let Some(i) = self.keys.iter().position(|k| k.eq(key)) {
      self.keys.remove(i);
      self.db.remove(key);
    }
    Ok(())
  }

  pub fn get(&mut self) -> Option<Arc<T>> {
    if self.keys.len() == 0 {
      return Option::None;
    } else if self.keys.len() == 1 {
      let key = self.keys.get(0)?;
      return self.db.get(key).cloned();
    }

    let count = self.count + 1;

    if self.keys.len() <= count {
      self.count = 0;
    } else {
      self.count = count;
    };
    let key = self.keys.get(self.count)?;
    self.db.get(key).cloned()
  }
}
