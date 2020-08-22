use std::rc::Rc;
use std::sync::{Arc, Mutex};

use dashmap::DashMap;
use tokio::io::Result;
use tokio::net::TcpStream;

use client_mux::ClientMuxChannel;

use crate::commons;

mod client_mux;
mod client_proxy;
mod socks5;

pub async fn start() -> Result<()> {
  let mut connection_map: DashMap<String, Arc<ClientMuxChannel>> = DashMap::new();
  let (rx, tx) = TcpStream::connect("127.0.0.1:12345").await?.into_split();
  let cmc = Arc::new(ClientMuxChannel::new(tx));
  let channel_id = commons::create_channel_id();

  connection_map.insert(channel_id.clone(), cmc.clone());
  tokio::spawn(async move {
    let mut rx = rx;
    let mut cmc = cmc;

    loop {
      match cmc.recv_process(rx).await {
        Ok(_) => (),
        Err(e) => {
          eprintln!("{:?}", e);
        }
      };
      connection_map.remove(&channel_id);
      let (rx2, tx) = TcpStream::connect("127.0.0.1:12345").await?.into_split();
      rx = rx2;
      cmc = Arc::new(ClientMuxChannel::new(tx));
    }
  });
  Ok(())
}
