use std::borrow::Borrow;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex, RwLock};
use std::sync::atomic::{AtomicU8, Ordering};

use dashmap::{DashMap, DashSet, Map};
use tokio::io::Result;
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::TcpStream;

use client_mux::ClientMuxChannel;

use crate::commons;

mod client_mux;
mod client_proxy;
mod socks5;

type AMConnectionPool = Arc<Mutex<ConnectionPool>>;

pub async fn start(host_list: Vec<String>) -> Result<()> {
  let connection_pool = Arc::new(Mutex::new(ConnectionPool::new()));

  for host in host_list {
    let cp = connection_pool.clone();
    tokio::spawn(async move {
      connect(host, &cp).await;
    });
  }

  Ok(())
}

async fn connect(host: String,
                 connection_pool: &AMConnectionPool) -> Result<()> {
  let channel_id = commons::create_channel_id();

  loop {
    let (rx, tx) = match TcpStream::connect(&host).await {
      Ok(socket) => socket.into_split(),
      Err(e) => {
        eprintln!("{:?}", e);
        continue;
      }
    };

    let cmc = Arc::new(ClientMuxChannel::new(tx));
    connection_pool.lock().unwrap().put(channel_id.clone(), cmc.clone());

    if let Err(e) = cmc.recv_process(rx).await {
      eprintln!("{:?}", e);
    }
    connection_pool.lock().unwrap().remove(&channel_id);
  }
}

pub struct ConnectionPool {
  db: HashMap<String, Arc<ClientMuxChannel>>,
  keys: Vec<String>,
  count: usize,
}

impl ConnectionPool {
  pub fn new() -> ConnectionPool {
    ConnectionPool { db: HashMap::new(), keys: Vec::new(), count: 0 }
  }

  pub fn put(&mut self, k: String, v: Arc<ClientMuxChannel>) {
    self.keys.push(k.clone());
    self.db.insert(k, v);
  }

  pub fn remove(&mut self, key: &str) -> Result<()> {
    if let Some(i) = self.keys.iter().position(|k| k.eq(key)) {
      self.keys.remove(i);
      self.db.remove(key);
    }
    Ok(())
  }

  pub fn get(&mut self) -> Option<Arc<ClientMuxChannel>> {
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
