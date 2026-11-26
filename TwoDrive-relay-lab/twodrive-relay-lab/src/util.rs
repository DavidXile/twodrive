use anyhow::{Context, bail};
use rand::{Rng, distributions::Alphanumeric};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn random_ascii(len: usize) -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

pub fn parse_bytes(value: &str) -> anyhow::Result<u64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("empty size");
    }
    let split = trimmed
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(trimmed.len());
    let number: f64 = trimmed[..split].parse().context("invalid size number")?;
    let unit = trimmed[split..].trim().to_ascii_lowercase();
    let multiplier = match unit.as_str() {
        "" | "b" => 1_f64,
        "kib" | "ki" => 1024_f64,
        "kb" | "k" => 1000_f64,
        "mib" | "mi" => 1024_f64.powi(2),
        "mb" | "m" => 1000_f64.powi(2),
        "gib" | "gi" => 1024_f64.powi(3),
        "gb" | "g" => 1000_f64.powi(3),
        other => bail!("unsupported size unit: {other}"),
    };
    let bytes = number * multiplier;
    if !bytes.is_finite() || bytes < 0.0 || bytes > u64::MAX as f64 {
        bail!("size is out of range");
    }
    Ok(bytes.round() as u64)
}

pub fn parse_duration(value: &str) -> anyhow::Result<i64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("empty duration");
    }
    let split = trimmed
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(trimmed.len());
    let amount: i64 = trimmed[..split].parse().context("invalid duration number")?;
    let unit = trimmed[split..].trim().to_ascii_lowercase();
    let multiplier = match unit.as_str() {
        "" | "s" => 1,
        "m" | "min" => 60,
        "h" | "hr" => 60 * 60,
        "d" | "day" => 24 * 60 * 60,
        other => bail!("unsupported duration unit: {other}"),
    };
    amount.checked_mul(multiplier).context("duration overflow")
}

pub fn atomic_write(path: &Path, bytes: &[u8], private: bool) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("tmp-{}-{}", std::process::id(), random_ascii(8)));
    let mut options = fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

pub fn safe_filename(name: &str) -> String {
    let leaf = Path::new(name)
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("received.bin");
    let mut out = String::with_capacity(leaf.len());
    for ch in leaf.chars() {
        if ch.is_control() || matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
            out.push('_');
        } else {
            out.push(ch);
        }
    }
    let out = out.trim().trim_matches('.').to_string();
    if out.is_empty() { "received.bin".to_string() } else { out }
}

pub fn unique_output_path(dir: &Path, name: &str, file_id: &str) -> PathBuf {
    let base = safe_filename(name);
    let candidate = dir.join(&base);
    if !candidate.exists() {
        return candidate;
    }
    let p = Path::new(&base);
    let stem = p.file_stem().and_then(|v| v.to_str()).unwrap_or("received");
    let ext = p.extension().and_then(|v| v.to_str());
    let suffix = &file_id[..file_id.len().min(8)];
    let renamed = match ext {
        Some(ext) => format!("{stem}-received-{suffix}.{ext}"),
        None => format!("{stem}-received-{suffix}"),
    };
    dir.join(renamed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sizes() {
        assert_eq!(parse_bytes("1GiB").unwrap(), 1024 * 1024 * 1024);
        assert_eq!(parse_bytes("512MiB").unwrap(), 512 * 1024 * 1024);
        assert_eq!(parse_bytes("1GB").unwrap(), 1_000_000_000);
    }

    #[test]
    fn parses_durations() {
        assert_eq!(parse_duration("24h").unwrap(), 86_400);
        assert_eq!(parse_duration("7d").unwrap(), 604_800);
    }
}
