use std::net::SocketAddr;
use std::str::FromStr;

use crypto::rc4::Rc4;
use tokio::io::{Error, ErrorKind, Result};
use yaml_rust::Yaml;

use crate::commons::{OptionConvert, StdResAutoConvert};
use crate::CONFIG_ERROR;

mod tcpmux_server;
mod tcp_server;

pub async fn start(config: &Yaml) -> Result<()> {
  let host = config["host"].as_str().option_to_res(CONFIG_ERROR)?;
  let protocol = config["protocol"].as_str().option_to_res(CONFIG_ERROR)?;
  let key = config["key"].as_str().option_to_res(CONFIG_ERROR)?;

  match protocol {
    "tcp" => {
      let buff_size = config["buffSize"].as_i64().option_to_res(CONFIG_ERROR)?;
      let channel_capacity = config["channelCapacity"].as_i64().option_to_res(CONFIG_ERROR)?;
      tcpmux_server::start(host, key, buff_size as usize, channel_capacity as usize).await
    }
    "tcpmux" => {
      let buff_size = config["buffSize"].as_i64().option_to_res(CONFIG_ERROR)?;
      tcp_server::start(SocketAddr::from_str(host).res_auto_convert()?, Rc4::new(key.as_bytes()), buff_size as usize).await
    }
    _ => Err(Error::new(ErrorKind::Other, CONFIG_ERROR))
  }
}

