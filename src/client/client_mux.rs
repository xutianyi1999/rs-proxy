use bytes::Bytes;
use crypto::rc4::Rc4;
use dashmap::DashMap;
use tokio::io::{Error, ErrorKind, Result};
use tokio::net::tcp::OwnedReadHalf;
use tokio::sync::mpsc::{Sender, UnboundedSender};
use tokio::sync::Mutex;

use crate::commons;
use crate::commons::{Address, MsgReadHandler, StdResConvert};
use crate::message::Msg;

type DB = DashMap<String, UnboundedSender<Bytes>>;

pub struct ClientMuxChannel {
  tx: Sender<Msg>,
  db: DB,
  is_close: Mutex<Box<bool>>,
}

impl ClientMuxChannel {
  pub fn new(tx: Sender<Msg>) -> ClientMuxChannel {
    ClientMuxChannel { tx, db: DashMap::new(), is_close: Mutex::new(Box::new(false)) }
  }

  pub async fn recv_process(&self, rx: OwnedReadHalf, rc4: Rc4) -> Result<()> {
    let res = self.f(rx, rc4).await;
    let mut flag_mutex_guard = self.is_close.lock().await;
    **flag_mutex_guard = true;
    self.db.clear();
    res
  }

  async fn f(&self, mut rx: OwnedReadHalf, mut rc4: Rc4) -> Result<()> {
    loop {
      let msg = rx.read_msg(&mut rc4).await?;

      match msg {
        Msg::DATA(channel_id, data) => {
          if let Some(tx) = self.db.get(&channel_id) {
            if let Err(_) = tx.send(data) {
              error!("Send msg to local TX error")
            }
          }
        }
        Msg::DISCONNECT(channel_id) => {
          self.db.remove(&channel_id);
        }
        _ => return Err(Error::new(ErrorKind::Other, "Message error"))
      }
    }
  }

  pub async fn register(&self, addr: Address, mpsc_tx: UnboundedSender<Bytes>) -> Result<P2pChannel<'_>> {
    let is_close_lock_guard = self.is_close.lock().await;
    if **is_close_lock_guard == true {
      return Err(Error::new(ErrorKind::Other, "Is closed"));
    }

    let channel_id = commons::create_channel_id();
    let mut tx = self.tx.clone();

    tx.send(Msg::CONNECT(channel_id.clone(), addr)).await
      .std_res_convert(|e| e.to_string())?;

    self.db.insert(channel_id.clone(), mpsc_tx);

    let p2p_channel = P2pChannel { tx, mux_channel: self, channel_id };
    Ok(p2p_channel)
  }

  async fn remove(&self, channel_id: &String, tx: &mut Sender<Msg>) -> Result<()> {
    if let Some(_) = self.db.remove(channel_id) {
      tx.send(Msg::DISCONNECT(channel_id.clone())).await
        .std_res_convert(|e| e.to_string())?;
    }
    Ok(())
  }
}

pub struct P2pChannel<'a> {
  tx: Sender<Msg>,
  mux_channel: &'a ClientMuxChannel,
  channel_id: String,
}

impl P2pChannel<'_> {
  pub async fn write(&mut self, data: Bytes) -> Result<()> {
    self.tx.send(Msg::DATA(self.channel_id.clone(), data)).await
      .std_res_convert(|e| e.to_string())
  }

  pub async fn close(&mut self) -> Result<()> {
    self.mux_channel.remove(&self.channel_id, &mut self.tx).await
  }
}
