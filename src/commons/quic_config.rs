use std::net::SocketAddr;

use quinn::{Certificate, CertificateChain, ClientConfig, ClientConfigBuilder, Endpoint, PrivateKey, ServerConfig, ServerConfigBuilder};
use tokio::fs;
use tokio::io::Result;

use crate::commons::{StdResAutoConvert, StdResConvert};

pub async fn configure_client(cert_paths: Vec<String>) -> Result<ClientConfig> {
  let mut cfg_builder = ClientConfigBuilder::default();

  for path in cert_paths {
    let v = fs::read(path).await?;
    let certificate = Certificate::from_der(&v).unwrap();
    cfg_builder.add_certificate_authority(certificate);
  };

  cfg_builder.enable_0rtt();
  Ok(cfg_builder.build())
}

pub async fn configure_server(cert_path: &str, priv_key_path: &str) -> Result<ServerConfig> {
  let cert_future = fs::read(cert_path);
  let priv_key_future = fs::read(priv_key_path);

  let (cert, priv_key) = tokio::try_join!(cert_future, priv_key_future)?;

  let cert = Certificate::from_der(&cert).res_auto_convert()?;
  let priv_key = PrivateKey::from_der(&priv_key).res_auto_convert()?;

  let mut cfg_builder = ServerConfigBuilder::default();
  cfg_builder.certificate(CertificateChain::from_certs(vec![cert]), priv_key);

  Ok(cfg_builder.build())
}
