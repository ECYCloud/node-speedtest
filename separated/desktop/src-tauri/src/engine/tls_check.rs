//! 经 SOCKS5 出站对目标主机做严格 TLS（SNI + WebPKI）核实。
//! 语义对齐旧 C++ `verifyTlsForLatency`：Verified / Failed / NotApplicable。

use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, RootCertStore};
use tokio_rustls::TlsConnector;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsVerifyState {
    NotApplicable,
    Verified,
    Failed,
}

impl TlsVerifyState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotApplicable => "NotApplicable",
            Self::Verified => "Verified",
            Self::Failed => "Failed",
        }
    }
}

/// 经本地 SOCKS5 连接 host:443，完成证书链与主机名校验后立即断开。
pub async fn verify_tls_via_socks5(socks_port: u16, host: &str, port: u16) -> TlsVerifyState {
    match verify_inner(socks_port, host, port).await {
        Ok(true) => TlsVerifyState::Verified,
        Ok(false) => TlsVerifyState::Failed,
        Err(_) => TlsVerifyState::NotApplicable,
    }
}

async fn verify_inner(socks_port: u16, host: &str, port: u16) -> Result<bool, String> {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let cfg = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(cfg));

    let mut tcp = timeout(
        Duration::from_secs(5),
        TcpStream::connect(("127.0.0.1", socks_port)),
    )
    .await
    .map_err(|_| "connect timeout".to_string())?
    .map_err(|e| e.to_string())?;
    tcp.set_nodelay(true).ok();

    // SOCKS5 greeting
    tcp.write_all(&[0x05, 0x01, 0x00])
        .await
        .map_err(|e| e.to_string())?;
    let mut g = [0u8; 2];
    timeout(Duration::from_secs(3), tcp.read_exact(&mut g))
        .await
        .map_err(|_| "greet timeout".to_string())?
        .map_err(|e| e.to_string())?;
    if g != [0x05, 0x00] {
        return Err("socks auth".into());
    }

    // CONNECT host:port
    let host_b = host.as_bytes();
    if host_b.len() > 255 {
        return Err("host too long".into());
    }
    let mut req = Vec::with_capacity(7 + host_b.len());
    req.extend_from_slice(&[0x05, 0x01, 0x00, 0x03, host_b.len() as u8]);
    req.extend_from_slice(host_b);
    req.extend_from_slice(&port.to_be_bytes());
    tcp.write_all(&req).await.map_err(|e| e.to_string())?;

    let mut rh = [0u8; 4];
    timeout(Duration::from_secs(8), tcp.read_exact(&mut rh))
        .await
        .map_err(|_| "connect reply timeout".to_string())?
        .map_err(|e| e.to_string())?;
    if rh[1] != 0x00 {
        return Err(format!("socks connect failed {}", rh[1]));
    }
    match rh[3] {
        0x01 => {
            let mut s = [0u8; 6];
            tcp.read_exact(&mut s).await.map_err(|e| e.to_string())?;
        }
        0x03 => {
            let mut n = [0u8; 1];
            tcp.read_exact(&mut n).await.map_err(|e| e.to_string())?;
            let mut s = vec![0u8; n[0] as usize + 2];
            tcp.read_exact(&mut s).await.map_err(|e| e.to_string())?;
        }
        0x04 => {
            let mut s = [0u8; 18];
            tcp.read_exact(&mut s).await.map_err(|e| e.to_string())?;
        }
        _ => return Err("bad atyp".into()),
    }

    let server_name = ServerName::try_from(host.to_string()).map_err(|e| e.to_string())?;
    let tls = timeout(
        Duration::from_secs(10),
        connector.connect(server_name, tcp),
    )
    .await
    .map_err(|_| "tls handshake timeout".to_string())?;

    match tls {
        Ok(mut stream) => {
            // 握手成功且 rustls 已完成证书校验
            let _ = stream.shutdown().await;
            Ok(true)
        }
        Err(e) => {
            let msg = e.to_string().to_lowercase();
            // SOCKS CONNECT 已成功：能判定为证书问题的一律 Failed，避免误标 NotApplicable
            if msg.contains("certificate")
                || msg.contains("cert")
                || msg.contains("webpki")
                || msg.contains("unknown issuer")
                || msg.contains("unknownissuer")
                || msg.contains("not valid")
                || msg.contains("notvalidforname")
                || msg.contains("expired")
                || msg.contains("hostname")
                || msg.contains("name mismatch")
                || msg.contains("badencoding")
                || msg.contains("unsupportedcert")
                || msg.contains("invalid peer")
            {
                Ok(false)
            } else {
                // 对端断开 / 协议中断等 → 无法下证书结论
                Err(e.to_string())
            }
        }
    }
}

/// 测速链路常用核实目标（与延迟探测同域）。
pub const TLS_VERIFY_HOST: &str = "cp.cloudflare.com";
pub const TLS_VERIFY_PORT: u16 = 443;
