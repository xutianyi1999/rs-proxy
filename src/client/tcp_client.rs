use std::sync::Arc;

use crypto::rc4::Rc4;
use dashmap::DashMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream, Error, ErrorKind, Result};
use tokio::net::tcp::ReadHalf;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, RwLock};
use tokio::sync::mpsc::Sender;
use yaml_rust::Yaml;

use crate::client::CONNECTION_POOL;
use crate::commons::{Address, OptionConvert, StdResAutoConvert, tcp_mux};
use crate::commons::tcp_mux::{Msg, MsgReadHandler, MsgWriteHandler};
use crate::CONFIG_ERROR;

pub fn start(host_list: Vec<&Yaml>) -> Result<()> {
  for host in host_list {
    let server_name = host["name"].as_str().option_to_res(CONFIG_ERROR)?;
    let count = host["connections"].as_i64().option_to_res(CONFIG_ERROR)?;
    let addr = host["host"].as_str().option_to_res(CONFIG_ERROR)?;
    let key = host["key"].as_str().option_to_res(CONFIG_ERROR)?;
    let buff_size = host["buffSize"].as_i64().option_to_res(CONFIG_ERROR)?;
    let rc4 = Rc4::new(key.as_bytes());

    for i in 0..count {
      let server_name = server_name.to_string();
      let addr = addr.to_string();

      tokio::spawn(async move {
        let server_name = format!("{}-{}", server_name, i);

        if let Err(e) = connect(&addr, &server_name, rc4, buff_size as usize).await {
          error!("{}", e);
        }
        error!("{} crashed", server_name);
      });
    }
  }
  Ok(())
}

/// 连接远程主机
async fn connect(host: &str, server_name: &str, mut rc4: Rc4, buff_size: usize) -> Result<()> {
  let channel_id = tcp_mux::create_channel_id();

  loop {
    let mut socket = match TcpStream::connect(host).await {
      Ok(socket) => socket,
      Err(e) => {
        error!("{}", e);
        continue;
      }
    };

    let (tcp_rx, mut tcp_tx) = socket.split();
    info!("{} connected", server_name);

    let (mpsc_tx, mut mpsc_rx) = mpsc::channel::<Vec<u8>>(buff_size);
    let cmc = TcpMuxChannel::new(mpsc_tx);
    let cmc = Arc::new(cmc);

    // 读取本地管道数据，发送到远端
    let f1 = async move {
      while let Some(msg) = mpsc_rx.recv().await {
        tcp_tx.write_msg(msg, &mut rc4).await?;
      };
      Ok(())
    };

    let f2 = cmc.exec_remote_inbound_handler(tcp_rx, rc4);

    CONNECTION_POOL.lock().res_auto_convert()?
      .put(channel_id.clone(), cmc.clone());

    let res = tokio::select! {
      res = f1 => res,
      res = f2 => res
    };

    cmc.close().await;

    if let Err(e) = res {
      error!("{}", e);
    }

    let _ = CONNECTION_POOL.lock().res_auto_convert()?.remove(&channel_id);
    error!("{} disconnected", server_name);
  }
}

// 本地管道映射
pub type DB = Arc<DashMap<String, DuplexStream>>;

pub struct TcpMuxChannel {
  tx: Sender<Vec<u8>>,
  db: DB,
  is_close: RwLock<bool>,
}

impl TcpMuxChannel {
  pub fn new(tx: Sender<Vec<u8>>) -> TcpMuxChannel {
    TcpMuxChannel { tx, db: Arc::new(DashMap::new()), is_close: RwLock::new(false) }
  }

  pub async fn close(&self) {
    let mut flag_lock_guard = self.is_close.write().await;
    *flag_lock_guard = true;
    self.db.clear();
  }

  pub async fn exec_remote_inbound_handler(&self, rx: ReadHalf<'_>, mut rc4: Rc4) -> Result<()> {
    let mut rx = BufReader::new(rx);

    while let Some(msg) = rx.read_msg(&mut rc4).await? {
      match msg {
        Msg::DATA(channel_id, data) => {
          if let Some(mut tx) = self.db.get_mut(&channel_id) {
            if let Err(e) = tx.write_all(&data).await {
              error!("{}", e)
            }
          }
        }
        Msg::DISCONNECT(channel_id) => {
          self.db.remove(&channel_id);
        }
        _ => return Err(Error::new(ErrorKind::Other, "Message error"))
      }
    }
    Ok(())
  }

  /// 本地连接处理器
  pub async fn exec_local_inbound_handler(&self, mut socket: TcpStream, addr: Address) -> Result<()> {
    // 10MB
    let (mut child_rx, child_tx) = tokio::io::duplex(10485760);
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
    let is_close_lock_guard = self.is_close.read().await;
    if *is_close_lock_guard == true {
      return Err(Error::new(ErrorKind::Other, "Is closed"));
    }

    let channel_id = tcp_mux::create_channel_id();

    self.tx.send(Msg::CONNECT(channel_id.clone(), addr).encode()).await
      .res_auto_convert()?;

    self.db.insert(channel_id.clone(), child_tx);

    let p2p_channel = P2pChannel {
      mux_channel: self,
      channel_id,
    };
    Ok(p2p_channel)
  }

  async fn remove(&self, channel_id: &String) -> Result<()> {
    // 可能存在死锁
    if let Some(_) = self.db.remove(channel_id) {
      self.tx.send(Msg::DISCONNECT(channel_id.clone()).encode()).await.res_auto_convert()?;
    }
    Ok(())
  }
}

struct P2pChannel<'a> {
  mux_channel: &'a TcpMuxChannel,
  channel_id: String,
}

impl P2pChannel<'_> {
  pub async fn write(&self, data: &[u8]) -> Result<()> {
    let data = Msg::DATA(
      self.channel_id.clone(),
      data.to_vec(),
    ).encode();

    self.mux_channel.tx.send(data).await.res_auto_convert()
  }

  pub async fn close(&self) -> Result<()> {
    self.mux_channel.remove(&self.channel_id).await
  }
}
