#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
#[macro_use]
extern crate nanoid;

use std::env;

use log4rs::append::console::ConsoleAppender;
use log4rs::config::{Appender, Config, Root};
use log4rs::encode::pattern::PatternEncoder;
use log::LevelFilter;
use tokio::fs;
use tokio::io::{Error, ErrorKind, Result};
use yaml_rust::YamlLoader;

use crate::commons::{option_convert, std_res_convert};

mod client;
mod server;
mod commons;
mod message;

const COMMAND_FAILED: &str = "Command failed";
const CONFIG_ERROR: &str = "Config error";

#[tokio::main]
async fn main() -> Result<()> {
  logger_init();

  if let Err(e) = process().await {
    error!("{}", e);
  };
  Ok(())
}

async fn process() -> Result<()> {
  let mut args = env::args();
  args.next();

  let mode = option_convert(args.next(), COMMAND_FAILED)?;

  let config_path = option_convert(args.next(), COMMAND_FAILED)?;

  let config = fs::read_to_string(config_path).await?;
  let config = std_res_convert(YamlLoader::load_from_str(&config), |e| e.to_string())?;
  let config = &config[0];

  match mode.as_str() {
    "client" => {
      let bind_addr = option_convert(config["host"].as_str(), CONFIG_ERROR)?;
      let host_list = option_convert(config["remote"].as_vec(), CONFIG_ERROR)?;

      client::start(bind_addr, host_list).await
    }
    "server" => {
      let host = option_convert(config["host"].as_str(), CONFIG_ERROR)?;
      let key = option_convert(config["key"].as_str(), CONFIG_ERROR)?;

      server::start(host, key).await
    }
    _ => Err(Error::new(ErrorKind::Other, COMMAND_FAILED))
  }
}

fn logger_init() {
  let stdout = ConsoleAppender::builder()
    .encoder(Box::new(PatternEncoder::new("[Console] {d} - {l} -{t} - {m}{n}")))
    .build();

  let config = Config::builder()
    .appender(Appender::builder().build("stdout", Box::new(stdout)))
    .build(Root::builder().appender("stdout").build(LevelFilter::Info))
    .unwrap();

  log4rs::init_config(config).unwrap();
}
