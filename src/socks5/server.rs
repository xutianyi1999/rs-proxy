use std::convert::TryInto;
use std::net::{IpAddr, SocketAddr};

use ::tokio::io::ErrorKind::*;
use anyhow::Result;
use bytes::{BufMut, BytesMut};
use tokio::net::TcpStream;
use tokio::prelude::*;

use crate::socks5::Socks5Server;

const SOCKS5_VERSION: u8 = 0x05;
const NO_AUTH: u8 = 0x00;
const CMD_CONNECT: u8 = 0x01;

const IPV4: u8 = 0x01;
const IPV6: u8 = 0x02;
const DOMAIN_NAME: u8 = 0x03;

impl Socks5Server<'_> {
  pub async fn initial_request(&mut self) -> Result<()> {
    let mut buffer = [0u8; 2];
    self.socket.read_exact(&mut buffer).await?;

    if buffer[0] != SOCKS5_VERSION {
      return Err(anyhow!("INVALID PROTOCOL VERSION"));
    }

    let mut discard = Vec::with_capacity(buffer[1] as usize);
    self.socket.read_exact(&mut discard).await?;
    drop(discard);

    self.socket.write_all(&[SOCKS5_VERSION, NO_AUTH]).await?;
    Ok(())
  }

  pub async fn command_request(&mut self) -> Result<Address> {
    let mut buffer = [0u8; 4];
    self.socket.read_exact(&mut buffer).await?;

    if buffer[0] != SOCKS5_VERSION {
      return Err(anyhow!("INVALID PROTOCOL VERSION"));
    }

    if buffer[1] != CMD_CONNECT {
      self.write_err_reply(0x07).await?;
      return Err(anyhow!("UNSUPPORTED COMMAND"));
    }

    if buffer[2] != 0x00 {
      return Err(anyhow!("INVALID RESERVED DATA"));
    }

    match buffer[3] {
      IPV4 => {
        let mut buffer = [0u8; 6];
        self.socket.read_exact(&mut buffer).await?;

        let addr: [u8; 4] = buffer[..4].try_into()?;
        let port: [u8; 2] = buffer[..4].try_into()?;

        Ok(Address::IP(IpAddr::from(addr), u16::from_be_bytes(port)))
      }
      IPV6 => {
        let mut buffer = [0u8; 18];
        self.socket.read_exact(&mut buffer).await?;

        let addr: [u8; 16] = buffer[..16].try_into()?;
        let port: [u8; 2] = buffer[16..].try_into()?;

        Ok(Address::IP(IpAddr::from(addr), u16::from_be_bytes(port)))
      }
      DOMAIN_NAME => {
        let len = self.socket.read_u8().await? as usize;

        let mut buffer: Vec<u8> = Vec::with_capacity(len + 2);
        self.socket.read_buf(&mut buffer).await?;

        let domain_name = String::from_utf8(buffer[..len].to_vec())?;
        let port: [u8; 2] = buffer[len..].try_into()?;

        Ok(Address::DOMAIN(domain_name, u16::from_be_bytes(port)))
      }
      _ => {
        self.write_err_reply(0x08).await?;
        Err(anyhow!("INVALID ADDRESS TYPE"))
      }
    }
  }

  pub async fn reply_request(&mut self, address: Address) -> Result<()> {
    let r = match address {
      Address::IP(addr, port) => {
        TcpStream::connect((addr, port)).await
      }
      Address::DOMAIN(domain, port) => {
        TcpStream::connect(format!("{}:{}", domain, port)).await
      }
    };

    match r {
      Ok(mut dst_socket) => {
        let remote_address = dst_socket.peer_addr()?;
        let mut buffer = BytesMut::with_capacity(10);

        buffer.put_slice(&[0x05, 0x00, 0x00]);

        match remote_address {
          SocketAddr::V4(ipv4_addr) => {
            buffer.put_u8(0x01);
            buffer.put_slice(&ipv4_addr.ip().octets());
            buffer.put_u16(ipv4_addr.port());
          }
          SocketAddr::V6(ipv6_addr) => {
            buffer.put_u8(0x04);
            buffer.put_slice(&ipv6_addr.ip().octets());
            buffer.put_u16(ipv6_addr.port());
          }
        }

        self.socket.write_all(&buffer).await?;

        let (mut local_reader, mut local_writer) = self.socket.split();
        let (mut dst_reader, mut dst_writer) = dst_socket.split();

        let future1 = io::copy(&mut local_reader, &mut dst_writer);
        let future2 = io::copy(&mut dst_reader, &mut local_writer);
        tokio::try_join!(future1, future2)?;
        Ok(())
      }
      Err(e) => {
        let err_code = match e.kind() {
          NotFound | NotConnected => 0x03,
          PermissionDenied => 0x02,
          ConnectionRefused => 0x05,
          ConnectionAborted | ConnectionReset => 0x04,
          AddrNotAvailable => 0x08,
          TimedOut => 0x06,
          _ => 0x01,
        };

        self.write_err_reply(err_code).await?;
        Err(anyhow!(e))
      }
    }
  }

  async fn write_err_reply(&mut self, rsp: u8) -> Result<()> {
    let data = [0x05, rsp, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    self.socket.write_all(&data).await?;
    Ok(())
  }
}

pub enum Address {
  IP(IpAddr, u16),
  DOMAIN(String, u16),
}
