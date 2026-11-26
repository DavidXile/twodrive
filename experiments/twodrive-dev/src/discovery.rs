//! Opt-in IPv4 LAN discovery. Signed advertisements identify the announcing key;
//! they never authorize a client or contain invitations, grants or share paths.
use crate::state::{State, random_secret};
use anyhow::ensure;
use iroh::{PublicKey, SecretKey, Signature};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    net::{Ipv4Addr, SocketAddr, UdpSocket},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

pub const PORT: u16 = 49183;
const DOMAIN: &[u8] = b"twodrive:lan-discovery:v1\0";
const MAGIC: &str = "twodrive-discovery";
const MAX_PACKET: usize = 4096;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Probe {
    magic: String,
    version: u8,
    nonce: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Advertisement {
    magic: String,
    version: u8,
    nonce: String,
    device: PublicKey,
    name: String,
    writable: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Signed {
    advertisement: Advertisement,
    signature: Signature,
}
#[derive(Clone, Serialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub writable: bool,
    pub fingerprint: String,
}
fn message(a: &Advertisement) -> anyhow::Result<Vec<u8>> {
    let mut m = DOMAIN.to_vec();
    m.extend(serde_json::to_vec(a)?);
    Ok(m)
}
fn valid_nonce(n: &str) -> bool {
    n.len() == 43
        && n.bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
}
fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name.len() <= 120 && !name.chars().any(char::is_control)
}
fn response(bytes: &[u8], nonce: &str) -> anyhow::Result<Device> {
    ensure!(bytes.len() <= MAX_PACKET, "discovery packet too large");
    let s: Signed =
        serde_json::from_slice(bytes).map_err(|_| anyhow::anyhow!("invalid discovery response"))?;
    let a = &s.advertisement;
    ensure!(
        a.magic == MAGIC && a.version == 1 && a.nonce == nonce && valid_name(&a.name),
        "invalid discovery attempt"
    );
    a.device
        .verify(&message(a)?, &s.signature)
        .map_err(|_| anyhow::anyhow!("invalid discovery signature"))?;
    let id = a.device.to_string();
    Ok(Device {
        id: id.clone(),
        name: a.name.clone(),
        writable: a.writable,
        fingerprint: format!("{}…{}", &id[..12], &id[id.len() - 12..]),
    })
}
fn lan(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(ip) => ip.is_private() || ip.is_loopback() || ip.is_link_local(),
        _ => false,
    }
}
pub async fn advertise(
    state: State,
    name: String,
    writable: bool,
    cancel: std::sync::Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let socket = tokio::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, PORT)).await?;
    advertise_at(socket, state.identity()?, name, writable, cancel).await
}
async fn advertise_at(
    socket: tokio::net::UdpSocket,
    key: SecretKey,
    name: String,
    writable: bool,
    cancel: std::sync::Arc<AtomicBool>,
) -> anyhow::Result<()> {
    ensure!(
        valid_name(&name),
        "device name must be non-empty, at most 120 bytes, without control characters"
    );
    let mut buffer = [0u8; MAX_PACKET + 1];
    let mut sources = HashMap::<std::net::IpAddr, Instant>::new();
    let mut window = Instant::now();
    let mut replies = 0;
    while !cancel.load(Ordering::Relaxed) {
        let read =
            tokio::time::timeout(Duration::from_millis(100), socket.recv_from(&mut buffer)).await;
        let Ok(Ok((n, source))) = read else {
            continue;
        };
        if n > 512 || !lan(source.ip()) {
            continue;
        }
        let Ok(probe) = serde_json::from_slice::<Probe>(&buffer[..n]) else {
            continue;
        };
        if probe.magic != MAGIC || probe.version != 1 || !valid_nonce(&probe.nonce) {
            continue;
        }
        if window.elapsed() > Duration::from_secs(1) {
            window = Instant::now();
            replies = 0;
            sources.retain(|_, time| time.elapsed() < Duration::from_secs(2));
        }
        if replies >= 32
            || sources.len() >= 256
            || sources
                .get(&source.ip())
                .is_some_and(|time| time.elapsed() < Duration::from_millis(100))
        {
            continue;
        }
        let a = Advertisement {
            magic: MAGIC.into(),
            version: 1,
            nonce: probe.nonce,
            device: key.public(),
            name: name.clone(),
            writable,
        };
        let signature = key.sign(&message(&a)?);
        let bytes = serde_json::to_vec(&Signed {
            advertisement: a,
            signature,
        })?;
        if bytes.len() > MAX_PACKET {
            continue;
        }
        socket.send_to(&bytes, source).await?;
        sources.insert(source.ip(), Instant::now());
        replies += 1;
    }
    Ok(())
}
pub fn scan(cancel: &AtomicBool) -> anyhow::Result<Vec<Device>> {
    let mut targets = vec![
        SocketAddr::from((Ipv4Addr::BROADCAST, PORT)),
        SocketAddr::from((Ipv4Addr::LOCALHOST, PORT)),
    ];
    for interface in netdev::get_interfaces()
        .into_iter()
        .filter(|i| i.is_up() && !i.is_loopback() && !i.is_tun())
    {
        for network in interface.ipv4 {
            targets.push(SocketAddr::from((network.broadcast(), PORT)));
        }
    }
    targets.sort();
    targets.dedup();
    scan_targets(&targets, Duration::from_secs(3), cancel)
}
fn scan_targets(
    targets: &[SocketAddr],
    duration: Duration,
    cancel: &AtomicBool,
) -> anyhow::Result<Vec<Device>> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    socket.set_broadcast(true)?;
    socket.set_read_timeout(Some(Duration::from_millis(100)))?;
    let nonce = random_secret();
    let probe = serde_json::to_vec(&Probe {
        magic: MAGIC.into(),
        version: 1,
        nonce: nonce.clone(),
    })?;
    let deadline = Instant::now() + duration;
    let mut next_probe = Instant::now();
    let mut rounds = 0;
    let mut devices = BTreeMap::new();
    let mut buffer = [0u8; MAX_PACKET + 1];
    while Instant::now() < deadline {
        ensure!(!cancel.load(Ordering::Relaxed), "设备发现已取消");
        if rounds < 3 && Instant::now() >= next_probe {
            for target in targets.iter().take(64) {
                let _ = socket.send_to(&probe, target);
            }
            rounds += 1;
            next_probe = Instant::now() + Duration::from_millis(350);
        }
        match socket.recv_from(&mut buffer) {
            Ok((n, source)) if lan(source.ip()) => {
                if let Ok(device) = response(&buffer[..n], &nonce)
                    && devices.len() < 64
                {
                    devices.insert(device.id.clone(), device);
                }
            }
            Ok(_) => {}
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(devices.into_values().collect())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_discovery_rejects_tampering_and_stale_attempts() {
        let key = SecretKey::generate();
        let nonce = random_secret();
        let a = Advertisement {
            magic: MAGIC.into(),
            version: 1,
            nonce: nonce.clone(),
            device: key.public(),
            name: "Disposable NAS".into(),
            writable: true,
        };
        let signature = key.sign(&message(&a).unwrap());
        let mut s = Signed {
            advertisement: a,
            signature,
        };
        let bytes = serde_json::to_vec(&s).unwrap();
        assert!(response(&bytes, &nonce).is_ok());
        assert!(response(&bytes, &random_secret()).is_err());
        s.advertisement.name = "Forged PC".into();
        assert!(response(&serde_json::to_vec(&s).unwrap(), &nonce).is_err());
        assert!(!bytes.windows(6).any(|w| w == b"secret"));
        assert!(!bytes.windows(4).any(|w| w == b"root"));
    }
    #[test]
    fn real_loopback_udp_discovers_identity_without_authorizing_client() {
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let target = socket.local_addr().unwrap();
        socket.set_nonblocking(true).unwrap();
        let key = SecretKey::generate();
        let expected = key.public().to_string();
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    advertise_at(
                        tokio::net::UdpSocket::from_std(socket).unwrap(),
                        key,
                        "Disposable PC".into(),
                        false,
                        worker_stop,
                    )
                    .await
                    .unwrap()
                })
        });
        let devices = scan_targets(
            &[target],
            Duration::from_millis(600),
            &AtomicBool::new(false),
        )
        .unwrap();
        stop.store(true, Ordering::Relaxed);
        worker.join().unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].id, expected);
        assert!(!devices[0].writable);
    }
}
