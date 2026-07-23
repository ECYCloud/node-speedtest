//! 经本地 SOCKS5（mihomo）做 RFC 3489 Classic STUN NAT / UDP 探测。
//! 协议与候选 STUN 列表对齐旧 `src/ntt.cpp`，结果枚举供结果表 / PNG 使用。

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::time::timeout;

const MAGIC: [u8; 4] = [0x21, 0x12, 0xa4, 0x42];
const BIND_REQUEST: [u8; 2] = [0x00, 0x01];
const BIND_RESPONSE: [u8; 2] = [0x01, 0x01];
const ATTR_MAPPED: [u8; 2] = [0x00, 0x01];
const ATTR_CHANGED: [u8; 2] = [0x00, 0x05];
const ATTR_XOR_MAPPED: [u8; 2] = [0x00, 0x20];
const CHANGE_IP_PORT: [u8; 8] = [0x00, 0x03, 0x00, 0x04, 0x00, 0x00, 0x00, 0x06];
const CHANGE_PORT: [u8; 8] = [0x00, 0x03, 0x00, 0x04, 0x00, 0x00, 0x00, 0x02];

#[derive(Clone, Debug)]
struct StunResp {
    failed: bool,
    ext_ip: Ipv4Addr,
    ext_port: u16,
    change_ip: Ipv4Addr,
    change_port: u16,
}

impl Default for StunResp {
    fn default() -> Self {
        Self {
            failed: true,
            ext_ip: Ipv4Addr::UNSPECIFIED,
            ext_port: 0,
            change_ip: Ipv4Addr::UNSPECIFIED,
            change_port: 0,
        }
    }
}

/// 返回 NAT_TYPE_STR 兼容字符串。
pub async fn detect_nat_thru_socks5(socks_port: u16) -> String {
    match detect_inner(socks_port).await {
        Ok(s) => s,
        Err(_) => "Unknown".into(),
    }
}

async fn detect_inner(socks_port: u16) -> Result<String, String> {
    let socks = SocketAddr::from((Ipv4Addr::LOCALHOST, socks_port));
    let mut tcp = timeout(Duration::from_secs(3), TcpStream::connect(socks))
        .await
        .map_err(|_| "socks tcp timeout".to_string())?
        .map_err(|e| e.to_string())?;
    tcp.set_nodelay(true).ok();

    // SOCKS5 no-auth greeting
    tcp.write_all(&[0x05, 0x01, 0x00]).await.map_err(|e| e.to_string())?;
    let mut g = [0u8; 2];
    timeout(Duration::from_secs(2), tcp.read_exact(&mut g))
        .await
        .map_err(|_| "socks greet timeout".to_string())?
        .map_err(|e| e.to_string())?;
    if g[0] != 0x05 || g[1] != 0x00 {
        return Err("socks auth rejected".into());
    }

    let udp = UdpSocket::bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)))
        .await
        .map_err(|e| e.to_string())?;
    let local = udp.local_addr().map_err(|e| e.to_string())?;
    let local_port = local.port();

    // UDP ASSOCIATE：告诉代理本机 UDP 端口（ATYP=IPv4 0.0.0.0:local_port）
    let mut assoc = vec![0x05, 0x03, 0x00, 0x01, 0, 0, 0, 0];
    assoc.extend_from_slice(&local_port.to_be_bytes());
    tcp.write_all(&assoc).await.map_err(|e| e.to_string())?;

    let mut hdr = [0u8; 4];
    timeout(Duration::from_secs(3), tcp.read_exact(&mut hdr))
        .await
        .map_err(|_| "udp associate timeout".to_string())?
        .map_err(|e| e.to_string())?;
    if hdr[1] != 0x00 {
        // 代理不支持 UDP 中继
        return Ok("Unknown".into());
    }
    let atyp = hdr[3];
    let relay_host = match atyp {
        0x01 => {
            let mut ip = [0u8; 4];
            tcp.read_exact(&mut ip).await.map_err(|e| e.to_string())?;
            let v4 = Ipv4Addr::from(ip);
            // 多数实现回报 0.0.0.0，实际应打到本机 SOCKS 端口
            if v4.is_unspecified() {
                "127.0.0.1".into()
            } else {
                v4.to_string()
            }
        }
        0x03 => {
            let mut n = [0u8; 1];
            tcp.read_exact(&mut n).await.map_err(|e| e.to_string())?;
            let mut name = vec![0u8; n[0] as usize];
            tcp.read_exact(&mut name).await.map_err(|e| e.to_string())?;
            String::from_utf8_lossy(&name).into_owned()
        }
        0x04 => {
            let mut skip = [0u8; 16];
            tcp.read_exact(&mut skip).await.map_err(|e| e.to_string())?;
            "127.0.0.1".into()
        }
        _ => return Err("bad atyp".into()),
    };
    let mut pb = [0u8; 2];
    tcp.read_exact(&mut pb).await.map_err(|e| e.to_string())?;
    let relay_port = u16::from_be_bytes(pb);
    let relay = resolve_v4(&relay_host, relay_port).await?;

    // TCP 关联必须保持，直到 UDP 探测结束
    let _tcp_keep = tcp;

    // 与 C++ ntt.cpp 对齐：多候选兜底，避免首选 STUN 不可达时误判 Blocked
    let cands: &[(&str, u16)] = &[
        ("stun.l.google.com", 19302),
        ("stun1.l.google.com", 19302),
        ("stun.cloudflare.com", 3478),
        ("stun.voipgate.com", 3478),
        ("stun.miwifi.com", 3478),
    ];

    // 优先选用带回 CHANGED_ADDRESS 的 STUN：Test 2 失败后还要靠它分型。
    // 仅 MAPPED、无 CHANGED 的先记下作兜底（仍可走 Test 2 → FullCone）。
    let mut picked_with_change: Option<(String, u16, StunResp)> = None;
    let mut picked_any: Option<(String, u16, StunResp)> = None;
    for &(host, port) in cands {
        let resp = stun_exchange(&udp, relay, host, port, &[]).await;
        if resp.failed {
            continue;
        }
        let has_change = !resp.change_ip.is_unspecified() && resp.change_port != 0;
        if has_change {
            picked_with_change = Some((host.to_string(), port, resp));
            break;
        }
        if picked_any.is_none() {
            picked_any = Some((host.to_string(), port, resp));
        }
    }
    let Some((host, port, r1)) = picked_with_change.or(picked_any) else {
        return Ok("Blocked".into());
    };

    let r2 = stun_exchange(&udp, relay, &host, port, &CHANGE_IP_PORT).await;
    if !r2.failed {
        return Ok("FullCone".into());
    }

    if r1.change_ip.is_unspecified() || r1.change_port == 0 {
        // UDP 已通（Test 1 成功），只是分不出锥型——不是 Blocked
        return Ok("Unknown".into());
    }
    let change_host = r1.change_ip.to_string();
    let r3 = stun_exchange(&udp, relay, &change_host, r1.change_port, &[]).await;
    if r3.failed {
        return Ok("Unknown".into());
    }
    if r3.ext_ip != r1.ext_ip || r3.ext_port != r1.ext_port {
        return Ok("Symmetric".into());
    }

    let r4 = stun_exchange(&udp, relay, &change_host, r1.change_port, &CHANGE_PORT).await;
    if r4.failed {
        return Ok("PortRestrictedCone".into());
    }
    Ok("RestrictedCone".into())
}

async fn resolve_v4(host: &str, port: u16) -> Result<SocketAddr, String> {
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        return Ok(SocketAddr::from((ip, port)));
    }
    let mut addrs = timeout(Duration::from_secs(2), tokio::net::lookup_host((host, port)))
        .await
        .map_err(|_| format!("dns timeout for {host}"))?
        .map_err(|e| e.to_string())?;
    addrs
        .find(|a| a.is_ipv4())
        .ok_or_else(|| format!("no v4 for {host}"))
}

async fn stun_exchange(
    udp: &UdpSocket,
    relay: SocketAddr,
    target_host: &str,
    target_port: u16,
    extra_attr: &[u8],
) -> StunResp {
    let mut last = StunResp::default();
    // 3×1.4s：兼顾丢包重试；外层 detect_nat 仍有 10s 总封顶防拖死。
    for _ in 0..3 {
        let tid = make_tid();
        let mut msg = Vec::with_capacity(20 + extra_attr.len());
        msg.extend_from_slice(&BIND_REQUEST);
        msg.extend_from_slice(&(extra_attr.len() as u16).to_be_bytes());
        msg.extend_from_slice(&tid);
        msg.extend_from_slice(extra_attr);

        let packet = wrap_socks_udp(target_host, target_port, &msg);
        if udp.send_to(&packet, relay).await.is_err() {
            continue;
        }
        let mut buf = [0u8; 2048];
        let Ok(Ok((n, _))) = timeout(Duration::from_millis(1400), udp.recv_from(&mut buf)).await
        else {
            continue;
        };
        if let Some(payload) = unwrap_socks_udp(&buf[..n]) {
            if let Some(r) = parse_stun(payload, &tid) {
                last = r;
                if !last.failed {
                    return last;
                }
            }
        }
    }
    last
}

fn make_tid() -> [u8; 16] {
    let mut tid = [0u8; 16];
    tid[..4].copy_from_slice(&MAGIC);
    let r = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    tid[4..12].copy_from_slice(&r.to_le_bytes());
    tid[12] = (r >> 8) as u8;
    tid[13] = (r >> 16) as u8;
    tid[14] = (r >> 24) as u8;
    tid[15] = (r >> 32) as u8;
    tid
}

fn wrap_socks_udp(host: &str, port: u16, data: &[u8]) -> Vec<u8> {
    let mut out = vec![0, 0, 0]; // RSV RSV FRAG
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        out.push(0x01);
        out.extend_from_slice(&ip.octets());
    } else {
        out.push(0x03);
        let b = host.as_bytes();
        out.push(b.len() as u8);
        out.extend_from_slice(b);
    }
    out.extend_from_slice(&port.to_be_bytes());
    out.extend_from_slice(data);
    out
}

fn unwrap_socks_udp(buf: &[u8]) -> Option<&[u8]> {
    if buf.len() < 10 || buf[0] != 0 || buf[1] != 0 || buf[2] != 0 {
        return None;
    }
    let mut i = 4;
    match buf.get(3)? {
        0x01 => i += 4,
        0x03 => {
            let n = *buf.get(4)? as usize;
            i = 5 + n;
        }
        0x04 => i += 16,
        _ => return None,
    }
    i += 2; // port
    if i > buf.len() {
        return None;
    }
    Some(&buf[i..])
}

fn parse_stun(payload: &[u8], tid: &[u8; 16]) -> Option<StunResp> {
    if payload.len() < 20 {
        return None;
    }
    if payload[0..2] != BIND_RESPONSE {
        return None;
    }
    if payload[4..20] != *tid {
        return None;
    }
    let mut r = StunResp::default();
    let mut pos = 20usize;
    while pos + 4 <= payload.len() {
        let atype = &payload[pos..pos + 2];
        let alen = u16::from_be_bytes([payload[pos + 2], payload[pos + 3]]) as usize;
        let start = pos + 4;
        let end = start + alen;
        if end > payload.len() {
            break;
        }
        let val = &payload[start..end];
        if atype == ATTR_MAPPED {
            if let Some((ip, port)) = parse_addr_attr(val) {
                r.ext_ip = ip;
                r.ext_port = port;
                r.failed = false;
            }
        } else if atype == ATTR_XOR_MAPPED {
            if let Some((ip, port)) = parse_xor_addr_attr(val, tid) {
                r.ext_ip = ip;
                r.ext_port = port;
                r.failed = false;
            }
        } else if atype == ATTR_CHANGED {
            // CHANGED 只记备用地址，不能单独把 failed 清掉（否则无 MAPPED 会误判成功）
            if let Some((ip, port)) = parse_addr_attr(val) {
                r.change_ip = ip;
                r.change_port = port;
            }
        }
        let pad = (4 - (alen % 4)) % 4;
        pos = end + pad;
    }
    Some(r)
}

fn parse_addr_attr(val: &[u8]) -> Option<(Ipv4Addr, u16)> {
    // reserved(1) family(1) port(2) addr...
    if val.len() < 8 || val[1] != 0x01 {
        return None;
    }
    let port = u16::from_be_bytes([val[2], val[3]]);
    let ip = Ipv4Addr::new(val[4], val[5], val[6], val[7]);
    Some((ip, port))
}

fn parse_xor_addr_attr(val: &[u8], tid: &[u8; 16]) -> Option<(Ipv4Addr, u16)> {
    if val.len() < 8 || val[1] != 0x01 {
        return None;
    }
    let xport = u16::from_be_bytes([val[2], val[3]]) ^ u16::from_be_bytes([tid[0], tid[1]]);
    let mut oct = [val[4], val[5], val[6], val[7]];
    for i in 0..4 {
        oct[i] ^= tid[i];
    }
    Some((Ipv4Addr::from(oct), xport))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// 经本地 mihomo DIRECT SOCKS 跑一轮 STUN，确认不是整列消失那种空结果。
    #[tokio::test]
    async fn live_stun_thru_mihomo_direct_not_empty() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mihomo = root
            .join("engine")
            .join("tools")
            .join("clients")
            .join(if cfg!(windows) {
                "mihomo.exe"
            } else {
                "mihomo"
            });
        if !mihomo.is_file() {
            eprintln!("skip live stun: no mihomo at {}", mihomo.display());
            return;
        }
        let dir = std::env::temp_dir().join(format!("ns-stun-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        const PORT: u16 = 61988;
        let cfg = dir.join("config.yaml");
        std::fs::write(
            &cfg,
            format!(
                "socks-port: {PORT}\nallow-lan: false\nmode: global\nlog-level: silent\n\
                 dns:\n  enable: true\n  listen: 0.0.0.0:0\n  enhanced-mode: fake-ip\n\
                 proxy-groups:\n  - name: GLOBAL\n    type: select\n    proxies: [DIRECT]\n"
            ),
        )
        .expect("cfg");
        let mut child = std::process::Command::new(&mihomo)
            .arg("-d")
            .arg(&dir)
            .arg("-f")
            .arg(&cfg)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn");
        let mut ready = false;
        for _ in 0..40 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if TcpStream::connect(("127.0.0.1", PORT)).await.is_ok() {
                ready = true;
                break;
            }
        }
        if !ready {
            let _ = child.kill();
            panic!("mihomo SOCKS :{PORT} 未就绪");
        }
        let t0 = Instant::now();
        let nat = detect_nat_thru_socks5(PORT).await;
        let elapsed = t0.elapsed();
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&dir);
        eprintln!("live stun: nat={nat} elapsed={:.2}s", elapsed.as_secs_f64());
        assert!(
            !nat.is_empty(),
            "NAT 结果为空"
        );
        assert!(
            elapsed <= Duration::from_secs(12),
            "STUN 超时过久: {:.2}s",
            elapsed.as_secs_f64()
        );
        // DIRECT 本机通常能分出锥型；偶发 Unknown 也算测过，但不能是空串
        assert!(
            matches!(
                nat.as_str(),
                "FullCone"
                    | "RestrictedCone"
                    | "PortRestrictedCone"
                    | "Symmetric"
                    | "Blocked"
                    | "Unknown"
            ),
            "意外 NAT 值: {nat}"
        );
    }
}
