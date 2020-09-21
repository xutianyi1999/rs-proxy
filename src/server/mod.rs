use tokio::io::{Error, ErrorKind, Result};
use yaml_rust::Yaml;

use crate::commons::OptionConvert;
use crate::CONFIG_ERROR;

mod tcp_server;
mod quic_server;

pub async fn start(config: &Yaml) -> Result<()> {
  let host = config["host"].as_str().option_to_res(CONFIG_ERROR)?;
  let protocol = config["protocol"].as_str().option_to_res(CONFIG_ERROR)?;

  match protocol {
    "tcp" => {
      let key = config["key"].as_str().option_to_res(CONFIG_ERROR)?;
      let buff_size = config["buff_size"].as_i64().option_to_res(CONFIG_ERROR)?;
      tcp_server::start(host, key, buff_size as usize).await
    }
    "quic" => {
      let cert_path = config["cert"].as_str().option_to_res(CONFIG_ERROR)?;
      let priv_key_path = config["private_key"].as_str().option_to_res(CONFIG_ERROR)?;
      quic_server::start(host, cert_path, priv_key_path).await
    }
    _ => Err(Error::new(ErrorKind::Other, CONFIG_ERROR))
  }
}

