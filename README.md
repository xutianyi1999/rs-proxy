# rs-proxy
socks5代理

## Config
### tcp-server
```yaml
---
# 本机地址
host: 0.0.0.0:12345
# 协议
protocol: tcp
# 密钥
key: "123a"
# 缓冲队列容量
buff_size: 3000
```
### quic-server
```yaml
---
# 本机地址
host: 0.0.0.0:12346
# 协议
protocol: quic
# 证书路径
cert: ./key/cert.der
# 私钥路径
private_key: ./key/priv_key
```
### client
```yaml
---
# 本地socks5绑定地址
host: 0.0.0.0:12333
# 服务端地址 (多个)
remote:
    # 名称
  - name: local-tcp
    # 协议
    protocol: tcp
    # 连接数
    connections: 1
    # ip 地址
    host: 127.0.0.1:12345
    # 密钥
    key: "123a"
    # 缓冲队列容量
    buff_size: 3000

    # 名称
  - name: local-quic
    # 协议
    protocol: quic
    # ip 地址
    host: 127.0.0.1:12346
    # 证书路径
    cert: ./key/cert.der
    # subjectAltName
    server_name: local
```
## Usage
### server
```shell script
./rs-proxy server server-config.yaml
```
### client
```shell script
./rs-proxy client client-config.yaml
```

## Build project
```shell script
cargo build --release
```


