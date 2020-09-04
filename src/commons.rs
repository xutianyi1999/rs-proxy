use async_trait::async_trait;
use bytes::{BufMut, Bytes, BytesMut};
use crypto::buffer::{RefReadBuffer, RefWriteBuffer};
use crypto::rc4::Rc4;
use crypto::symmetriccipher::{Decryptor, Encryptor};
use nanoid;
use tokio::io::{AsyncWriteExt, Error, ErrorKind, Result};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

use crate::message;
use crate::message::Msg;

pub type Address = (String, u16);

pub fn create_channel_id() -> String {
  nanoid!(4)
}

#[async_trait]
pub trait MsgWriteHandler {
  async fn write_msg(&mut self, msg: &Msg, rc4: &mut Rc4) -> Result<()>;
}

#[async_trait]
impl MsgWriteHandler for OwnedWriteHalf {
  async fn write_msg(&mut self, msg: &Msg, rc4: &mut Rc4) -> Result<()> {
    let msg = message::encode(msg);
    let msg = crypto(&msg, rc4, MODE::ENCRYPT)?;

    let mut data = BytesMut::new();
    data.put_u16(msg.len() as u16);
    data.put_slice(&msg);

    self.write_all(&data).await
  }
}

#[async_trait]
pub trait MsgReadHandler {
  async fn read_msg(&mut self, rc4: &mut Rc4) -> Result<Msg>;
}

#[async_trait]
impl MsgReadHandler for OwnedReadHalf {
  async fn read_msg(&mut self, rc4: &mut Rc4) -> Result<Msg> {
    let data = message::read_msg(self).await?;
    let data = crypto(&data, rc4, MODE::DECRYPT)?;
    message::decode(Bytes::from(data))
  }
}

enum MODE {
  ENCRYPT,
  DECRYPT,
}

fn crypto<'a>(input: &'a [u8], rc4: &'a mut Rc4, mode: MODE) -> Result<Vec<u8>> {
  let mut ref_read_buf = RefReadBuffer::new(input);
  let mut out = vec![0u8; input.len()];
  let mut ref_write_buf = RefWriteBuffer::new(&mut out);

  let _ = match mode {
    MODE::DECRYPT => {
      if let Err(_) = rc4.decrypt(&mut ref_read_buf, &mut ref_write_buf, false) {
        return Err(Error::new(ErrorKind::Other, "Decrypt error"));
      }
    }
    MODE::ENCRYPT => {
      if let Err(_) = rc4.encrypt(&mut ref_read_buf, &mut ref_write_buf, false) {
        return Err(Error::new(ErrorKind::Other, "Encrypt error"));
      }
    }
  };
  Ok(out)
}

pub trait OptionConvert<T> {
  fn option_to_res(self, msg: &str) -> Result<T>;
}

impl<T> OptionConvert<T> for Option<T> {
  fn option_to_res(self, msg: &str) -> Result<T> {
    option_convert(self, msg)
  }
}

pub trait StdResConvert<T, E> {
  fn std_res_convert(self, f: fn(E) -> String) -> Result<T>;
}

impl<T, E> StdResConvert<T, E> for std::result::Result<T, E> {
  fn std_res_convert(self, f: fn(E) -> String) -> Result<T> {
    std_res_convert(self, f)
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
