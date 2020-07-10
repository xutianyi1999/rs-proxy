use std::convert::TryInto;
use std::net::{IpAddr, SocketAddr};

use anyhow::Result;
use bytes::{BufMut, BytesMut};
use tokio::io::ErrorKind::*;
use tokio::net::TcpStream;
use tokio::prelude::*;

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
    return Err(anyhow!("INVALID PROTOCOL VERSION"));
  }

  let mut discard = vec![0u8; buffer[1] as usize];
  socket.read_exact(&mut discard).await?;
  drop(discard);

  socket.write_all(&[SOCKS5_VERSION, NO_AUTH]).await?;
  Ok(())
}

pub async fn command_request(socket: &mut TcpStream) -> Result<Address> {
  let mut buffer = [0u8; 4];
  socket.read_exact(&mut buffer).await?;

  if buffer[0] != SOCKS5_VERSION {
    return Err(anyhow!("INVALID PROTOCOL VERSION"));
  }

  if buffer[1] != CMD_CONNECT {
    write_err_reply(socket, 0x07).await?;
    return Err(anyhow!("UNSUPPORTED COMMAND"));
  }

  if buffer[2] != 0x00 {
    return Err(anyhow!("INVALID RESERVED DATA"));
  }

  let address = match buffer[3] {
    IPV4 => {
      let mut buffer = [0u8; 6];
      socket.read_exact(&mut buffer).await?;

      let addr: [u8; 4] = buffer[..4].try_into()?;
      let port: [u8; 2] = buffer[4..].try_into()?;

      Address::IP(IpAddr::from(addr), u16::from_be_bytes(port))
    }
    IPV6 => {
      let mut buffer = [0u8; 18];
      socket.read_exact(&mut buffer).await?;

      let addr: [u8; 16] = buffer[..16].try_into()?;
      let port: [u8; 2] = buffer[16..].try_into()?;

      Address::IP(IpAddr::from(addr), u16::from_be_bytes(port))
    }
    DOMAIN_NAME => {
      let len = socket.read_u8().await? as usize;

      let mut buffer: Vec<u8> = vec![0u8; len + 2];
      socket.read_exact(&mut buffer).await?;

      let domain_name = String::from_utf8(Vec::from(&buffer[..len]))?;
      let port: [u8; 2] = buffer[len..].try_into()?;

      Address::DOMAIN(domain_name, u16::from_be_bytes(port))
    }
    _ => {
      write_err_reply(socket, 0x08).await?;
      return Err(anyhow!("INVALID ADDRESS TYPE"));
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
