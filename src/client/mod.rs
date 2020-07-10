use anyhow::Result;
use tokio::net::{TcpListener, TcpStream};

mod socks5;
mod mux;

pub async fn build(address: &str) -> Result<()> {
  let mut listener = TcpListener::bind(address).await?;

  loop {
    let (mut socket, _) = listener.accept().await?;

    tokio::spawn(async move {
      let res = process(&mut socket).await;

      if let Err(err) = res {
        eprintln!("connection error: {:?}", err);
      }
    });
  }
  Ok(())
}

async fn process(client_socket: &mut TcpStream) -> Result<()> {
  socks5::initial_request(client_socket).await?;
  let addr = socks5::command_request(client_socket).await?;
  Ok(())
}
