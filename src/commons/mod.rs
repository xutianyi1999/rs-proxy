use crypto::buffer::{RefReadBuffer, RefWriteBuffer};
use crypto::rc4::Rc4;
use crypto::symmetriccipher::Encryptor;
use socket2::{Socket, TcpKeepalive};
use tokio::io::{Error, ErrorKind, Result};
use tokio::net::{TcpSocket, TcpStream};
use tokio::time::Duration;

pub mod tcpmux_comm;
pub mod tcp_comm;

pub type Address = (Vec<u8>, u16);

pub const MAGIC_CODE: u32 = 0xA5C878F0;

pub trait OptionConvert<T> {
  fn option_to_res(self, msg: &str) -> Result<T>;
}

impl<T> OptionConvert<T> for Option<T> {
  fn option_to_res(self, msg: &str) -> Result<T> {
    option_convert(self, msg)
  }
}

pub trait StdResConvert<T, E> {
  fn res_convert(self, f: fn(E) -> String) -> Result<T>;
}

impl<T, E> StdResConvert<T, E> for std::result::Result<T, E> {
  fn res_convert(self, f: fn(E) -> String) -> Result<T> {
    std_res_convert(self, f)
  }
}

pub trait StdResAutoConvert<T, E: ToString> {
  fn res_auto_convert(self) -> Result<T>;
}

impl<T, E: ToString> StdResAutoConvert<T, E> for std::result::Result<T, E> {
  fn res_auto_convert(self) -> Result<T> {
    std_res_convert(self, |e| e.to_string())
  }
}

fn option_convert<T>(o: Option<T>, msg: &str) -> Result<T> {
  match o {
    Some(v) => Ok(v),
    None => Err(Error::new(ErrorKind::Other, msg))
  }
}

fn std_res_convert<T, E>(res: std::result::Result<T, E>, f: fn(E) -> String) -> Result<T> {
  match res {
    Ok(v) => Ok(v),
    Err(e) => {
      let msg = f(e);
      Err(Error::new(ErrorKind::Other, msg))
    }
  }
}

pub fn crypto<'a>(input: &'a [u8], output: &'a mut [u8], rc4: &'a mut Rc4) -> Result<&'a mut [u8]> {
  let mut ref_read_buf = RefReadBuffer::new(input);
  let mut ref_write_buf = RefWriteBuffer::new(output);

  rc4.encrypt(&mut ref_read_buf, &mut ref_write_buf, false)
    .res_convert(|_| "Crypto error".to_string())?;
  Ok(&mut output[..input.len()])
}

pub trait TcpSocketExt {
  fn set_keepalive(&self) -> tokio::io::Result<()>;
}

impl TcpSocketExt for TcpStream {
  fn set_keepalive(&self) -> tokio::io::Result<()> {
    set_keepalive(self)
  }
}

impl TcpSocketExt for TcpSocket {
  fn set_keepalive(&self) -> tokio::io::Result<()> {
    set_keepalive(self)
  }
}

const TCP_KEEPALIVE: TcpKeepalive = TcpKeepalive::new().with_time(Duration::from_secs(120));

#[cfg(target_os = "windows")]
fn set_keepalive<S: std::os::windows::io::AsRawSocket>(socket: &S) -> tokio::io::Result<()> {
  use std::os::windows::io::FromRawSocket;

  unsafe {
    let socket = Socket::from_raw_socket(socket.as_raw_socket());
    socket.set_tcp_keepalive(&TCP_KEEPALIVE)?;
    std::mem::forget(socket);
  };
  Ok(())
}

#[cfg(target_os = "linux")]
fn set_keepalive<S: std::os::unix::io::AsRawFd>(socket: &S) -> tokio::io::Result<()> {
  use std::os::unix::io::FromRawFd;

  unsafe {
    let socket = Socket::from_raw_fd(socket.as_raw_fd());
    socket.set_tcp_keepalive(&TCP_KEEPALIVE)?;
    std::mem::forget(socket);
  };
  Ok(())
}
