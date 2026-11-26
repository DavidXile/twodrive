use crate::crypto::{
    ChunkSpec, RelayEnvelope, RelayPayload, decrypt_envelope, encrypt_chunk, payload_data,
    random_file_id,
};
use crate::identity::{DeviceIdentity, DeviceManifest};
use crate::util::{atomic_write, safe_component, unique_output_path};
use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const CHUNK_BYTES: usize = 4 * 1024 * 1024;
const MAX_CHUNK_COUNT: u32 = 1_048_576; // ~4 TiB with 4 MiB chunks.

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct AssemblyMeta {
    file_id: String,
    original_name: String,
    original_size: u64,
    chunk_count: u32,
    sender_id: String,
}

pub fn stream_file_envelopes<F>(
    identity: &DeviceIdentity,
    peer: &DeviceManifest,
    path: &Path,
    ttl_seconds: i64,
    mut on_envelope: F,
) -> anyhow::Result<u32>
where
    F: FnMut(u32, u32, &RelayEnvelope) -> anyhow::Result<()>,
{
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        bail!("{} is not a regular file", path.display());
    }
    let original_size = metadata.len();
    let original_name = path
        .file_name()
        .and_then(|v| v.to_str())
        .context("file name is not valid UTF-8")?;
    if original_name.len() > 255 {
        bail!("file name is too long for relay transport");
    }
    let chunk_count = expected_chunk_count(original_size)?;
    let file_id = random_file_id();
    let mut file = File::open(path)?;
    for index in 0..chunk_count {
        let remaining = original_size.saturating_sub(index as u64 * CHUNK_BYTES as u64);
        let wanted = if original_size == 0 {
            0
        } else {
            remaining.min(CHUNK_BYTES as u64) as usize
        };
        let mut data = vec![0_u8; wanted];
        if wanted > 0 {
            file.read_exact(&mut data)?;
        }
        let envelope = encrypt_chunk(
            identity,
            peer,
            ChunkSpec {
                file_id: &file_id,
                chunk_index: index,
                chunk_count,
                original_name,
                original_size,
                data: &data,
                ttl_seconds,
            },
        )?;
        on_envelope(index, chunk_count, &envelope)?;
    }
    Ok(chunk_count)
}

pub fn accept_envelope(
    identity: &DeviceIdentity,
    trusted: &[DeviceManifest],
    envelope: &RelayEnvelope,
    inbox_root: &Path,
    output_dir: &Path,
) -> anyhow::Result<Option<PathBuf>> {
    let payload = decrypt_envelope(identity, envelope, trusted)?;
    accept_payload(&payload, inbox_root, output_dir)
}

fn accept_payload(
    payload: &RelayPayload,
    inbox_root: &Path,
    output_dir: &Path,
) -> anyhow::Result<Option<PathBuf>> {
    validate_payload_shape(payload)?;
    let completed_dir = inbox_root.join("completed");
    fs::create_dir_all(&completed_dir)?;
    let completed_marker = completed_dir.join(format!("{}.done", payload.file_id));
    if completed_marker.exists() {
        // This file was already reconstructed. Returning success lets the caller ACK a replay
        // without creating another output copy.
        return Ok(None);
    }

    let file_dir = inbox_root.join(&payload.file_id);
    fs::create_dir_all(&file_dir)?;
    let chunk_path = file_dir.join(format!("{:08}.chunk", payload.chunk_index));
    if !chunk_path.exists() {
        atomic_write(&chunk_path, payload_data(payload), false)?;
    } else {
        let existing = fs::read(&chunk_path)?;
        if existing != payload_data(payload) {
            bail!("relay replay for an existing chunk contains different data");
        }
    }

    let expected_meta = AssemblyMeta {
        file_id: payload.file_id.clone(),
        original_name: payload.original_name.clone(),
        original_size: payload.original_size,
        chunk_count: payload.chunk_count,
        sender_id: payload.sender_id.clone(),
    };
    let meta_path = file_dir.join("meta.json");
    if meta_path.exists() {
        let existing: AssemblyMeta = serde_json::from_slice(&fs::read(&meta_path)?)?;
        if existing != expected_meta {
            bail!("relay chunks with the same file id disagree on file metadata");
        }
    } else {
        atomic_write(&meta_path, &serde_json::to_vec_pretty(&expected_meta)?, false)?;
    }

    for idx in 0..payload.chunk_count {
        if !file_dir.join(format!("{:08}.chunk", idx)).exists() {
            return Ok(None);
        }
    }

    fs::create_dir_all(output_dir)?;
    let output = unique_output_path(output_dir, &payload.original_name, &payload.file_id);
    let part_name = format!(
        "{}.tdpart",
        output
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("received")
    );
    let part = output.with_file_name(part_name);
    let mut out = File::create(&part)?;
    let mut total = 0_u64;
    for idx in 0..payload.chunk_count {
        let data = fs::read(file_dir.join(format!("{:08}.chunk", idx)))?;
        let expected_len = expected_chunk_len(payload.original_size, idx, payload.chunk_count)?;
        if data.len() != expected_len {
            let _ = fs::remove_file(&part);
            bail!("stored relay chunk {idx} has an unexpected length");
        }
        out.write_all(&data)?;
        total += data.len() as u64;
    }
    out.sync_all()?;
    drop(out);
    if total != payload.original_size {
        let _ = fs::remove_file(&part);
        bail!(
            "reassembled file size mismatch: expected {}, got {}",
            payload.original_size,
            total
        );
    }
    fs::rename(&part, &output)?;
    atomic_write(
        &completed_marker,
        format!("{}\n", output.display()).as_bytes(),
        false,
    )?;
    fs::remove_dir_all(&file_dir)?;
    Ok(Some(output))
}

fn validate_payload_shape(payload: &RelayPayload) -> anyhow::Result<()> {
    if !safe_component(&payload.file_id) || payload.file_id.len() > 64 {
        bail!("unsafe relay file id");
    }
    if payload.original_name.is_empty() || payload.original_name.len() > 255 {
        bail!("invalid relay file name length");
    }
    let expected_count = expected_chunk_count(payload.original_size)?;
    if payload.chunk_count != expected_count {
        bail!(
            "relay chunk count does not match original file size: expected {}, got {}",
            expected_count,
            payload.chunk_count
        );
    }
    if payload.chunk_index >= payload.chunk_count {
        bail!("relay chunk index is out of range");
    }
    let expected_len = expected_chunk_len(
        payload.original_size,
        payload.chunk_index,
        payload.chunk_count,
    )?;
    if payload.chunk_data.len() != expected_len {
        bail!(
            "relay chunk length mismatch: expected {}, got {}",
            expected_len,
            payload.chunk_data.len()
        );
    }
    Ok(())
}

fn expected_chunk_count(size: u64) -> anyhow::Result<u32> {
    let count = if size == 0 {
        1_u64
    } else {
        (size + CHUNK_BYTES as u64 - 1) / CHUNK_BYTES as u64
    };
    if count > MAX_CHUNK_COUNT as u64 {
        bail!("file is too large for relay protocol v1");
    }
    Ok(count as u32)
}

fn expected_chunk_len(size: u64, index: u32, count: u32) -> anyhow::Result<usize> {
    if count == 0 || index >= count {
        bail!("invalid chunk coordinates");
    }
    if size == 0 {
        return Ok(0);
    }
    let start = index as u64 * CHUNK_BYTES as u64;
    let remaining = size.saturating_sub(start);
    Ok(remaining.min(CHUNK_BYTES as u64) as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{ChunkSpec, encrypt_chunk};
    use tempfile::tempdir;

    #[test]
    fn rejects_path_traversal_file_id() {
        let a = DeviceIdentity::create("a".into());
        let b = DeviceIdentity::create("b".into());
        let env = encrypt_chunk(
            &a,
            &b.manifest(),
            ChunkSpec {
                file_id: "../../escape",
                chunk_index: 0,
                chunk_count: 1,
                original_name: "x.txt",
                original_size: 1,
                data: b"x",
                ttl_seconds: 60,
            },
        )
        .unwrap();
        let inbox = tempdir().unwrap();
        let out = tempdir().unwrap();
        assert!(accept_envelope(&b, &[a.manifest()], &env, inbox.path(), out.path()).is_err());
    }
}
