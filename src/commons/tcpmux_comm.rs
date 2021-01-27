use bytes::BufMut;
use crypto::rc4::Rc4;
use tokio::io::{AsyncBufRead, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ErrorKind, Result};
use tokio::io::Error;

use crate::commons::Address;
use crate::commons::crypto;

const CONNECT: u8 = 0x00;
const DISCONNECT: u8 = 0x01;
const DATA: u8 = 0x03;

pub type ChannelId = u32;

pub enum Msg<'a> {
  Connect(ChannelId, Address),
  Disconnect(ChannelId),
  Data(ChannelId, &'a [u8]),
}

impl Msg<'_> {
  pub fn encode(self) -> Vec<u8> {
    encode(self)
  }
}

pub struct MsgReader<R>
  where R: AsyncBufRead
{
  reader: R,
  rc4: Rc4,
  out: Vec<u8>,
  buff: Vec<u8>,
}

impl<R> MsgReader<R>
  where R: AsyncBufRead + Unpin
{
  pub fn new(reader: R, rc4: Rc4) -> MsgReader<R> {
    let size = 65535;
    MsgReader { reader, rc4, out: vec![0u8; size], buff: vec![0u8; size] }
  }

  pub async fn read_msg(&mut self) -> Result<Option<Msg<'_>>> {
    let op = read_msg(&mut self.reader, &mut self.buff).await?;

    let data = match op {
      Some(v) => v,
      None => return Ok(None)
    };

    let data = crypto(data, &mut self.out, &mut self.rc4)?;
    let msg = decode(data)?;
    Ok(Some(msg))
  }
}

async fn read_msg<'a, A>(rx: &mut A, buff: &'a mut [u8]) -> Result<Option<&'a [u8]>>
  where A: AsyncRead + Unpin
{
  let len = match rx.read_u16().await {
    Ok(len) => len,
    Err(_) => return Ok(None)
  };

  let msg = &mut buff[..len as usize];
  rx.read_exact(msg).await?;
  Ok(Some(msg))
}

fn decode(data: &mut [u8]) -> Result<Msg> {
  let len = data.len();
  let mode = data[0];

  let mut t = [0u8; 4];
  t.copy_from_slice(&data[1..5]);
  let channel_id = u32::from_be_bytes(t);

  let msg = match mode {
    CONNECT => {
      let mut host = vec![0u8; len - 1 - 4 - 2];
      host.copy_from_slice(&data[5..(len - 2)]);

      let mut port = [0u8; 2];
      port.copy_from_slice(&data[(len - 2)..]);

      Msg::Connect(channel_id, (host, u16::from_be_bytes(port)))
    }
    DISCONNECT => Msg::Disconnect(channel_id),
    DATA => Msg::Data(channel_id, &data[(1 + 4)..]),
    _ => return Err(Error::new(ErrorKind::Other, "Message error"))
  };
  Ok(msg)
}

pub struct MsgWriter<W>
  where W: AsyncWrite
{
  writer: W,
  rc4: Rc4,
  out: Vec<u8>,
  buff: Vec<u8>,
}

impl<W> MsgWriter<W>
  where W: AsyncWrite + Unpin
{
  pub fn new(writer: W, rc4: Rc4) -> MsgWriter<W> {
    MsgWriter { writer, rc4, out: vec![0u8; 65537], buff: vec![0u8; 65535] }
  }

  pub async fn write_msg(&mut self, msg: &[u8]) -> Result<()> {
    let slice = crypto(&msg, &mut self.buff, &mut self.rc4)?;
    let msg_len = slice.len();

    let out = &mut self.out;
    let len = (msg_len as u16).to_be_bytes();
    out[..2].copy_from_slice(&len);
    out[2..(msg_len + 2)].copy_from_slice(slice);

    self.writer.write_all(&out[..(msg_len + 2)]).await
  }
}

fn encode(msg: Msg) -> Vec<u8> {
  match msg {
    Msg::Connect(id, addr) => encode_connect_msg(addr, id),
    Msg::Disconnect(id) => encode_disconnect_msg(id),
    Msg::Data(id, data) => encode_data_msg(id, &data),
  }
}

fn encode_connect_msg(addr: Address, channel_id: ChannelId) -> Vec<u8> {
  let (host, port) = addr;
  let mut buff = Vec::with_capacity(1 + 4 + host.len() + 2);

  buff.put_u8(CONNECT);
  buff.put_u32(channel_id);

  buff.put_slice(&host);
  buff.put_u16(port);
  buff
}

fn encode_disconnect_msg(channel_id: ChannelId) -> Vec<u8> {
  let mut buff = Vec::with_capacity(1 + 4);
  buff.put_u8(DISCONNECT);
  buff.put_u32(channel_id);
  buff
}

fn encode_data_msg(channel_id: ChannelId, data: &[u8]) -> Vec<u8> {
  let mut buff = Vec::with_capacity(1 + 4 + data.len());
  buff.put_u8(DATA);
  buff.put_u32(channel_id);
  buff.put_slice(data);
  buff
}
