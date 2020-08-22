use std::borrow::BorrowMut;
use std::net::IpAddr;
use std::rc::Rc;
use std::sync::Arc;

use bytes::BytesMut;
use dashmap::DashMap;
use tokio::io::{Error, ErrorKind, Result};
use tokio::macros::support::Future;
use tokio::net::{TcpListener, TcpStream};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf, ReadHalf, WriteHalf};
use tokio::prelude::*;
use tokio::sync::{Mutex, MutexGuard};

use crate::commons;
use crate::commons::Address;
use crate::message;
use crate::message::Msg;

pub struct ClientMuxChannel {
  tx: Mutex<OwnedWriteHalf>,
  db: DashMap<String, OwnedWriteHalf>,
}

impl ClientMuxChannel {
  pub fn new(tx: OwnedWriteHalf) -> ClientMuxChannel {
    let cmc = ClientMuxChannel { tx: Mutex::new(tx), db: DashMap::new() };
    cmc
  }

  pub async fn recv_process(&self, mut rx: OwnedReadHalf) -> Result<()> {
    loop {
      let len = rx.read_u32().await?;
      let mut msg = BytesMut::with_capacity(len as usize);
      rx.read_exact(&mut msg).await?;

      match message::decode(msg)? {
        Msg::DATA(channel_id, data) => {
          match self.db.get_mut(&channel_id) {
            Some(mut tx) => tx.write_all(&data).await?,
            None => ()
          }
        }
        Msg::DISCONNECT(channel_id) => {
          self.db.remove(&channel_id);
        }
        _ => return Err(Error::new(ErrorKind::Other, "Message error"))
      }
    }
  }

  pub async fn write_to_remote(&self, buff: &BytesMut) -> Result<()> {
    let mut channel = self.tx.lock().await;
    channel.write_all(buff).await
  }

  pub async fn register(&self, addr: Address, writer: OwnedWriteHalf) -> Result<String> {
    let channel_id = commons::create_channel_id();
    let msg = message::encode_connect_msg(addr, &channel_id);
    self.tx.lock().await.write_all(&msg).await?;
    self.db.insert(channel_id.clone(), writer);
    Ok(channel_id)
  }

  pub async fn remove(&self, channel_id: String) -> Result<()> {
    let msg = message::encode_disconnect_msg(&channel_id);
    self.tx.lock().await.write_all(&msg).await?;
    self.db.remove(&channel_id);
    Ok(())
  }
}
