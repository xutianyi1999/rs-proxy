use tokio::net::TcpStream;

mod server;

pub struct Socks5Server<'a> {
  socket: &'a mut TcpStream
}

pub fn server(socket: &mut TcpStream) -> Socks5Server {
  Socks5Server { socket }
}
