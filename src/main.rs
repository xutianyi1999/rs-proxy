#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate nanoid;

use std::env;

use tokio::fs;
use tokio::io::{Error, ErrorKind, Result};
use yaml_rust::YamlLoader;

mod client;
mod server;
mod commons;
mod message;

#[tokio::main]
async fn main() -> Result<()> {
  let mut args = env::args();
  args.next();

  if let Some(mode) = args.next() {
    let config_path = args.next().unwrap();
    let config = fs::read_to_string(config_path).await?;

    let config = &YamlLoader::load_from_str(&config).unwrap()[0];

    match mode.as_str() {
      "client" => {
        let bind_addr = config["host"].as_str().unwrap();
        let host_list = config["remote"].as_vec().unwrap();

        if let Err(e) = client::start(bind_addr, host_list).await {
          eprintln!("{:?}", e)
        }
      }
      _ => {
        let host = config["host"].as_str().unwrap();
        let key = config["key"].as_str().unwrap();

        if let Err(e) = server::start(host, key).await {
          eprintln!("{:?}", e);
        }
      }
    }
    Ok(())
  } else {
    Err(Error::new(ErrorKind::Other, "Args error"))
  }
}
