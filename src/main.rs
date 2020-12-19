#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
#[macro_use]
extern crate nanoid;

use std::env;
use std::net::SocketAddr;
use std::os::raw::c_char;
use std::str::FromStr;

use log4rs::append::console::ConsoleAppender;
use log4rs::config::{Appender, Config, Root};
use log4rs::encode::pattern::PatternEncoder;
use log::LevelFilter;
use tokio::fs;
use tokio::io::{Error, ErrorKind, Result};
use yaml_rust::YamlLoader;

use crate::commons::{OptionConvert, StdResAutoConvert};

mod client;
mod server;
mod commons;

pub const COMMAND_FAILED: &str = "Command failed";
pub const CONFIG_ERROR: &str = "Config error";

#[tokio::main]
async fn main() -> Result<()> {
  logger_init()?;

  if let Err(e) = process().await {
    error!("{}", e);
  };
  Ok(())
}

async fn process() -> Result<()> {
  let mut args = env::args();
  args.next();

  let mode = args.next().option_to_res(COMMAND_FAILED)?;
  let config_path = args.next().option_to_res(COMMAND_FAILED)?;

  let config = fs::read_to_string(config_path).await?;
  let config = YamlLoader::load_from_str(&config).res_auto_convert()?;
  let config = &config[0];

  match mode.as_str() {
    "client" => {
      let socks5_listen = config["socks5Listen"].as_str().option_to_res(CONFIG_ERROR)?.to_string();
      let http_listen = config["httpListen"].as_str().option_to_res(CONFIG_ERROR)?.to_string();
      let remote_hosts = config["remote"].as_vec().option_to_res(CONFIG_ERROR)?;

      let temp = SocketAddr::from_str(&socks5_listen).res_auto_convert()?;
      let local_socks5_addr = format!("127.0.0.1:{}", temp.port());

      let f1 = tokio::task::spawn_blocking(move || {
        if let Err(e) = start_http_proxy_server(&http_listen, &local_socks5_addr) {
          error!("{}", e)
        }
      });

      let f2 = client::start(&socks5_listen, remote_hosts);

      tokio::select! {
        res = f1 => res.res_auto_convert(),
        res = f2 => res,
      }
    }
    "server" => {
      server::start(config).await
    }
    _ => Err(Error::new(ErrorKind::Other, COMMAND_FAILED))
  }
}

fn logger_init() -> Result<()> {
  let stdout = ConsoleAppender::builder()
    .encoder(Box::new(PatternEncoder::new("[Console] {d(%Y-%m-%d %H:%M:%S)} - {l} - {m}{n}")))
    .build();

  let config = Config::builder()
    .appender(Appender::builder().build("stdout", Box::new(stdout)))
    .build(Root::builder().appender("stdout").build(LevelFilter::Info))
    .res_auto_convert()?;

  log4rs::init_config(config).res_auto_convert()?;
  Ok(())
}

fn start_http_proxy_server(bind_addr: &str, socks5_addr: &str) -> Result<()> {
  let lib = libloading::Library::new("./httptosocks").res_auto_convert()?;

  unsafe {
    let start: libloading::Symbol<unsafe extern fn(*const c_char, u8, *const c_char, u8, u8) -> ()> = lib.get(b"start").res_auto_convert()?;

    start(bind_addr.as_ptr() as *const c_char,
          bind_addr.len() as u8,
          socks5_addr.as_ptr() as *const c_char,
          socks5_addr.len() as u8,
          1);
  };
  Ok(())
}

