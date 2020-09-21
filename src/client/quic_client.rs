use std::net::SocketAddr;
use std::sync::Arc;

use bytes::{BufMut, BytesMut};
use quinn::{Connection, Endpoint};
use tokio::io::{Error, ErrorKind};
use tokio::io::Result;
use tokio::net::TcpStream;
use tokio::stream::StreamExt;
use tokio::sync::RwLock;
use yaml_rust::Yaml;

use crate::client::Channel;
use crate::client::CONNECTION_POOL;
use crate::commons::{Address, OptionConvert, quic_config, StdResAutoConvert, StdResConvert};
use crate::CONFIG_ERROR;

pub async fn start(local_addr: &str, host_list: Vec<&Yaml>) -> Result<()> {
  let cert_paths = host_list.iter()
    .map(|e| e["cert"].as_str().unwrap().to_string())
    .collect();

  let client_config = quic_config::configure_client(cert_paths).await?;
  let mut builder = Endpoint::builder();
  builder.default_client_config(client_config);

  let (endpoint, _) = builder.bind(&local_addr.parse().res_auto_convert()?)
    .res_convert(|_| "Udp client bind error".to_string())?;

  let endpoint = Arc::new(endpoint);

  for host in host_list {
    let quic_channel = QuicChannel::new(
      endpoint.clone(),
      host["host"].as_str().option_to_res(CONFIG_ERROR)?.parse().res_auto_convert()?,
      host["server_name"].as_str().option_to_res(CONFIG_ERROR)?.to_string(),
    );
    let quic_channel = Channel::Quic(quic_channel);
    CONNECTION_POOL.lock().res_auto_convert()?.put(nanoid!(4), quic_channel);
  };
  Ok(())
}

pub struct QuicChannel {
  endpoint: Arc<Endpoint>,
  remote_addr: SocketAddr,
  server_name: String,
  conn: Arc<RwLock<Option<Connection>>>,
}

impl QuicChannel {
  fn new(endpoint: Arc<Endpoint>, remote_addr: SocketAddr, server_name: String) -> QuicChannel {
    QuicChannel {
      endpoint,
      remote_addr,
      server_name,
      conn: Arc::new(RwLock::new(Option::None)),
    }
  }

  async fn connect(&self) -> Result<()> {
    let mut conn_lock_guard = self.conn.write().await;

    if conn_lock_guard.is_none() {
      let conn = self.endpoint.connect(&self.remote_addr, &self.server_name)
        .res_convert(|_| "Connection error".to_string())?.await?;

      let connection = conn.connection;
      let mut uni = conn.uni_streams;

      let inner_conn = self.conn.clone();

      tokio::spawn(async move {
        let _ = uni.next().await;
        *inner_conn.write().await = Option::None;
      });

      *conn_lock_guard = Some(connection);
    }
    Ok(())
  }

  pub async fn open_bi(&self, socket: TcpStream, remote_addr: Address) -> Result<()> {
    let op_lock_guard = self.conn.read().await;
    let op = &*op_lock_guard;

    if let Some(conn) = op {
      QuicChannel::f(socket, remote_addr, conn).await
    } else {
      drop(op_lock_guard);
      self.connect().await?;

      if let Some(conn) = &*self.conn.read().await {
        QuicChannel::f(socket, remote_addr, conn).await
      } else {
        Err(Error::new(ErrorKind::Other, "Open bi error"))
      }
    }
  }

  async fn f(mut socket: TcpStream, remote_addr: Address, conn: &Connection) -> Result<()> {
    let (mut quic_tx, mut quic_rx) = conn.open_bi().await?;
    let (mut tcp_rx, mut tcp_tx) = socket.split();

    let (host, port) = remote_addr;
    let mut buff = BytesMut::new();
    buff.put_u8(host.len() as u8 + 2);
    buff.put_slice(host.as_bytes());
    buff.put_u16(port);
    quic_tx.write_all(&buff).await?;

    let f1 = tokio::io::copy(&mut tcp_rx, &mut quic_tx);
    let f2 = tokio::io::copy(&mut quic_rx, &mut tcp_tx);

    tokio::select! {
        _ = f1 => (),
        _ = f2 => ()
      }
    Ok(())
  }
}
