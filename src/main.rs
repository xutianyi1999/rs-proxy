#[macro_use]
extern crate nanoid;

use std::sync::Arc;

use dashmap::DashMap;
use tokio::io::Result;
use tokio::net::TcpStream;
use tokio::prelude::*;
use tokio::sync::Mutex;

mod client;
mod commons;
mod message;

#[tokio::main]
async fn main() -> Result<()> {
  Ok(())
}


