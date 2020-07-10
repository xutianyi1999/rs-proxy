use std::collections::HashMap;
use std::net::IpAddr;

use anyhow::Result;
use tokio::net::TcpStream;
use tokio::prelude::*;

pub async fn connect(host: String) -> Result<()> {
  let mut socket = TcpStream::connect(host).await?;
  let map: HashMap<String, TcpStream> = HashMap::new();

  tokio::spawn(async move {
    loop {
      let msg_len = socket.read_u32().await?;

      let mut buffer = vec![0u8; msg_len as usize];
      socket.read_exact(&mut buffer).await?;
    }
  });
  Ok(())
}

impl MuxChannel {}
