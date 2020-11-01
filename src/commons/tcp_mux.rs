use async_trait::async_trait;
use bytes::{Buf, BufMut, Bytes};
use crypto::buffer::{RefReadBuffer, RefWriteBuffer};
use crypto::rc4::Rc4;
use crypto::symmetriccipher::{Decryptor, Encryptor};
use socket2::Socket;
use tokio::io::{BufReader, ErrorKind, Result};
use tokio::io::Error;
use tokio::net::{TcpSocket, TcpStream};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::prelude::*;
use tokio::prelude::io::AsyncWriteExt;
use tokio::time::Duration;

use crate::commons::{Address, StdResAutoConvert, StdResConvert};

const CONNECT: u8 = 0x00;
const DISCONNECT: u8 = 0x01;
const DATA: u8 = 0x03;

pub enum Msg {
  CONNECT(String, Address),
  DISCONNECT(String),
  DATA(String, Vec<u8>),
}

impl Msg {
  pub fn encode(self) -> Vec<u8> {
    encode(self)
  }
}

fn encode(msg: Msg) -> Vec<u8> {
  match msg {
    Msg::CONNECT(id, addr) => encode_connect_msg(addr, id),
    Msg::DISCONNECT(id) => encode_disconnect_msg(id),
    Msg::DATA(id, data) => encode_data_msg(id, &data)
  }
}

fn encode_connect_msg(addr: Address, channel_id: String) -> Vec<u8> {
  let (host, port) = addr;
  let mut buff = Vec::with_capacity(5 + 2 + host.len());

  buff.put_u8(CONNECT);
  buff.put_slice(channel_id.as_bytes());

  buff.put_slice(host.as_bytes());
  buff.put_u16(port);
  buff
}

fn encode_disconnect_msg(channel_id: String) -> Vec<u8> {
  let mut buff = Vec::with_capacity(5);
  buff.put_u8(DISCONNECT);
  buff.put_slice(channel_id.as_bytes());
  buff
}

fn encode_data_msg(channel_id: String, data: &[u8]) -> Vec<u8> {
  let mut buff = Vec::with_capacity(5 + data.len());
  buff.put_u8(DATA);
  buff.put_slice(channel_id.as_bytes());
  buff.put_slice(data);
  buff
}

fn decode(data: Vec<u8>) -> Result<Msg> {
  let mut msg = Bytes::from(data);
  let mode = msg.get_u8();
  let mut str = vec![0u8; 4];
  msg.copy_to_slice(&mut str);
  let channel_id = String::from_utf8(str).res_auto_convert()?;

  let msg = match mode {
    CONNECT => {
      let mut host = vec![0u8; msg.len() - 2];
      msg.copy_to_slice(&mut host);
      let port = msg.get_u16();
      Msg::CONNECT(channel_id, (String::from_utf8(host).res_auto_convert()?, port))
    }
    DISCONNECT => Msg::DISCONNECT(channel_id),
    DATA => Msg::DATA(channel_id, msg.to_vec()),
    _ => return Err(Error::new(ErrorKind::Other, "Message error"))
  };
  Ok(msg)
}

async fn read_msg(rx: &mut BufReader<OwnedReadHalf>) -> Result<Vec<u8>> {
  let len = rx.read_u16().await?;
  let mut msg = vec![0u8; len as usize];
  rx.read_exact(&mut msg).await?;
  Ok(msg)
}

pub fn create_channel_id() -> String {
  nanoid!(4)
}

#[async_trait]
pub trait MsgWriteHandler {
  async fn write_msg(&mut self, msg: Vec<u8>, rc4: &mut Rc4) -> Result<()>;
}

#[async_trait]
impl MsgWriteHandler for OwnedWriteHalf {
  async fn write_msg(&mut self, msg: Vec<u8>, rc4: &mut Rc4) -> Result<()> {
    let msg = crypto(&msg, rc4, MODE::ENCRYPT)?;

    let mut data = Vec::with_capacity(msg.len() + 2);
    data.put_u16(msg.len() as u16);
    data.put_slice(&msg);

    self.write_all(&data).await
  }
}

#[async_trait]
pub trait MsgReadHandler {
  async fn read_msg(&mut self, rc4: &mut Rc4) -> Result<Msg>;
}

#[async_trait]
impl MsgReadHandler for BufReader<OwnedReadHalf> {
  async fn read_msg(&mut self, rc4: &mut Rc4) -> Result<Msg> {
    let data = read_msg(self).await?;
    let data = crypto(&data, rc4, MODE::DECRYPT)?;
    decode(data)
  }
}

enum MODE {
  ENCRYPT,
  DECRYPT,
}

fn crypto<'a>(input: &'a [u8], rc4: &'a mut Rc4, mode: MODE) -> Result<Vec<u8>> {
  let mut ref_read_buf = RefReadBuffer::new(input);
  let mut out = vec![0u8; input.len()];
  let mut ref_write_buf = RefWriteBuffer::new(&mut out);

  match mode {
    MODE::DECRYPT => {
      rc4.decrypt(&mut ref_read_buf, &mut ref_write_buf, false)
        .res_convert(|_| "Decrypt error".to_string())?;
    }
    MODE::ENCRYPT => {
      rc4.encrypt(&mut ref_read_buf, &mut ref_write_buf, false)
        .res_convert(|_| "Encrypt error".to_string())?;
    }
  };
  Ok(out)
}

pub trait TcpSocketExt {
  fn set_keepalive(&self) -> tokio::io::Result<()>;
}

impl TcpSocketExt for TcpStream {
  fn set_keepalive(&self) -> tokio::io::Result<()> {
    set_keepalive(self)
  }
}

impl TcpSocketExt for TcpSocket {
  fn set_keepalive(&self) -> tokio::io::Result<()> {
    set_keepalive(self)
  }
}

const KEEPALIVE_DURATION: Option<Duration> = Option::Some(Duration::from_secs(120));

#[cfg(target_os = "windows")]
fn set_keepalive<S: std::os::windows::io::AsRawSocket>(socket: &S) -> tokio::io::Result<()> {
  use std::os::windows::io::FromRawSocket;

  unsafe {
    let socket = Socket::from_raw_socket(socket.as_raw_socket());
    socket.set_keepalive(KEEPALIVE_DURATION)?;
    std::mem::forget(socket);
  };
  Ok(())
}

#[cfg(target_os = "linux")]
fn set_keepalive<S: std::os::unix::io::AsRawFd>(socket: &S) -> tokio::io::Result<()> {
  use std::os::unix::io::FromRawFd;

  unsafe {
    let socket = Socket::from_raw_fd(socket.as_raw_socket());
    socket.set_keepalive(KEEPALIVE_DURATION)?;
    std::mem::forget(socket);
  };
  Ok(())
}

