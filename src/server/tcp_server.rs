use std::net::SocketAddr;
use std::sync::Arc;

use crypto::rc4::Rc4;
use dashmap::DashMap;
use tokio::io::{BufReader, DuplexStream, Result};
use tokio::net::{TcpListener, TcpSocket, TcpStream};
use tokio::prelude::*;
use tokio::sync::mpsc;
use tokio::sync::mpsc::Sender;

use crate::commons::{Address, OptionConvert, StdResAutoConvert};
use crate::commons::tcp_mux::{Msg, MsgReadHandler, MsgWriteHandler, TcpSocketExt};

type DB = Arc<DashMap<String, DuplexStream>>;

pub async fn start(host: &str, key: &str, buff_size: usize) -> Result<()> {
  let listener = TcpListener::bind(host).await?;
  info!("Listening on {}", listener.local_addr()?);

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

async fn process(mut socket: TcpStream, db: &DB, mut rc4: Rc4, buff_size: usize) -> Result<()> {
  socket.set_keepalive()?;
  let (tcp_rx, mut tcp_tx) = socket.split();
  let mut tcp_rx = BufReader::with_capacity(10485760, tcp_rx);
  let (mpsc_tx, mut mpsc_rx) = mpsc::channel::<Vec<u8>>(buff_size);

  let f1 = async move {
    while let Some(msg) = mpsc_rx.recv().await {
      tcp_tx.write_msg(msg, &mut rc4).await?;
    };
    Ok(())
  };

  let f2 = async move {
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

            // 可能存在死锁
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
  };
  tokio::select! {
    res = f1 => res,
    res = f2 => res
  }
}

async fn child_channel_process(channel_id: &String, addr: Address,
                               mpsc_tx: &Sender<Vec<u8>>, mut child_rx: DuplexStream) -> Result<()> {
  let addr = tokio::net::lookup_host((addr.0.as_str(), addr.1)).await?.next().option_to_res("Target address error")?;

  let tcp_socket = match addr {
    SocketAddr::V4(_) => TcpSocket::new_v4(),
    SocketAddr::V6(_) => TcpSocket::new_v6()
  }?;
  tcp_socket.set_keepalive()?;

  let mut socket = tcp_socket.connect(addr).await?;
  let (mut tcp_rx, mut tcp_tx) = socket.split();

  let f1 = async move {
    let _ = tokio::io::copy(&mut child_rx, &mut tcp_tx).await?;
    Ok(())
  };

  let f2 = async move {
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
  };

  tokio::select! {
    res = f1 => res,
    res = f2 => res,
  }
}
