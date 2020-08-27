use std::sync::Arc;

use bytes::BytesMut;
use dashmap::DashMap;
use tokio::io::Result;
use tokio::net::{TcpListener, TcpStream};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::prelude::*;
use tokio::sync::Mutex;

use crate::commons::{MsgReadHandler, MsgWriteHandler};
use crate::message::Msg;

type DB = Arc<DashMap<String, OwnedWriteHalf>>;

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
      db.clear();
    });
  };
  Ok(())
}

async fn process(socket: TcpStream, db: &DB) -> Result<()> {
  let (mut rx, tx) = socket.into_split();
  let tx = Arc::new(Mutex::new(tx));

  loop {
    let msg = rx.read_msg().await?;

    match msg {
      Msg::CONNECT(channel_id, addr) => {
        let rx = match TcpStream::connect((addr.0.as_str(), addr.1)).await {
          Ok(socket) => {
            let (rx, tx) = socket.into_split();
            db.insert(channel_id.clone(), tx);
            rx
          }
          Err(e) => {
            tx.lock().await.write_msg(&Msg::DISCONNECT(channel_id)).await?;
            return Err(e);
          }
        };

        let tx = tx.clone();
        let db = db.clone();

        tokio::spawn(async move {
          if let Err(e) = child_channel_process(&tx, &channel_id, rx).await {
            eprintln!("{:?}", e);
          }

          if let Some(_) = db.remove(&channel_id) {
            if let Err(e) = tx.lock().await.write_msg(&Msg::DISCONNECT(channel_id)).await {
              eprintln!("{:?}", e)
            }
          }
        });
      }
      Msg::DISCONNECT(channel_id) => {
        db.remove(&channel_id);
      }
      Msg::DATA(channel_id, data) => {
        if let Some(mut tx) = db.get_mut(&channel_id) {
          tx.write_all(&data).await?;
        }
      }
    }
  }
}

async fn child_channel_process(main_tx: &Arc<Mutex<OwnedWriteHalf>>, channel_id: &String, mut rx: OwnedReadHalf) -> Result<()> {
  loop {
    let mut buff = BytesMut::new();
    match rx.read_buf(&mut buff).await {
      Ok(len) => if len == 0 {
        return Ok(());
      }
      Err(e) => {
        return Err(e);
      }
    }
    main_tx.lock().await.write_msg(&Msg::DATA(channel_id.clone(), buff.freeze())).await?;
  };
}
