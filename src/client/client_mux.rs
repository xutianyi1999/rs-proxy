use bytes::Bytes;
use dashmap::DashMap;
use tokio::io::{Error, ErrorKind, Result};
use tokio::net::tcp::OwnedReadHalf;
use tokio::sync::mpsc::{Sender, UnboundedSender};

use crate::commons;
use crate::commons::{Address, MsgReadHandler};
use crate::message::Msg;

type DB = DashMap<String, UnboundedSender<Bytes>>;

pub struct ClientMuxChannel {
  tx: Sender<Msg>,
  db: DB,
}

impl ClientMuxChannel {
  pub fn new(tx: Sender<Msg>) -> ClientMuxChannel {
    ClientMuxChannel { tx, db: DashMap::new() }
  }

  pub async fn recv_process(&self, rx: OwnedReadHalf) -> Result<()> {
    let res = self.f(rx).await;
    self.db.clear();
    res
  }

  async fn f(&self, mut rx: OwnedReadHalf) -> Result<()> {
    loop {
      let msg = rx.read_msg().await?;

      match msg {
        Msg::DATA(channel_id, data) => {
          if let Some(tx) = self.db.get(&channel_id) {
            if let Err(_) = tx.send(data) {
              drop(tx);
              self.db.remove(&channel_id);
              eprintln!("Send msg to local TX error");
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
    let channel_id = commons::create_channel_id();
    let mut tx = self.tx.clone();

    if let Err(e) = tx.send(Msg::CONNECT(channel_id.clone(), addr)).await {
      return Err(Error::new(ErrorKind::Other, e.to_string()));
    }
    self.db.insert(channel_id.clone(), mpsc_tx);

    let p2p_channel = P2pChannel { tx, mux_channel: self, channel_id };
    Ok(p2p_channel)
  }

  async fn remove(&self, channel_id: String, tx: &mut Sender<Msg>) -> Result<()> {
    if let Some(_) = self.db.remove(&channel_id) {
      if let Err(e) = tx.send(Msg::DISCONNECT(channel_id)).await {
        return Err(Error::new(ErrorKind::Other, e.to_string()));
      }
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
    if let Err(e) = self.tx.send(Msg::DATA(self.channel_id.clone(), data)).await {
      return Err(Error::new(ErrorKind::Other, e.to_string()));
    };
    Ok(())
  }

  pub async fn close(&mut self) -> Result<()> {
    self.mux_channel.remove(self.channel_id.clone(), &mut self.tx).await
  }
}
