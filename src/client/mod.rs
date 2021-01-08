use std::collections::HashMap;
use std::net::SocketAddr;
use std::os::raw::c_char;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use tokio::io::{Error, ErrorKind, Result};
use tokio::net::{TcpListener, TcpStream};
use yaml_rust::Yaml;
use yaml_rust::yaml::Array;

use crate::client::tcp_client::TcpMuxChannel;
use crate::commons::{Address, StdResAutoConvert};

mod tcp_client;
mod socks5;

lazy_static! {
  static ref CONNECTION_POOL: Mutex<ConnectionPool> = Mutex::new(ConnectionPool::new());
}

pub async fn start(http_addr: &str, socks5_addr: &str, remote_hosts: &Array) -> Result<()> {
  let tcp_list: Vec<&Yaml> = remote_hosts.iter()
    .filter(|e| e["protocol"].as_str().unwrap().eq("tcp"))
    .collect();

  tcp_client::start(tcp_list)?;

  let port = SocketAddr::from_str(socks5_addr.clone()).res_auto_convert()?.port();
  let http_addr = String::from(http_addr);

  let f1 = tokio::task::spawn_blocking(move || {
    let local_socks5_addr = format!("127.0.0.1:{}", port);
    let res = start_http_proxy_server(&http_addr, &local_socks5_addr);

    if let Err(e) = res {
      error!("{}", e)
    }
  });

  let f2 = socks5_server_bind(socks5_addr);

  tokio::select! {
    res = f1 => res.res_auto_convert(),
    res = f2 => res,
  }
}

async fn socks5_server_bind(host: &str) -> Result<()> {
  let tcp_listener = TcpListener::bind(host).await?;
  info!("Listening on socks5://{}", tcp_listener.local_addr()?);

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

  channel.exec_local_inbound_handler(socket, address).await
}

async fn socks5_decode(socket: &mut TcpStream) -> Result<Address> {
  socks5::initial_request(socket).await?;
  let addr = socks5::command_request(socket).await?;
  Ok(addr)
}

pub struct ConnectionPool {
  db: HashMap<String, Arc<TcpMuxChannel>>,
  keys: Vec<String>,
  count: usize,
}

impl ConnectionPool {
  pub fn new() -> ConnectionPool {
    ConnectionPool { db: HashMap::new(), keys: Vec::new(), count: 0 }
  }

  pub fn put(&mut self, k: String, v: Arc<TcpMuxChannel>) {
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

  pub fn get(&mut self) -> Option<Arc<TcpMuxChannel>> {
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

fn start_http_proxy_server(bind_addr: &str, socks5_addr: &str) -> Result<()> {
  let lib = libloading::Library::new("./httptosocks").res_auto_convert()?;

  unsafe {
    let start: libloading::Symbol<unsafe extern fn(*const c_char, u8, *const c_char, u8) -> ()> = lib.get(b"start").res_auto_convert()?;

    start((bind_addr.to_owned() + "\0").as_ptr() as *const c_char,
          (socks5_addr.to_owned() + "\0").as_ptr() as *const c_char);
  };
  Ok(())
}
