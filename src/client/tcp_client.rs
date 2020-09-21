use std::sync::Arc;

use bytes::{Bytes, BytesMut};
use crypto::rc4::Rc4;
use dashmap::DashMap;
use tokio::io::{BufReader, Error, ErrorKind, Result};
use tokio::net::TcpStream;
use tokio::net::tcp::OwnedReadHalf;
use tokio::prelude::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, Mutex};
use tokio::sync::mpsc::{Sender, UnboundedSender};
use yaml_rust::Yaml;

use crate::client::{Channel, CONNECTION_POOL};
use crate::commons::{Address, OptionConvert, StdResAutoConvert, tcp_mux};
use crate::commons::tcp_mux::{Msg, MsgReadHandler, MsgWriteHandler};
use crate::CONFIG_ERROR;

pub fn start(host_list: Vec<&Yaml>, buff_size: usize) -> Result<()> {
  for host in host_list {
    let server_name = host["name"].as_str().option_to_res(CONFIG_ERROR)?;
    let count = host["connections"].as_i64().option_to_res(CONFIG_ERROR)?;
    let addr = host["host"].as_str().option_to_res(CONFIG_ERROR)?;
    let key = host["key"].as_str().option_to_res(CONFIG_ERROR)?;
    let rc4 = Rc4::new(key.as_bytes());

    for i in 0..count {
      let server_name = server_name.to_string();
      let addr = addr.to_string();

      tokio::spawn(async move {
        let server_name = format!("{}-{}", server_name, i);

        if let Err(e) = connect(&addr, &server_name, rc4, buff_size).await {
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
    let (rx, mut tx) = match TcpStream::connect(host).await {
      Ok(socket) => socket.into_split(),
      Err(e) => {
        error!("{}", e);
        continue;
      }
    };

    info!("{} connected", server_name);

    let (mpsc_tx, mut mpsc_rx) = mpsc::channel::<Msg>(buff_size);
    let (cmc, db) = TcpMuxChannel::new(mpsc_tx);
    let cmc = Arc::new(cmc);

    // 读取本地管道数据，发送到远端
    tokio::spawn(async move {
      while let Some(msg) = mpsc_rx.recv().await {
        if let Msg::DATA(channel_id, _) = &msg {
          if !db.contains_key(channel_id) {
            continue;
          }
        }

        if let Err(e) = tx.write_msg(&msg, &mut rc4).await {
          error!("{}", e);
          return;
        }
      };
    });

    CONNECTION_POOL.lock().res_auto_convert()?
      .put(channel_id.clone(), Channel::Tcp(cmc.clone()));

    if let Err(e) = cmc.exec_remote_inbound_handler(rx, rc4).await {
      error!("{}", e);
    }

    let _ = CONNECTION_POOL.lock().res_auto_convert()?.remove(&channel_id);
    error!("{} disconnected", server_name);
  }
}

// 本地管道映射
pub type DB = Arc<DashMap<String, UnboundedSender<Bytes>>>;

pub struct TcpMuxChannel {
  tx: Sender<Msg>,
  db: DB,
  is_close: Mutex<Box<bool>>,
}

impl TcpMuxChannel {
  pub fn new(tx: Sender<Msg>) -> (TcpMuxChannel, DB) {
    let db = Arc::new(DashMap::new());
    let channel = TcpMuxChannel { tx, db: db.clone(), is_close: Mutex::new(Box::new(false)) };
    (channel, db)
  }

  pub async fn exec_remote_inbound_handler(&self, rx: OwnedReadHalf, mut rc4: Rc4) -> Result<()> {
    let mut rx = BufReader::with_capacity(65535 * 10, rx);

    let res = loop {
      let msg = match rx.read_msg(&mut rc4).await {
        Ok(msg) => msg,
        Err(e) => break Err(e)
      };

      match msg {
        Msg::DATA(channel_id, data) => {
          if let Some(tx) = self.db.get(&channel_id) {
            if let Err(e) = tx.send(data) {
              error!("{}", e.to_string())
            }
          }
        }
        Msg::DISCONNECT(channel_id) => {
          self.db.remove(&channel_id);
        }
        _ => break Err(Error::new(ErrorKind::Other, "Message error"))
      }
    };

    let mut flag_mutex_guard = self.is_close.lock().await;
    **flag_mutex_guard = true;
    self.db.clear();
    res
  }

  /// 本地连接处理器
  pub async fn exec_local_inbound_handler(&self, socket: TcpStream, addr: Address) -> Result<()> {
    let (mut rx, mut tx) = socket.into_split();
    let (mpsc_tx, mut mpsc_rx) = mpsc::unbounded_channel::<Bytes>();

    tokio::spawn(async move {
      while let Some(data) = mpsc_rx.recv().await {
        if let Err(e) = tx.write_all(&data).await {
          error!("{}", e);
          return;
        }
      };
    });

    let mut p2p_channel = self.register(addr, mpsc_tx).await?;

    loop {
      let mut buff = BytesMut::with_capacity(65530);

      match rx.read_buf(&mut buff).await {
        Ok(size) => if size == 0 {
          p2p_channel.close().await?;
          break Ok(());
        },
        Err(e) => {
          if let Err(e) = p2p_channel.close().await {
            error!("{}", e);
          }
          break Err(e);
        }
      }
      p2p_channel.write(buff.freeze()).await?;
    }
  }

  async fn register(&self, addr: Address, mpsc_tx: UnboundedSender<Bytes>) -> Result<P2pChannel<'_>> {
    let is_close_lock_guard = self.is_close.lock().await;
    if **is_close_lock_guard == true {
      return Err(Error::new(ErrorKind::Other, "Is closed"));
    }

    let channel_id = tcp_mux::create_channel_id();
    let mut tx = self.tx.clone();

    tx.send(Msg::CONNECT(channel_id.clone(), addr)).await
      .res_auto_convert()?;

    self.db.insert(channel_id.clone(), mpsc_tx);

    let p2p_channel = P2pChannel { tx, mux_channel: self, channel_id };
    Ok(p2p_channel)
  }

  async fn remove(&self, channel_id: &String, tx: &mut Sender<Msg>) -> Result<()> {
    if let Some(_) = self.db.remove(channel_id) {
      tx.send(Msg::DISCONNECT(channel_id.clone())).await.res_auto_convert()?;
    }
    Ok(())
  }
}

struct P2pChannel<'a> {
  tx: Sender<Msg>,
  mux_channel: &'a TcpMuxChannel,
  channel_id: String,
}

impl P2pChannel<'_> {
  pub async fn write(&mut self, data: Bytes) -> Result<()> {
    self.tx.send(Msg::DATA(self.channel_id.clone(), data)).await.res_auto_convert()
  }

  pub async fn close(&mut self) -> Result<()> {
    self.mux_channel.remove(&self.channel_id, &mut self.tx).await
  }
}
