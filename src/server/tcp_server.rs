use std::sync::Arc;

use crypto::rc4::Rc4;
use dashmap::DashMap;
use tokio::io::{BufReader, Result};
use tokio::net::{TcpListener, TcpStream};
use tokio::prelude::*;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Sender, UnboundedReceiver, UnboundedSender};

use crate::commons::{Address, StdResAutoConvert};
use crate::commons::tcp_mux::{Msg, MsgReadHandler, MsgWriteHandler};

type DB = Arc<DashMap<String, UnboundedSender<Vec<u8>>>>;

pub async fn start(host: &str, key: &str, buff_size: usize) -> Result<()> {
  let listener = TcpListener::bind(host).await?;
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
  let (tcp_rx, mut tcp_tx) = socket.into_split();
  let mut tcp_rx = BufReader::with_capacity(10485760, tcp_rx);
  let (mpsc_tx, mut mpsc_rx) = mpsc::channel::<Vec<u8>>(buff_size);

  tokio::spawn(async move {
    while let Some(msg) = mpsc_rx.recv().await {
      if let Err(e) = tcp_tx.write_msg(msg, &mut rc4).await {
        error!("{}", e);
        return;
      }
    };
  });

  loop {
    let msg = tcp_rx.read_msg(&mut rc4).await?;

    match msg {
      Msg::CONNECT(channel_id, addr) => {
        let (child_mpsc_tx, child_mpsc_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        db.insert(channel_id.clone(), child_mpsc_tx);

        let db = db.clone();
        let mpsc_tx = mpsc_tx.clone();

        tokio::spawn(async move {
          if let Err(e) = child_channel_process(&channel_id, addr, &mpsc_tx, child_mpsc_rx).await {
            error!("{}", e);
          }

          if let Some(_) = db.remove(&channel_id) {
            if let Err(e) = mpsc_tx.send(Msg::DISCONNECT(channel_id).encode()).await {
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
                               mpsc_tx: &Sender<Vec<u8>>, mut mpsc_rx: UnboundedReceiver<Vec<u8>>) -> Result<()> {
  let socket = TcpStream::connect((addr.0.as_str(), addr.1)).await?;
  let (mut tcp_rx, mut tcp_tx) = socket.into_split();

  tokio::spawn(async move {
    while let Some(data) = mpsc_rx.recv().await {
      if let Err(e) = tcp_tx.write_all(&data).await {
        error!("{}", e);
        return;
      }
    };
  });

  let mut buff = vec![0u8; 65530];

  loop {
    let slice = match tcp_rx.read(&mut buff).await {
      Ok(n) if n == 0 => return Ok(()),
      Ok(n) => &buff[..n],
      Err(e) => return Err(e)
    };

    mpsc_tx.send(Msg::DATA(channel_id.clone(), slice.to_vec()).encode()).await
      .res_auto_convert()?;
  };
}
