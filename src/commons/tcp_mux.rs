use bytes::BufMut;
use crypto::buffer::{RefReadBuffer, RefWriteBuffer};
use crypto::rc4::Rc4;
use crypto::symmetriccipher::{Decryptor, Encryptor};
use socket2::Socket;
use tokio::io::{AsyncBufRead, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ErrorKind, Result};
use tokio::io::Error;
use tokio::net::{TcpSocket, TcpStream};
use tokio::time::Duration;

use crate::commons::{Address, StdResConvert};
use crate::commons::tcp_mux::MODE::Decrypt;

const CONNECT: u8 = 0x00;
const DISCONNECT: u8 = 0x01;
const DATA: u8 = 0x03;

pub type ChannelId = u32;

pub enum Msg<'a> {
  Connect(ChannelId, Address),
  Disconnect(ChannelId),
  Data(ChannelId, Vec<u8>),
  RefData(ChannelId, &'a [u8]),
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

    let data = crypto(data, &mut self.out, &mut self.rc4, Decrypt)?;
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
    DATA => Msg::RefData(channel_id, &data[(1 + 4)..]),
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
    let slice = crypto(&msg, &mut self.buff, &mut self.rc4, MODE::Encrypt)?;
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
    _ => panic!("Msg encode error")
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

enum MODE {
  Encrypt,
  Decrypt,
}

fn crypto<'a>(input: &'a [u8], output: &'a mut [u8], rc4: &'a mut Rc4, mode: MODE) -> Result<&'a mut [u8]> {
  let mut ref_read_buf = RefReadBuffer::new(input);
  let mut ref_write_buf = RefWriteBuffer::new(output);

  match mode {
    MODE::Decrypt => {
      rc4.decrypt(&mut ref_read_buf, &mut ref_write_buf, false)
        .res_convert(|_| "Decrypt error".to_string())?;
    }
    MODE::Encrypt => {
      rc4.encrypt(&mut ref_read_buf, &mut ref_write_buf, false)
        .res_convert(|_| "Encrypt error".to_string())?;
    }
  };
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

const KEEPALIVE_DURATION: Option<Duration> = Option::Some(Duration::from_secs(120));

#[cfg(target_os = "windows")]
fn set_keepalive<S: std::os::windows::io::AsRawSocket>(socket: &S) -> tokio::io::Result<()> {
  use std::os::windows::io::FromRawSocket;

  unsafe {
    let socket = Socket::from_raw_socket(socket.as_raw_socket());
    socket.set_keepalive(KEEPALIVE_DURATION)?;
    std::mem::forget(socket);
  };
  Ok(())
}

#[cfg(target_os = "linux")]
fn set_keepalive<S: std::os::unix::io::AsRawFd>(socket: &S) -> tokio::io::Result<()> {
  use std::os::unix::io::FromRawFd;

  unsafe {
    let socket = Socket::from_raw_fd(socket.as_raw_fd());
    socket.set_keepalive(KEEPALIVE_DURATION)?;
    std::mem::forget(socket);
  };
  Ok(())
}

