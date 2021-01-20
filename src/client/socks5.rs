use std::net::IpAddr;

use tokio::io::{AsyncReadExt, AsyncWriteExt, Error, ErrorKind, Result};
use tokio::net::TcpStream;

use crate::commons::Address;

const SOCKS5_VERSION: u8 = 0x05;
const NO_AUTH: u8 = 0x00;
const CMD_CONNECT: u8 = 0x01;

const IPV4: u8 = 0x01;
const IPV6: u8 = 0x04;
const DOMAIN_NAME: u8 = 0x03;

pub async fn initial_request(socket: &mut TcpStream) -> Result<()> {
  let mut buffer = [0u8; 2];
  socket.read_exact(&mut buffer).await?;

  if buffer[0] != SOCKS5_VERSION {
    return Err(Error::new(ErrorKind::Other, "INVALID PROTOCOL VERSION"));
  }

  socket.read_exact(&mut vec![0u8; buffer[1] as usize]).await?;
  socket.write_all(&[SOCKS5_VERSION, NO_AUTH]).await?;
  Ok(())
}

pub async fn command_request(socket: &mut TcpStream) -> Result<Address> {
  let mut buffer = [0u8; 4];
  socket.read_exact(&mut buffer).await?;

  if buffer[0] != SOCKS5_VERSION {
    return Err(Error::new(ErrorKind::Other, "INVALID PROTOCOL VERSION"));
  }

  if buffer[1] != CMD_CONNECT {
    write_err_reply(socket, 0x07).await?;
    return Err(Error::new(ErrorKind::Other, "UNSUPPORTED COMMAND"));
  }

  if buffer[2] != 0x00 {
    return Err(Error::new(ErrorKind::Other, "INVALID RESERVED DATA"));
  }

  let address = match buffer[3] {
    IPV4 => {
      let mut addr = [0u8; 4];
      socket.read_exact(&mut addr).await?;
      let ipaddr = IpAddr::from(addr);

      let port = socket.read_u16().await?;
      (Vec::from(ipaddr.to_string()), port)
    }
    IPV6 => {
      let mut addr = [0u8; 16];
      socket.read_exact(&mut addr).await?;
      let ipaddr = IpAddr::from(addr);

      let port = socket.read_u16().await?;
      (Vec::from(ipaddr.to_string()), port)
    }
    DOMAIN_NAME => {
      let len = socket.read_u8().await? as usize;

      let mut domain = vec![0u8; len];
      socket.read_exact(&mut domain).await?;
      let port = socket.read_u16().await?;

      (domain, port)
    }
    _ => {
      write_err_reply(socket, 0x08).await?;
      return Err(Error::new(ErrorKind::Other, "INVALID ADDRESS TYPE"));
    }
  };

  let response = [0x05, 0x00, 0x00, IPV4, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
  socket.write_all(&response).await?;
  Ok(address)
}

async fn write_err_reply(socket: &mut TcpStream, rsp: u8) -> Result<()> {
  let data = [0x05, rsp, 0x00, IPV4, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
  socket.write_all(&data).await?;
  Ok(())
}
