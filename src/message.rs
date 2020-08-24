use std::net::IpAddr;
use std::str::FromStr;

use bytes::{Buf, BufMut, BytesMut};
use bytes::buf::BufExt;
use tokio::io::{ErrorKind, Result};
use tokio::io::Error;
use tokio::net::tcp::OwnedReadHalf;
use tokio::prelude::*;

use crate::commons::Address;

const CONNECT: u8 = 0x00;
const DISCONNECT: u8 = 0x01;
const DATA: u8 = 0x03;

pub enum Msg {
  CONNECT(String, Address),
  DISCONNECT(String),
  DATA(String, BytesMut),
}

pub fn encode(msg: Msg) -> BytesMut {
  match msg {
    Msg::CONNECT(id, addr) => encode_connect_msg(addr, &id),
    Msg::DISCONNECT(id) => encode_disconnect_msg(&id),
    Msg::DATA(id, data) => encode_data_msg(&id, &data);
  }
}

fn encode_connect_msg(addr: Address, channel_id: &str) -> BytesMut {
  let mut buff = BytesMut::new();
  buff.put_u8(CONNECT);
  buff.put_slice(channel_id.as_bytes());

  let (host, port) = addr;

  buff.put_slice(host.as_bytes());
  buff.put_u16(port);
  buff
}

fn encode_disconnect_msg(channel_id: &str) -> BytesMut {
  let mut buff = BytesMut::new();
  buff.put_u8(DISCONNECT);
  buff.put_slice(channel_id.as_bytes());
  buff
}

fn encode_data_msg(channel_id: &str, data: &[u8]) -> BytesMut {
  let mut buff = BytesMut::new();
  buff.put_u8(DATA);
  buff.put_slice(channel_id.as_bytes());
  buff.put_slice(data);
  buff
}

pub fn decode(mut msg: BytesMut) -> Result<Msg> {
  let mode = msg.get_u8();
  let mut str = vec![0u8; 4];
  msg.copy_to_slice(&mut str);
  let channel_id = String::from_utf8(str).unwrap();

  let msg = match mode {
    CONNECT => {
      let mut host = vec![0u8; msg.len() - 2];
      msg.copy_to_slice(&mut host);
      let port = msg.get_u16();
      Msg::CONNECT(channel_id, (String::from_utf8(host).unwrap(), port))
    }
    DISCONNECT => Msg::DISCONNECT(channel_id),
    DATA => Msg::DATA(channel_id, msg),
    _ => return Err(Error::new(ErrorKind::Other, "Message error"))
  };
  Ok(msg)
}

pub async fn read_msg(socket: &mut OwnedReadHalf) -> Result<BytesMut> {
  let len = socket.read_u32().await?;
  let mut msg = BytesMut::with_capacity(len as usize);
  rx.read_exact(&mut msg).await?;
  Ok(msg)
}
