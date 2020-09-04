use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crypto::rc4::Rc4;
use tokio::io::Result;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use yaml_rust::yaml::Array;

use client_mux::ClientMuxChannel;

use crate::{commons, CONFIG_ERROR};
use crate::commons::{MsgWriteHandler, OptionConvert};
use crate::message::Msg;

mod client_mux;
mod client_proxy;
mod socks5;

lazy_static! {
  static ref CONNECTION_POOL: Mutex<ConnectionPool> = Mutex::new(ConnectionPool::new());
}

pub async fn start(bind_addr: &str, host_list: &Array) -> Result<()> {
  for host in host_list {
    let target_name = host["name"].as_str().option_to_res(CONFIG_ERROR)?;
    let count = host["connections"].as_i64().option_to_res(CONFIG_ERROR)?;
    let addr = host["host"].as_str().option_to_res(CONFIG_ERROR)?;
    let key = host["key"].as_str().option_to_res(CONFIG_ERROR)?;
    let rc4 = Rc4::new(key.as_bytes());

    for i in 0..count {
      let target_name = target_name.to_string();
      let addr = addr.to_string();

      tokio::spawn(async move {
        let target_name = format!("{}-{}", target_name, i);

        if let Err(e) = connect(&addr, &target_name, rc4).await {
          error!("{}", e);
        }
        error!("{} crashed", target_name);
      });
    }
  }

  client_proxy::bind(bind_addr).await
}

async fn connect(host: &str, target_name: &str, mut rc4: Rc4) -> Result<()> {
  let channel_id = commons::create_channel_id();

  loop {
    let (rx, mut tx) = match TcpStream::connect(host).await {
      Ok(socket) => socket.into_split(),
      Err(e) => {
        eprintln!("{:?}", e);
        continue;
      }
    };

    println!("{} connected", target_name);

    let (mpsc_tx, mut mpsc_rx) = mpsc::channel::<Msg>(300);

    tokio::spawn(async move {
      while let Some(msg) = mpsc_rx.recv().await {
        if let Err(e) = tx.write_msg(&msg, &mut rc4).await {
          eprintln!("{:?}", e);
          return;
        }
      };
    });

    let cmc = Arc::new(ClientMuxChannel::new(mpsc_tx));

    CONNECTION_POOL.lock().unwrap().put(channel_id.clone(), cmc.clone());

    if let Err(e) = cmc.recv_process(rx, rc4).await {
      eprintln!("{:?}", e);
    }

    let _ = CONNECTION_POOL.lock().unwrap().remove(&channel_id);
    eprintln!("{} disconnected", target_name);
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
