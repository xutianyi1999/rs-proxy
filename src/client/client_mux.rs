use std::borrow::BorrowMut;
use std::net::IpAddr;
use std::rc::Rc;
use std::sync::Arc;

use bytes::BytesMut;
use dashmap::DashMap;
use tokio::io::{Result, Error, ErrorKind};
use tokio::net::{TcpListener, TcpStream};
use tokio::prelude::*;
use tokio::sync::{Mutex, MutexGuard};
use crate::message;
use crate::message::Msg;

type TcpDb = Arc<DashMap<String, Arc<Mutex<TcpStream>>>>;

pub struct ClientMuxChannel {
  channel: Arc<Mutex<TcpStream>>,
  db: TcpDb,
}

impl ClientMuxChannel {
  pub async fn new(host: &str) -> Result<ClientMuxChannel> {
    let channel = TcpStream::connect(host).await?;
    let channel = Arc::new(Mutex::new(channel));

    let socket = channel.clone();
    let db: TcpDb = Arc::new(DashMap::new());

    let db_inner = db.clone();
    tokio::spawn(async move {
      let mut socket = socket.lock().await;

      loop {
        match ClientMuxChannel::process(&mut socket, &db_inner).await {
          Ok(_) => {}
          Err(e) => {}
        }
      }
    });

    Ok(ClientMuxChannel { channel, db })
  }

  async fn process<'a>(socket: &mut MutexGuard<'a, TcpStream>, db: &TcpDb) -> Result<()> {
    let len = socket.read_u32().await?;
    let mut msg = BytesMut::with_capacity(len as usize);
    socket.read_exact(&mut msg).await?;

    match message::decode(msg)? {
      Msg::DATA(channel_id, data) => {
        match db.get(&channel_id) {
          Some(socket) => socket.lock().await.write_all(&data).await?,
          None => ()
        }
      }
      Msg::DISCONNECT(channel_id) => {
        match db.remove(&channel_id) {
          Some((_, socket)) => socket.lock().await.shutdown(std::net::Shutdown::Both)?,
          None => ()
        }
      }
      _ => return Err(Error::new(ErrorKind::Other, "Message error"))
    }
    Ok(())
  }

  pub async fn write_to_remote(&self, buff: &BytesMut) -> Result<()> {
    let mut channel = self.channel.lock().await;
    channel.write_i32(10).await;
    Ok(())
  }

  pub fn register(&self, channel_id: String, channel: Arc<Mutex<TcpStream>>) {
    self.db.insert(channel_id, channel);
  }

  pub fn remove(&self, channel_id: String) {
    self.db.remove(&channel_id);
  }
}
