#[macro_use]
extern crate anyhow;

use std::env;
use std::net::SocketAddr;

use anyhow::Result;
use tokio::net::{TcpListener, TcpStream};

mod socks5;

#[tokio::main]
async fn main() -> Result<()> {
  let addr = env::args().nth(1).unwrap_or("127.0.0.1:19998".to_string());
  let addr = addr.parse::<SocketAddr>()?;

  println!("bind: {:?}", addr);
  let mut listener = TcpListener::bind(addr).await?;

  loop {
    let (socket, _) = listener.accept().await?;

    tokio::spawn(async move {
      let result = handler(socket).await;

      if let Err(err) = result {
        eprintln!("connection error: {:?}", err);
      }
    });
  }
}

async fn handler(mut socket: TcpStream) -> Result<()> {
  let mut socks5_server = socks5::server(&mut socket);
  socks5_server.initial_request().await?;
  let dst_address = socks5_server.command_request().await?;
  socks5_server.reply_request(dst_address).await?;
  Ok(())
}
