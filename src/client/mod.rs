use std::net::SocketAddr;
use std::os::raw::c_char;
use std::str::FromStr;
use std::sync::Arc;

use tokio::io::{Error, ErrorKind, Result};
use tokio::net::{TcpListener, TcpStream};
use yaml_rust::Yaml;

use crate::client::tcp_client::TcpHandle;
use crate::client::tcpmux_client::TcpMuxHandle;
use crate::commons::{Address, OptionConvert, StdResAutoConvert};
use crate::CONFIG_ERROR;

mod tcpmux_client;
mod tcp_client;
mod socks5;

enum Protocol {
  Tcp(TcpHandle),
  TcpMux(TcpMuxHandle),
}

pub async fn start(config: &Yaml) -> Result<()> {
  let socks5_listen = config["socks5Listen"].as_str().option_to_res(CONFIG_ERROR)?;
  let http_listen = config["httpListen"].as_str().option_to_res(CONFIG_ERROR)?.to_owned();
  let protocol = config["protocol"].as_str().option_to_res(CONFIG_ERROR)?;

  let remote_hosts = config["remote"].as_vec().option_to_res(CONFIG_ERROR)?;

  let proto = match protocol {
    "tcp" => {
      let buff_size = config["buffSize"].as_i64().option_to_res(CONFIG_ERROR)?;
      Protocol::Tcp(TcpHandle::new(remote_hosts, buff_size as usize).await?)
    }
    "tcpmux" => {
      let buff_size = config["buffSize"].as_i64().option_to_res(CONFIG_ERROR)?;
      let channel_capacity = config["channelCapacity"].as_i64().option_to_res(CONFIG_ERROR)?;
      Protocol::TcpMux(TcpMuxHandle::new(remote_hosts, buff_size as usize, channel_capacity as usize)?)
    }
    _ => return Err(Error::new(ErrorKind::Other, CONFIG_ERROR))
  };


  let socks5_listen_addr = SocketAddr::from_str(socks5_listen).res_auto_convert()?;

  let f1 = tokio::task::spawn_blocking(move || {
    let local_socks5_addr = format!("127.0.0.1:{}", socks5_listen_addr.port());
    let res = start_http_proxy_server(&http_listen, &local_socks5_addr);

    if let Err(e) = res {
      error!("{}", e)
    }
  });

  let f2 = socks5_server_bind(socks5_listen_addr, proto);

  tokio::select! {
    res = f1 => res.res_auto_convert(),
    res = f2 => res,
  }
}

async fn socks5_server_bind(host: SocketAddr, proto: Protocol) -> Result<()> {
  let tcp_listener = TcpListener::bind(host).await?;
  info!("Listening on socks5://{}", tcp_listener.local_addr()?);
  let proto = Arc::new(proto);

  while let Ok((mut socket, _)) = tcp_listener.accept().await {
    let inner_proto = proto.clone();

    tokio::spawn(async move {
      let res = async move {
        let addr = socks5_decode(&mut socket).await?;

        match &*inner_proto {
          Protocol::Tcp(handle) => handle.proxy(socket, addr).await,
          Protocol::TcpMux(handle) => handle.proxy(socket, addr).await
        }
      };

      if let Err(e) = res.await {
        error!("{}", e)
      }
    });
  };
  Ok(())
}

async fn socks5_decode(socket: &mut TcpStream) -> Result<Address> {
  socks5::initial_request(socket).await?;
  let addr = socks5::command_request(socket).await?;
  Ok(addr)
}

fn start_http_proxy_server(bind_addr: &str, socks5_addr: &str) -> Result<()> {
  #[cfg(target_os = "windows")]
    let lib_name = "httptosocks.dll";
  #[cfg(target_os = "linux")]
    let lib_name = "./libhttptosocks.so";

  unsafe {
    let lib = libloading::Library::new(lib_name).res_auto_convert()?;

    let start: libloading::Symbol<unsafe extern fn(*const c_char, *const c_char) -> ()> = lib.get(b"start").res_auto_convert()?;

    start((bind_addr.to_owned() + "\0").as_ptr() as *const c_char,
          (socks5_addr.to_owned() + "\0").as_ptr() as *const c_char);
  };
  Ok(())
}
