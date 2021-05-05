use std::net::SocketAddr;
use std::str::FromStr;

use tokio::io::{Error, ErrorKind, Result};
use tokio_rustls::rustls::{NoClientAuth, ServerConfig};
use yaml_rust::Yaml;

use crypto::rc4::Rc4;

use crate::commons::{load_certs, load_priv_key, OptionConvert, StdResAutoConvert};
use crate::CONFIG_ERROR;

mod tcpmux_server;
mod tcp_server;

pub async fn start(config: &Yaml) -> Result<()> {
  let host = config["host"].as_str().option_to_res(CONFIG_ERROR)?;
  let protocol = config["protocol"].as_str().option_to_res(CONFIG_ERROR)?;

  let mut tls_config = ServerConfig::new(NoClientAuth::new());
  let certs = load_certs(config["certPath"].as_str().option_to_res(CONFIG_ERROR)?)?;
  let priv_key = load_priv_key(config["privKeyPath"].as_str().option_to_res(CONFIG_ERROR)?)?;
  tls_config.set_single_cert(certs, priv_key)?;

  match protocol {
    "tcpmux" => {
      let buff_size = config["buffSize"].as_i64().option_to_res(CONFIG_ERROR)?;
      let channel_capacity = config["channelCapacity"].as_i64().option_to_res(CONFIG_ERROR)?;
      tcpmux_server::start(
        host,
        key,
        buff_size as usize,
        channel_capacity as usize,
      ).await
    }
    "tcp" => {
      tcp_server::start(
        SocketAddr::from_str(host).res_auto_convert()?,
        tls_config,
      ).await
    }
    _ => Err(Error::new(ErrorKind::Other, CONFIG_ERROR))
  }
}

