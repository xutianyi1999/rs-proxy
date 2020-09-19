use std::sync::Arc;

use bytes::{Bytes, BytesMut};
use crypto::rc4::Rc4;
use dashmap::DashMap;
use tokio::io::{BufReader, Result};
use tokio::net::{TcpListener, TcpStream};
use tokio::prelude::*;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Sender, UnboundedReceiver, UnboundedSender};
use tokio::time::Duration;

use crate::commons::{Address, StdResAutoConvert, StdResConvert};
use crate::commons::tcp_mux::{Msg, MsgReadHandler, MsgWriteHandler};

type DB = Arc<DashMap<String, UnboundedSender<Bytes>>>;

pub async fn start(host: &str, key: &str, buff_size: usize) -> Result<()> {
  let mut listener = TcpListener::bind(host).await?;
  info!("Server bind {:?}", listener.local_addr()?);

  let rc4 = Rc4::new(key.as_bytes());

  while let Ok((socket, addr)) = listener.accept().await {
    tokio::spawn(async move {
      info!("{:?} connected", addr);
      let db: DB = Arc::new(DashMap::new());

      if let Err(e) = process(socket, &db, rc4, buff_size).await {
        error!("{}", e);
      };

      db.clear()
    });
  };
  Ok(())
}

async fn process(socket: TcpStream, db: &DB, mut rc4: Rc4, buff_size: usize) -> Result<()> {
  if let Err(e) = socket.set_keepalive(Option::Some(Duration::from_secs(120))) {
    error!("{}", e);
  }

  let (rx, mut tx) = socket.into_split();
  let mut rx = BufReader::with_capacity(65535 * 10, rx);

  let (mpsc_tx, mut mpsc_rx) = mpsc::channel::<Msg>(buff_size);

  let inner_db = db.clone();
  tokio::spawn(async move {
    while let Some(msg) = mpsc_rx.recv().await {
      if let Msg::DATA(channel_id, _) = &msg {
        if !inner_db.contains_key(channel_id) {
          continue;
        }
      }

      if let Err(e) = tx.write_msg(&msg, &mut rc4).await {
        error!("{}", e);
        return;
      }
    };
  });

  loop {
    let msg = rx.read_msg(&mut rc4).await?;

    match msg {
      Msg::CONNECT(channel_id, addr) => {
        let (child_mpsc_tx, child_mpsc_rx) = mpsc::unbounded_channel::<Bytes>();
        db.insert(channel_id.clone(), child_mpsc_tx);

        let db = db.clone();
        let mut mpsc_tx = mpsc_tx.clone();

        tokio::spawn(async move {
          if let Err(e) = child_channel_process(&channel_id, addr, &mut mpsc_tx, child_mpsc_rx).await {
            error!("{}", e);
          }

          if let Some(_) = db.remove(&channel_id) {
            if let Err(e) = mpsc_tx.send(Msg::DISCONNECT(channel_id)).await {
              error!("{}", e.to_string());
            }
          }
        });
      }
      Msg::DISCONNECT(channel_id) => {
        db.remove(&channel_id);
      }
      Msg::DATA(channel_id, data) => {
        if let Some(tx) = db.get(&channel_id) {
          if let Err(e) = tx.send(data) {
            error!("{}", e.to_string())
          }
        }
      }
    };
  }
}

async fn child_channel_process(channel_id: &String, addr: Address,
                               mpsc_tx: &mut Sender<Msg>, mut mpsc_rx: UnboundedReceiver<Bytes>) -> Result<()> {
  let (mut rx, mut tx) = TcpStream::connect((addr.0.as_str(), addr.1)).await?.into_split();

  tokio::spawn(async move {
    while let Some(data) = mpsc_rx.recv().await {
      if let Err(e) = tx.write_all(&data).await {
        error!("{}", e);
        return;
      }
    };
  });

  loop {
    let mut buff = BytesMut::with_capacity(65530);

    match rx.read_buf(&mut buff).await {
      Ok(len) => if len == 0 {
        return Ok(());
      }
      Err(e) => {
        return Err(e);
      }
    }

    mpsc_tx.send(Msg::DATA(channel_id.clone(), buff.freeze())).await
      .res_auto_convert()?;
  };
}
