use std::net::IpAddr;

use nanoid;

pub enum Address {
  IP(IpAddr, u16),
  DOMAIN(String, u16),
}

pub fn create_channel_id() -> String {
  nanoid!(4)
}
