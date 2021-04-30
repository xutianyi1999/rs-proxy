use std::collections::HashMap;
use std::sync::Arc;

use crypto::rc4::Rc4;
use once_cell::sync::Lazy;
use rand::random;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream, Error, ErrorKind, Result};
use tokio::net::tcp::ReadHalf;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::sync::mpsc::Sender;
use yaml_rust::yaml::Array;

use crate::commons::{Address, MAGIC_CODE, OptionConvert, StdResAutoConvert, TcpSocketExt};
use crate::commons::tcpmux_comm::{ChannelId, Msg, MsgReader, MsgWriter};
use crate::CONFIG_ERROR;

static CONNECTION_POOL: Lazy<parking_lot::Mutex<ConnectionPool>> = Lazy::new(|| parking_lot::Mutex::new(ConnectionPool::new()));

fn start(host_list: &Array, buff_size: usize, channel_capacity: usize) -> Result<()> {
  for host in host_list {
    let server_name = host["name"].as_str().option_to_res(CONFIG_ERROR)?;
    let count = host["connections"].as_i64().option_to_res(CONFIG_ERROR)?;
    let addr = host["host"].as_str().option_to_res(CONFIG_ERROR)?;
    let key = host["key"].as_str().option_to_res(CONFIG_ERROR)?;
    let rc4 = Rc4::new(key.as_bytes());

    for i in 0..count {
      let channel_name = format!("{}-{}", server_name, i);
      let addr = addr.to_string();

      tokio::spawn(async move {
        if let Err(e) = connect(&addr, &channel_name, rc4, buff_size, channel_capacity).await {
          error!("{}", e);
        }
        error!("{} crashed", channel_name);
      });
    }
  }
  Ok(())
}

/// 连接远程主机
async fn connect(host: &str, channel_name: &str, rc4: Rc4, buff_size: usize, channel_capacity: usize) -> Result<()> {
  let channel_id: u32 = random();

  let mut magic_code = [0u8; 4];
  crate::commons::crypto(&MAGIC_CODE.to_be_bytes(), &mut magic_code, &mut (rc4.clone()))?;

  loop {
    let res = async move {
      let mut socket = TcpStream::connect(host).await?;
      socket.write_all(&magic_code).await?;
      socket.set_keepalive()?;
      Result::Ok(socket)
    };

    let mut socket = match res.await {
      Ok(socket) => socket,
      Err(e) => {
        error!("{}", e);
        continue;
      }
    };

    let (tcp_rx, tcp_tx) = socket.split();
    info!("{} connected", channel_name);

    let (mpsc_tx, mut mpsc_rx) = mpsc::channel::<Vec<u8>>(channel_capacity);
    let cmc = TcpMuxChannel::new(mpsc_tx, buff_size);
    let cmc = Arc::new(cmc);

    // 读取本地管道数据，发送到远端
    let f1 = async move {
      let mut msg_writer = MsgWriter::new(tcp_tx, rc4);

      while let Some(msg) = mpsc_rx.recv().await {
        msg_writer.write_msg(&msg).await?;
      };
      Ok(())
    };

    let f2 = cmc.exec_remote_inbound_handler(tcp_rx, rc4);

    CONNECTION_POOL.lock().put(channel_id, cmc.clone());

    let res = tokio::select! {
      res = f1 => res,
      res = f2 => res
    };

    let _ = CONNECTION_POOL.lock().remove(&channel_id);
    cmc.close().await;

    if let Err(e) = res {
      error!("{}", e);
    }

    error!("{} disconnected", channel_name);
  }
}

// 本地管道映射
pub type DB = Mutex<HashMap<ChannelId, DuplexStream>>;

pub struct TcpMuxChannel {
  tx: Sender<Vec<u8>>,
  db: DB,
  buff_size: usize,
  is_closed: RwLock<bool>,
}

impl TcpMuxChannel {
  pub fn new(tx: Sender<Vec<u8>>, buff_size: usize) -> TcpMuxChannel {
    TcpMuxChannel {
      tx,
      db: Mutex::new(HashMap::new()),
      buff_size,
      is_closed: RwLock::new(false),
    }
  }

  pub async fn close(&self) {
    let mut guard = self.is_closed.write().await;
    *guard = true;
    self.db.lock().await.clear();
  }

  pub async fn exec_remote_inbound_handler(&self, rx: ReadHalf<'_>, rc4: Rc4) -> Result<()> {
    let mut msg_reader = MsgReader::new(BufReader::new(rx), rc4);

    while let Some(msg) = msg_reader.read_msg().await? {
      match msg {
        Msg::Data(channel_id, data) => {
          if let Some(tx) = self.db.lock().await.get_mut(&channel_id) {
            if let Err(e) = tx.write_all(data).await {
              error!("{}", e)
            }
          }
        }
        Msg::Disconnect(channel_id) => {
          self.db.lock().await.remove(&channel_id);
        }
        _ => return Err(Error::new(ErrorKind::Other, "Message type error"))
      }
    }
    Ok(())
  }

  /// 本地连接处理器
  pub async fn exec_local_inbound_handler(&self, mut socket: TcpStream, addr: Address) -> Result<()> {
    let (mut child_rx, child_tx) = tokio::io::duplex(self.buff_size);
    let p2p_channel = self.register(addr, child_tx).await?;
    let (mut tcp_rx, mut tcp_tx) = socket.split();

    let f1 = async move {
      let _ = tokio::io::copy(&mut child_rx, &mut tcp_tx).await?;
      Ok(())
    };

    let f2 = async {
      let mut buff = vec![0u8; 65530];

      loop {
        match tcp_rx.read(&mut buff).await {
          Ok(n) if n == 0 => return Ok(()),
          Ok(n) => p2p_channel.write(&buff[..n]).await?,
          Err(e) => return Err(e)
        };
      }
    };

    let res = tokio::select! {
      res = f1 => res,
      res = f2 => res
    };

    if let Err(e) = p2p_channel.close().await {
      error!("{}", e);
    }
    res
  }

  async fn register(&self, addr: Address, child_tx: DuplexStream) -> Result<P2pChannel<'_>> {
    let channel_id: ChannelId = rand::random();

    self.tx.send(Msg::Connect(channel_id, addr).encode()).await
      .res_auto_convert()?;

    let guard = self.is_closed.read().await;

    if *guard {
      return Err(Error::new(ErrorKind::Other, "Register error"));
    }

    self.db.lock().await.insert(channel_id, child_tx);

    let p2p_channel = P2pChannel {
      mux_channel: self,
      channel_id,
    };
    Ok(p2p_channel)
  }

  async fn remove(&self, channel_id: u32) -> Result<()> {
    if !self.tx.is_closed() {
      if self.db.lock().await.remove(&channel_id).is_some() {
        self.tx.send(Msg::Disconnect(channel_id).encode()).await.res_auto_convert()?;
      }
    }
    Ok(())
  }
}

struct P2pChannel<'a> {
  mux_channel: &'a TcpMuxChannel,
  channel_id: u32,
}

impl P2pChannel<'_> {
  pub async fn write(&self, data: &[u8]) -> Result<()> {
    let data = Msg::Data(self.channel_id, data).encode();
    self.mux_channel.tx.send(data).await.res_auto_convert()
  }

  pub async fn close(&self) -> Result<()> {
    self.mux_channel.remove(self.channel_id).await
  }
}

pub struct ConnectionPool {
  db: HashMap<ChannelId, Arc<TcpMuxChannel>>,
  keys: Vec<u32>,
  count: usize,
}

impl ConnectionPool {
  pub fn new() -> ConnectionPool {
    ConnectionPool { db: HashMap::new(), keys: Vec::new(), count: 0 }
  }

  pub fn put(&mut self, k: u32, v: Arc<TcpMuxChannel>) {
    self.keys.push(k);
    self.db.insert(k, v);
  }

  pub fn remove(&mut self, key: &u32) -> Result<()> {
    if let Some(i) = self.keys.iter().position(|k| k.eq(key)) {
      self.keys.swap_remove(i);
      self.db.remove(key);
    }
    Ok(())
  }

  pub fn get(&mut self) -> Option<Arc<TcpMuxChannel>> {
    if self.keys.is_empty() {
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

pub struct TcpMuxHandle;

impl TcpMuxHandle {
  pub fn new(host_list: &Array, buff_size: usize, channel_capacity: usize) -> Result<TcpMuxHandle> {
    start(host_list, buff_size, channel_capacity)?;
    Ok(TcpMuxHandle)
  }

  pub async fn proxy(&self, stream: TcpStream, address: Address) -> Result<()> {
    let opt = CONNECTION_POOL.lock().get();

    let channel = match opt {
      Some(channel) => channel,
      None => return Err(Error::new(ErrorKind::Other, "Get connection error"))
    };

    channel.exec_local_inbound_handler(stream, address).await
  }
}
