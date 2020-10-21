use std::sync::Arc;

use crypto::rc4::Rc4;
use dashmap::DashMap;
use tokio::io::{BufReader, DuplexStream, Result};
use tokio::net::{TcpListener, TcpStream};
use tokio::prelude::*;
use tokio::sync::mpsc;
use tokio::sync::mpsc::Sender;

use crate::commons::{Address, StdResAutoConvert};
use crate::commons::tcp_mux::{Msg, MsgReadHandler, MsgWriteHandler, TcpStreamExt};

type DB = Arc<DashMap<String, DuplexStream>>;

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
  socket.set_keepalive();
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
        // 10MB
        let (child_rx, child_tx) = tokio::io::duplex(10485760);
        db.insert(channel_id.clone(), child_tx);

        let db = db.clone();
        let mpsc_tx = mpsc_tx.clone();

        tokio::spawn(async move {
          if let Err(e) = child_channel_process(&channel_id, addr, &mpsc_tx, child_rx).await {
            error!("{}", e);
          }

          if let Some(_) = db.remove(&channel_id) {
            if let Err(e) = mpsc_tx.send(Msg::DISCONNECT(channel_id).encode()).await {
              error!("{}", e);
            }
          }
        });
      }
      Msg::DISCONNECT(channel_id) => {
        db.remove(&channel_id);
      }
      Msg::DATA(channel_id, data) => {
        if let Some(mut tx) = db.get_mut(&channel_id) {
          if let Err(e) = tx.write_all(&data).await {
            error!("{}", e)
          }
        }
      }
    };
  }
}

async fn child_channel_process(channel_id: &String, addr: Address,
                               mpsc_tx: &Sender<Vec<u8>>, mut child_rx: DuplexStream) -> Result<()> {
  let socket = TcpStream::connect((addr.0.as_str(), addr.1)).await?;
  socket.set_keepalive();
  let (mut tcp_rx, mut tcp_tx) = socket.into_split();

  tokio::spawn(async move {
    if let Err(e) = tokio::io::copy(&mut child_rx, &mut tcp_tx).await {
      error!("{}", e)
    }
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
