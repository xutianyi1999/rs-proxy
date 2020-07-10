use std::convert::TryInto;
use std::net::{IpAddr, Ipv4Addr};

use bytes::{BufMut, Bytes, BytesMut};

use crate::commons::Address;
use crate::commons::Address::{DOMAIN, IP};

pub const CONNECT: u8 = 0x01;
pub const DISCONNECT: u8 = 0x02;
pub const DATA: u8 = 0x03;
pub const HEARTBEAT: u8 = 0x04;

pub fn connect_msg_build(addr: Address, channel_id: String) -> BytesMut {
  let mut buffer = BytesMut::new();

  buffer.put_u8(CONNECT);
  buffer.put_slice(channel_id.as_bytes());

  let (addr, port) = match addr {
    IP(host, port) => {
      let addr_bytes = match host {
        IpAddr::V4(addr) => { Vec::from(addr.octets()) }
        IpAddr::V6(addr) => { Vec::from(addr.octets()) }
      };

      (addr_bytes, port)
    }
    DOMAIN(mut domain_name, port) => {
      (Vec::from(domain_name), port)
    }
  };

  buffer.put_slice(&addr);
  buffer.put_u8(b':');
  buffer.put_u16(port);
  buffer
}

pub fn disconnect_msg_build(channel_id: String) -> BytesMut {
  let mut buffer = BytesMut::new();

  buffer.put_u8(DISCONNECT);
  buffer.put_slice(channel_id.as_bytes());
  buffer
}

pub fn data_msg_build(data: Vec<u8>, channel_id: String) -> BytesMut {
  let mut buffer = BytesMut::new();

  buffer.put_slice(&data);
  buffer.put_slice(channel_id.as_bytes());
  buffer
}
