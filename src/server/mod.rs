use std::sync::Arc;

use bytes::{Bytes, BytesMut};
use dashmap::DashMap;
use tokio::io::{Error, ErrorKind, Result};
use tokio::net::{TcpListener, TcpStream};
use tokio::prelude::*;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Sender, UnboundedReceiver, UnboundedSender};

use crate::commons::{Address, MsgReadHandler, MsgWriteHandler};
use crate::message::Msg;

type DB = Arc<DashMap<String, UnboundedSender<Bytes>>>;

pub async fn start(host: &str, key: &str) -> Result<()> {
  let mut listener = TcpListener::bind(host).await?;
  println!("bind {:?}", listener.local_addr()?);

  while let Ok((socket, addr)) = listener.accept().await {
    tokio::spawn(async move {
      println!("{:?} connected", addr);
      let db: DB = Arc::new(DashMap::new());

      if let Err(e) = process(socket, &db).await {
        eprintln!("{:?}", e);
      };

      db.clear()
    });
  };
  Ok(())
}

async fn process(socket: TcpStream, db: &DB) -> Result<()> {
  let (mut rx, mut tx) = socket.into_split();
  let (mpsc_tx, mut mpsc_rx) = mpsc::channel::<Msg>(200);

  tokio::spawn(async move {
    while let Some(msg) = mpsc_rx.recv().await {
      if let Err(e) = tx.write_msg(&msg).await {
        eprintln!("{:?}", e);
        return;
      }
    };
  });

  loop {
    let msg = rx.read_msg().await?;

    match msg {
      Msg::CONNECT(channel_id, addr) => {
        let (child_mpsc_tx, child_mpsc_rx) = mpsc::unbounded_channel::<Bytes>();
        db.insert(channel_id.clone(), child_mpsc_tx);

        let db = db.clone();

        let mut mpsc_tx = mpsc_tx.clone();
        tokio::spawn(async move {
          if let Err(e) = child_channel_process(&channel_id, addr, &mut mpsc_tx, child_mpsc_rx).await {
            eprintln!("{:?}", e);
          }

          if let Some(_) = db.remove(&channel_id) {
            if let Err(_) = mpsc_tx.send(Msg::DISCONNECT(channel_id.clone())).await {
              eprintln!("Send disconnect msg error");
            }
          }
        });
      }
      Msg::DISCONNECT(channel_id) => {
        db.remove(&channel_id);
      }
      Msg::DATA(channel_id, data) => {
        if let Some(tx) = db.get(&channel_id) {
          if let Err(_) = tx.send(data) {
            drop(tx);
            db.remove(&channel_id);
            eprintln!("Send msg to child TX error");
          }
        }
      }
    };
  }
}

async fn child_channel_process(channel_id: &String, addr: Address,
                               mpsc_tx: &mut Sender<Msg>, mut mpsc_rx: UnboundedReceiver<Bytes>) -> Result<()> {
  let (mut rx, mut tx) = match TcpStream::connect((addr.0.as_str(), addr.1)).await {
    Ok(socket) => socket.into_split(),
    Err(e) => return Err(e)
  };

  tokio::spawn(async move {
    while let Some(data) = mpsc_rx.recv().await {
      if let Err(e) = tx.write_all(&data).await {
        eprintln!("{:?}", e);
        return;
      }
    };
  });

  loop {
    let mut buff = BytesMut::with_capacity(4294967295);

    match rx.read_buf(&mut buff).await {
      Ok(len) => if len == 0 {
        return Ok(());
      }
      Err(e) => {
        return Err(e);
      }
    }

    if let Err(e) = mpsc_tx.send(Msg::DATA(channel_id.clone(), buff.freeze())).await {
      return Err(Error::new(ErrorKind::Other, e.to_string()));
    }
  };
}
