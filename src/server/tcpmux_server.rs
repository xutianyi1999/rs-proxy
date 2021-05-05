use std::collections::HashMap;
use std::io::{Error, ErrorKind};
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream, Result, split};
use tokio::net::{TcpListener, TcpSocket, TcpStream};
use tokio::sync::{mpsc, Mutex};
use tokio::sync::mpsc::Sender;
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::TlsAcceptor;

use crypto::rc4::Rc4;

use crate::commons::{Address, MAGIC_CODE, OptionConvert, StdResAutoConvert, TcpSocketExt};
use crate::commons::tcpmux_comm::{ChannelId, Msg, MsgReader, MsgWriter};

type DB = Arc<Mutex<HashMap<ChannelId, DuplexStream>>>;

pub async fn start(
  host: &str,
  tls_config: ServerConfig,
  buff_size: usize,
  channel_capacity: usize,
) -> Result<()> {
  let tls_acceptor = TlsAcceptor::from(Arc::new(tls_config));
  let listener = TcpListener::bind(host).await?;
  info!("Listening on {}", listener.local_addr()?);

  while let Ok((stream, addr)) = listener.accept().await {
    let inner_acceptor = tls_acceptor.clone();

    tokio::spawn(async move {
      info!("{:?} connected", addr);

      if let Err(e) = process(
        stream,
        inner_acceptor,
        buff_size,
        channel_capacity,
      ).await {
        error!("{}", e);
      };
    });
  };
  Ok(())
}

async fn process(
  mut tcp_stream: TcpStream,
  tls_acceptor: TlsAcceptor,
  buff_size: usize,
  channel_capacity: usize,
) -> Result<()> {
  tcp_stream.set_keepalive()?;
  let tls_stream = tls_acceptor.accept(socket);

  let (tls_rx, tls_tx) = split(tls_stream);
  let tls_rx = BufReader::new(tls_rx);
  let (mpsc_tx, mut mpsc_rx) = mpsc::channel::<Vec<u8>>(channel_capacity);

  let outer_db: DB = Arc::new(Mutex::new(HashMap::new()));
  let db = &outer_db;

  let f1 = async move {
    let mut msg_writer = MsgWriter::new(tls_tx);

    while let Some(msg) = mpsc_rx.recv().await {
      msg_writer.write_msg(&msg).await?;
    };
    Ok(())
  };

  let f2 = async move {
    let mut msg_reader = MsgReader::new(tls_rx);

    while let Some(msg) = msg_reader.read_msg().await? {
      match msg {
        Msg::Connect(channel_id, addr) => {
          let (child_rx, child_tx) = tokio::io::duplex(buff_size);
          db.lock().await.insert(channel_id, child_tx);

          let db = db.clone();
          let mpsc_tx = mpsc_tx.clone();

          tokio::spawn(async move {
            if let Err(e) = child_channel_process(channel_id, addr, &mpsc_tx, child_rx).await {
              error!("{}", e);
            }

            if !mpsc_tx.is_closed() {
              if db.lock().await.remove(&channel_id).is_some() {
                if let Err(e) = mpsc_tx.send(Msg::Disconnect(channel_id).encode()).await {
                  error!("{}", e);
                }
              }
            }
          });
        }
        Msg::Disconnect(channel_id) => {
          db.lock().await.remove(&channel_id);
        }
        Msg::Data(channel_id, data) => {
          if let Some(tx) = db.lock().await.get_mut(&channel_id) {
            if let Err(e) = tx.write_all(data).await {
              error!("{}", e)
            }
          }
        }
      };
    }
    Ok(())
  };

  let res = tokio::select! {
    res = f1 => res,
    res = f2 => res
  };

  outer_db.lock().await.clear();
  res
}

async fn child_channel_process(
  channel_id: ChannelId,
  addr: Address,
  mpsc_tx: &Sender<Vec<u8>>,
  mut child_rx: DuplexStream,
) -> Result<()> {
  let addr = tokio::net::lookup_host((String::from_utf8(addr.0).res_auto_convert()?, addr.1)).await?
    .next().option_to_res("Target address error")?;

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

      mpsc_tx.send(Msg::Data(channel_id, slice).encode()).await
        .res_auto_convert()?;
    };
  };

  tokio::select! {
    res = f1 => res,
    res = f2 => res,
  }
}
