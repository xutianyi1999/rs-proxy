use std::net::IpAddr;

pub enum Address {
  IP(IpAddr, u16),
  DOMAIN(String, u16),
}
