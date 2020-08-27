use async_trait::async_trait;
use bytes::{BufMut, BytesMut};
use nanoid;
use tokio::io::{AsyncWriteExt, Result};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

use crate::message;
use crate::message::Msg;

pub type Address = (String, u16);

pub fn create_channel_id() -> String {
  nanoid!(4)
}

#[async_trait]
pub trait MsgWriteHandler {
  async fn write_msg(&mut self, msg: &Msg) -> Result<()>;
}

#[async_trait]
impl MsgWriteHandler for OwnedWriteHalf {
  async fn write_msg(&mut self, msg: &Msg) -> Result<()> {
    let msg = message::encode(msg);

    let mut data = BytesMut::new();
    data.put_u32(msg.len() as u32);
    data.put_slice(&msg);

    self.write_all(&data).await
  }
}

#[async_trait]
pub trait MsgReadHandler {
  async fn read_msg(&mut self) -> Result<Msg>;
}

#[async_trait]
impl MsgReadHandler for OwnedReadHalf {
  async fn read_msg(&mut self) -> Result<Msg> {
    let data = message::read_msg(self).await?;
    message::decode(data)
  }
}
