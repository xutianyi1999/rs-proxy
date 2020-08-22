use std::net::IpAddr;

use bytes::{Buf, BufMut, BytesMut};
use bytes::buf::BufExt;
use tokio::io::{ErrorKind, Result};
use tokio::io::Error;

use crate::commons::Address;

const CONNECT: u8 = 0x00;
const DISCONNECT: u8 = 0x01;
const DATA: u8 = 0x03;

pub enum Msg {
  CONNECT(String),
  DISCONNECT(String),
  DATA(String, BytesMut),
}

pub fn encode_connect_msg(addr: Address, channel_id: &String) -> BytesMut {
  let mut buff = BytesMut::new();
  buff.put_u8(CONNECT);
  buff.put_slice(channel_id.as_bytes());

  match addr {
    Address::IP(ip, port) => {
      buff.put_slice(ip.to_string().as_bytes());
      buff.put_u16(port)
    }
    Address::DOMAIN(domain, port) => {
      buff.put_slice(domain.as_bytes());
      buff.put_u16(port)
    }
  }
  buff
}

pub fn encode_disconnect_msg(channel_id: &String) -> BytesMut {
  let mut buff = BytesMut::new();
  buff.put_u8(DISCONNECT);
  buff.put_slice(channel_id.as_bytes());
  buff
}

pub fn encode_data_msg(channel_id: &String, data: &[u8]) -> BytesMut {
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
    CONNECT => Msg::CONNECT(channel_id),
    DISCONNECT => Msg::DISCONNECT(channel_id),
    DATA => Msg::DATA(channel_id, msg),
    _ => return Err(Error::new(ErrorKind::Other, "Message error"))
  };
  Ok(msg)
}
