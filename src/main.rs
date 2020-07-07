#[macro_use]
extern crate anyhow;

use anyhow::Result;
use tokio::net::TcpListener;

mod socks5;

#[tokio::main]
async fn main() -> Result<()> {
  let mut listener = TcpListener::bind("127.0.0.1:19998").await?;

  loop {
    let (mut socket, _) = listener.accept().await?;

    tokio::spawn(async move {
      let mut socks5_server = socks5::server(&mut socket);
      socks5_server.initial_request().await?;
      let dst_address = socks5_server.command_request().await?;
      socks5_server.reply_request(dst_address).await
    });
  }
}
