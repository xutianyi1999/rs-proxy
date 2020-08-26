use bytes::Bytes;
use dashmap::DashMap;
use tokio::io::{Error, ErrorKind, Result};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::prelude::*;
use tokio::sync::Mutex;

use crate::commons;
use crate::commons::{Address, MsgReadHandler, MsgWriteHandler};
use crate::message::Msg;

pub struct ClientMuxChannel {
  tx: Mutex<OwnedWriteHalf>,
  db: DashMap<String, OwnedWriteHalf>,
}

impl ClientMuxChannel {
  pub fn new(tx: OwnedWriteHalf) -> ClientMuxChannel {
    ClientMuxChannel { tx: Mutex::new(tx), db: DashMap::new() }
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
          if let Some(mut tx) = self.db.get_mut(&channel_id) {
            tx.write_all(&data).await?;
          }
        }
        Msg::DISCONNECT(channel_id) => {
          self.db.remove(&channel_id);
        }
        _ => return Err(Error::new(ErrorKind::Other, "Message error"))
      }
    }
  }

  async fn write_to_remote(&self, channel_id: String, buff: Bytes) -> Result<()> {
    let mut channel = self.tx.lock().await;
    channel.write_msg(&Msg::DATA(channel_id, buff)).await
  }

  pub async fn register(&self, addr: Address, writer: OwnedWriteHalf) -> Result<P2pChannel<'_>> {
    let channel_id = commons::create_channel_id();
    self.tx.lock().await.write_msg(&Msg::CONNECT(channel_id.clone(), addr)).await?;
    self.db.insert(channel_id.clone(), writer);

    let p2p_channel = P2pChannel { mux_channel: self, channel_id };
    Ok(p2p_channel)
  }

  async fn remove(&self, channel_id: String) -> Result<()> {
    if let Some(_) = self.db.remove(&channel_id) {
      self.tx.lock().await.write_msg(&Msg::DISCONNECT(channel_id)).await?;
    }
    Ok(())
  }
}

pub struct P2pChannel<'a> {
  mux_channel: &'a ClientMuxChannel,
  channel_id: String,
}

impl P2pChannel<'_> {
  pub async fn write(&self, data: Bytes) -> Result<()> {
    self.mux_channel.write_to_remote(self.channel_id.clone(), data).await
  }

  pub async fn close(&self) -> Result<()> {
    self.mux_channel.remove(self.channel_id.clone()).await
  }
}
