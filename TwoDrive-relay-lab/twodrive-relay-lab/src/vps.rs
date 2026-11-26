use crate::util::{atomic_write, now_unix, safe_component};
use anyhow::{Context, bail};
use hmac::{Hmac, Mac};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const PROTOCOL_VERSION: u8 = 1;
const MAX_HEADER: usize = 64 * 1024;
const MAX_TTL: i64 = 7 * 24 * 60 * 60;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Serialize, Deserialize)]
struct Request {
    version: u8,
    op: String,
    recipient_id: String,
    message_id: String,
    expires_at_unix: i64,
    body_len: u64,
    auth_tag_hex: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ResponseHeader {
    ok: bool,
    message: String,
    message_id: String,
    expires_at_unix: i64,
    body_len: u64,
}

pub struct VpsRelayClient {
    endpoint: String,
    token: String,
}

impl VpsRelayClient {
    pub fn new(endpoint: String, token: String) -> anyhow::Result<Self> {
        if token.len() < 24 {
            bail!("VPS relay token must be at least 24 characters");
        }
        Ok(Self { endpoint, token })
    }

    pub fn put_wait(
        &self,
        recipient_id: &str,
        message_id: &str,
        expires_at: i64,
        body: &[u8],
        max_wait: Duration,
    ) -> anyhow::Result<()> {
        let started = std::time::Instant::now();
        let mut delay = Duration::from_secs(2);
        loop {
            let (header, _) = self.request("put", recipient_id, message_id, expires_at, body)?;
            if header.ok {
                return Ok(());
            }
            if !header.message.contains("quota exceeded") {
                bail!("VPS relay rejected put: {}", header.message);
            }
            if started.elapsed() >= max_wait {
                bail!("VPS relay cache remained full for {:?}", max_wait);
            }
            std::thread::sleep(delay);
            delay = (delay * 2).min(Duration::from_secs(15));
        }
    }

    pub fn get(&self, recipient_id: &str) -> anyhow::Result<Option<(String, i64, Vec<u8>)>> {
        let (header, body) = self.request("get", recipient_id, "", 0, &[])?;
        if header.ok && header.message == "empty" { return Ok(None); }
        if !header.ok { bail!("VPS relay get failed: {}", header.message); }
        Ok(Some((header.message_id, header.expires_at_unix, body)))
    }

    pub fn ack(&self, recipient_id: &str, message_id: &str) -> anyhow::Result<()> {
        let (header, _) = self.request("ack", recipient_id, message_id, 0, &[])?;
        if !header.ok { bail!("VPS relay ack failed: {}", header.message); }
        Ok(())
    }

    fn request(&self, op: &str, recipient_id: &str, message_id: &str, expires_at: i64, body: &[u8]) -> anyhow::Result<(ResponseHeader, Vec<u8>)> {
        if !safe_component(recipient_id) || (!message_id.is_empty() && !safe_component(message_id)) {
            bail!("unsafe relay identifier");
        }
        let mut stream = TcpStream::connect(&self.endpoint)
            .with_context(|| format!("cannot connect to VPS relay {}", self.endpoint))?;
        stream.set_read_timeout(Some(Duration::from_secs(90)))?;
        stream.set_write_timeout(Some(Duration::from_secs(90)))?;
        let mut challenge = [0_u8; 32];
        stream.read_exact(&mut challenge)?;
        let body_len = body.len() as u64;
        let auth_tag_hex = compute_tag(&self.token, &challenge, op, recipient_id, message_id, expires_at, body_len)?;
        let request = Request {
            version: PROTOCOL_VERSION,
            op: op.to_string(),
            recipient_id: recipient_id.to_string(),
            message_id: message_id.to_string(),
            expires_at_unix: expires_at,
            body_len,
            auth_tag_hex,
        };
        write_json_frame(&mut stream, &request)?;
        if !body.is_empty() { stream.write_all(body)?; }
        let response: ResponseHeader = read_json_frame(&mut stream)?;
        if response.body_len > 16 * 1024 * 1024 { bail!("VPS relay response body is too large"); }
        let mut response_body = vec![0_u8; response.body_len as usize];
        stream.read_exact(&mut response_body)?;
        Ok((response, response_body))
    }
}

#[derive(Clone)]
pub struct VpsServerConfig {
    pub listen: String,
    pub data_dir: PathBuf,
    pub token: String,
    pub quota_bytes: u64,
    pub max_message_bytes: u64,
}

pub fn serve(config: VpsServerConfig) -> anyhow::Result<()> {
    if config.token.len() < 24 { bail!("VPS relay token must be at least 24 characters"); }
    fs::create_dir_all(&config.data_dir)?;
    cleanup_expired(&config.data_dir)?;
    let listener = TcpListener::bind(&config.listen)?;
    let quota_lock = Arc::new(Mutex::new(()));
    eprintln!("TwoDrive VPS relay listening on {}", config.listen);
    eprintln!("Cache: {} (hard limit {} bytes, max message {} bytes)", config.data_dir.display(), config.quota_bytes, config.max_message_bytes);
    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                let cfg = config.clone();
                let quota_lock = Arc::clone(&quota_lock);
                thread::spawn(move || {
                    if let Err(err) = handle_connection(stream, &cfg, &quota_lock) {
                        eprintln!("relay connection error: {err:#}");
                    }
                });
            }
            Err(err) => eprintln!("relay accept error: {err}"),
        }
    }
    Ok(())
}

fn handle_connection(mut stream: TcpStream, config: &VpsServerConfig, quota_lock: &Mutex<()>) -> anyhow::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(90)))?;
    stream.set_write_timeout(Some(Duration::from_secs(90)))?;
    let mut challenge = [0_u8; 32];
    OsRng.fill_bytes(&mut challenge);
    stream.write_all(&challenge)?;
    let request: Request = read_json_frame(&mut stream)?;
    if request.version != PROTOCOL_VERSION { return write_error(&mut stream, "unsupported protocol version"); }
    if !safe_component(&request.recipient_id) || (!request.message_id.is_empty() && !safe_component(&request.message_id)) {
        return write_error(&mut stream, "invalid identifier");
    }
    let expected = compute_tag(&config.token, &challenge, &request.op, &request.recipient_id, &request.message_id, request.expires_at_unix, request.body_len)?;
    if !constant_time_hex_eq(&expected, &request.auth_tag_hex) {
        return write_error(&mut stream, "authentication failed");
    }
    match request.op.as_str() {
        "put" => handle_put(&mut stream, config, quota_lock, request),
        "get" => handle_get(&mut stream, config, request),
        "ack" => handle_ack(&mut stream, config, request),
        _ => write_error(&mut stream, "unknown operation"),
    }
}

fn handle_put(stream: &mut TcpStream, config: &VpsServerConfig, quota_lock: &Mutex<()>, request: Request) -> anyhow::Result<()> {
    if request.body_len == 0 || request.body_len > config.max_message_bytes {
        return write_error(stream, "message exceeds VPS relay size limit");
    }
    let now = now_unix();
    if request.expires_at_unix <= now || request.expires_at_unix > now + MAX_TTL {
        return write_error(stream, "invalid relay expiry");
    }
    let mut body = vec![0_u8; request.body_len as usize];
    stream.read_exact(&mut body)?;
    // Serialize quota check + commit so concurrent senders cannot race the hard limit.
    // Read the bounded body first so a slow authenticated sender does not hold the global quota lock.
    let _quota_guard = quota_lock.lock().map_err(|_| anyhow::anyhow!("VPS relay quota lock is poisoned"))?;
    cleanup_expired(&config.data_dir)?;
    let used = usage_bytes(&config.data_dir)?;
    if used.saturating_add(request.body_len) > config.quota_bytes {
        return write_error(stream, "VPS relay cache quota exceeded");
    }
    let recipient_dir = config.data_dir.join(&request.recipient_id);
    fs::create_dir_all(&recipient_dir)?;
    let path = recipient_dir.join(format!("{}__{}.tdr", request.expires_at_unix, request.message_id));
    if !path.exists() { atomic_write(&path, &body, false)?; }
    write_response(stream, ResponseHeader { ok: true, message: "stored".into(), message_id: request.message_id, expires_at_unix: request.expires_at_unix, body_len: 0 }, &[])
}

fn handle_get(stream: &mut TcpStream, config: &VpsServerConfig, request: Request) -> anyhow::Result<()> {
    let dir = config.data_dir.join(&request.recipient_id);
    let Some((path, message_id, expires)) = oldest_message(&dir)? else {
        return write_response(stream, ResponseHeader { ok: true, message: "empty".into(), message_id: String::new(), expires_at_unix: 0, body_len: 0 }, &[]);
    };
    let body = fs::read(path)?;
    write_response(stream, ResponseHeader { ok: true, message: "message".into(), message_id, expires_at_unix: expires, body_len: body.len() as u64 }, &body)
}

fn handle_ack(stream: &mut TcpStream, config: &VpsServerConfig, request: Request) -> anyhow::Result<()> {
    let dir = config.data_dir.join(&request.recipient_id);
    if dir.exists() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(&format!("__{}.tdr", request.message_id)) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    write_response(stream, ResponseHeader { ok: true, message: "acked".into(), message_id: request.message_id, expires_at_unix: 0, body_len: 0 }, &[])
}

fn oldest_message(dir: &Path) -> anyhow::Result<Option<(PathBuf, String, i64)>> {
    if !dir.exists() { return Ok(None); }
    let mut items = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() { continue; }
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some((expires, message_id)) = parse_vps_name(&name) else { continue; };
        if expires <= now_unix() { let _ = fs::remove_file(entry.path()); continue; }
        items.push((entry.path(), message_id.to_string(), expires));
    }
    items.sort_by_key(|(_, _, expires)| *expires);
    Ok(items.into_iter().next())
}

fn parse_vps_name(name: &str) -> Option<(i64, &str)> {
    let stem = name.strip_suffix(".tdr")?;
    let (expires, id) = stem.split_once("__")?;
    Some((expires.parse().ok()?, id))
}

fn cleanup_expired(root: &Path) -> anyhow::Result<usize> {
    if !root.exists() { return Ok(0); }
    let mut deleted = 0;
    for dir in fs::read_dir(root)? {
        let dir = dir?;
        if !dir.file_type()?.is_dir() { continue; }
        for entry in fs::read_dir(dir.path())? {
            let entry = entry?;
            if !entry.file_type()?.is_file() { continue; }
            let name = entry.file_name().to_string_lossy().into_owned();
            if parse_vps_name(&name).map(|(exp, _)| exp <= now_unix()).unwrap_or(false) {
                fs::remove_file(entry.path())?;
                deleted += 1;
            }
        }
    }
    Ok(deleted)
}

fn usage_bytes(root: &Path) -> anyhow::Result<u64> {
    if !root.exists() { return Ok(0); }
    let mut total = 0_u64;
    for dir in fs::read_dir(root)? {
        let dir = dir?;
        if !dir.file_type()?.is_dir() { continue; }
        for entry in fs::read_dir(dir.path())? {
            let entry = entry?;
            if entry.file_type()?.is_file() { total = total.saturating_add(entry.metadata()?.len()); }
        }
    }
    Ok(total)
}

fn compute_tag(token: &str, challenge: &[u8; 32], op: &str, recipient: &str, message_id: &str, expires: i64, body_len: u64) -> anyhow::Result<String> {
    let mut mac = HmacSha256::new_from_slice(token.as_bytes()).context("invalid HMAC key")?;
    mac.update(challenge);
    mac.update(&[0]); mac.update(op.as_bytes());
    mac.update(&[0]); mac.update(recipient.as_bytes());
    mac.update(&[0]); mac.update(message_id.as_bytes());
    mac.update(&[0]); mac.update(expires.to_string().as_bytes());
    mac.update(&[0]); mac.update(body_len.to_string().as_bytes());
    Ok(hex::encode(mac.finalize().into_bytes()))
}

fn constant_time_hex_eq(expected: &str, actual: &str) -> bool {
    let Ok(a) = hex::decode(expected) else { return false; };
    let Ok(b) = hex::decode(actual) else { return false; };
    if a.len() != b.len() { return false; }
    let mut diff = 0_u8;
    for (x, y) in a.iter().zip(b.iter()) { diff |= x ^ y; }
    diff == 0
}

fn write_json_frame<T: Serialize>(stream: &mut TcpStream, value: &T) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > MAX_HEADER { bail!("protocol header too large"); }
    stream.write_all(&(bytes.len() as u32).to_be_bytes())?;
    stream.write_all(&bytes)?;
    Ok(())
}

fn read_json_frame<T: for<'de> Deserialize<'de>>(stream: &mut TcpStream) -> anyhow::Result<T> {
    let mut len = [0_u8; 4];
    stream.read_exact(&mut len)?;
    let len = u32::from_be_bytes(len) as usize;
    if len == 0 || len > MAX_HEADER { bail!("invalid protocol header length"); }
    let mut bytes = vec![0_u8; len];
    stream.read_exact(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn write_response(stream: &mut TcpStream, header: ResponseHeader, body: &[u8]) -> anyhow::Result<()> {
    write_json_frame(stream, &header)?;
    if !body.is_empty() { stream.write_all(body)?; }
    Ok(())
}

fn write_error(stream: &mut TcpStream, message: &str) -> anyhow::Result<()> {
    write_response(stream, ResponseHeader { ok: false, message: message.to_string(), message_id: String::new(), expires_at_unix: 0, body_len: 0 }, &[])
}
