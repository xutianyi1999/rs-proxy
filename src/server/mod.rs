use std::collections::HashMap;
use std::fmt::Write;
use std::sync::Arc;

use bytes::BytesMut;
use dashmap::DashMap;
use tokio::io::Result;
use tokio::net::{TcpListener, TcpStream};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::prelude::*;
use tokio::sync::Mutex;

use crate::commons::Address;
use crate::message;
use crate::message::Msg;

pub async fn start() -> Result<()> {
  let mut listener = TcpListener::bind("127.0.0.1:12345").await?;

  while let Ok((socket, addr)) = listener.accept().await {
    tokio::spawn(async move {
      let db: DB = Arc::new(DashMap::new());

      match process(socket, db).await {
        Ok(_) => (),
        Err(e) => eprintln!("{:?}", e)
      };
      db.clear();
    });
  };
  Ok(())
}

type DB = Arc<DashMap<String, OwnedWriteHalf>>;

async fn process(socket: TcpStream, db: DB) -> Result<()> {
  let (mut rx, tx) = socket.into_split();
  let tx = Arc::new(Mutex::new(tx));

  loop {
    let msg = message::read_msg(&mut rx).await?;
    match message::decode(msg)? {
      Msg::CONNECT(channel_id, addr) => {
        let tx = tx.clone();
        let db = db.clone();

        tokio::spawn(async move {
          child_channel_process(tx, channel_id, addr, db)
        });
      }
      Msg::DISCONNECT(channel_id) => {
        db.remove(&channel_id)
      }
      Msg::DATA(channel_id, data) => {
        if let Some(mut tx) = db.get_mut(&channel_id) {
          tx.write_all(&data).await?;
        }
      }
    }
  }
}

async fn child_channel_process(main_tx: Arc<Mutex<OwnedWriteHalf>>, channel_id: String, addr: Address, db: DB) -> Result<()> {
  let socket = TcpStream::connect(addr).await?;
  let (mut rx, tx) = socket.into_split();

  db.insert(channel_id.clone(), tx);

  let mut buff = BytesMut::new();
  let res = loop {
    match rx.read_buf(&mut buff).await {
      Ok(len) => if len == 0 {
        break Ok(());
      }
      Err(e) => {
        break Err(e);
      }
    }
  };

  if let Some(_) = db.remove(&channel_id) {
    let mut msg = message::encode(Msg::DISCONNECT(channel_id));
    main_tx.lock().await.write_all(&mut msg).await?;
  }
  res
}
