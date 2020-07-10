#[macro_use]
extern crate anyhow;
extern crate nanoid;

use std::any::Any;
use std::env;
use std::net::SocketAddr;

use anyhow::Result;
use nanoid::nanoid;
use tokio::net::{TcpListener, TcpStream};

mod server;
mod client;
mod message;
mod commons;

fn main() {
  let id = nanoid!(4);

  println!("{:?}", id)
}

