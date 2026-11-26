//! Finite deterministic datasets. No filesystem access, registration or accounts.
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

pub const FILE_BYTES: usize = 65536;
pub const VISUAL_DIRECTORY: &str = "generated-目录";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dataset {
    #[default]
    StandardV1,
    VisualV1,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub relative_path: String,
    pub directory: bool,
    pub size: usize,
}
impl Dataset {
    pub const fn root_identity(self) -> &'static [u8] {
        match self {
            Self::StandardV1 => b"twodrive-cfapi-generated-fixture-v1",
            Self::VisualV1 => b"twodrive-cfapi-generated-visual-v1",
        }
    }
    pub fn from_identity(identity: &[u8]) -> Option<Self> {
        [Self::StandardV1, Self::VisualV1]
            .into_iter()
            .find(|d| d.root_identity() == identity)
    }
    pub fn provider_binding(self) -> &'static [u8] {
        match self {
            Self::StandardV1 => b"twodrive-owned-generated-provider-v1",
            Self::VisualV1 => b"twodrive-owned-generated-visual-provider-v1",
        }
    }
    pub fn version(self) -> &'static str {
        match self {
            Self::StandardV1 => "generated-version-1",
            Self::VisualV1 => "generated-visual-version-1",
        }
    }
    pub fn files(self) -> Vec<Entry> {
        let names = match self {
            Self::StandardV1 => vec![
                "generated-中文.bin".into(),
                format!("generated-{}.bin", "long-".repeat(35)),
                "generated-offline.bin".into(),
            ],
            Self::VisualV1 => vec![
                "generated-中文.bin".into(),
                format!("{VISUAL_DIRECTORY}/generated-preview.png"),
                format!(
                    "{VISUAL_DIRECTORY}/generated-offline-{}.bin",
                    "long-".repeat(30)
                ),
            ],
        };
        names
            .into_iter()
            .enumerate()
            .map(|(index, relative_path)| Entry {
                id: format!(
                    "{}-object-{index}",
                    if self == Self::StandardV1 {
                        "generated"
                    } else {
                        "visual"
                    }
                ),
                relative_path,
                directory: false,
                size: FILE_BYTES,
            })
            .collect()
    }
    pub fn directories(self) -> Vec<Entry> {
        if self == Self::VisualV1 {
            vec![Entry {
                id: "visual-directory-0".into(),
                relative_path: VISUAL_DIRECTORY.into(),
                directory: true,
                size: 0,
            }]
        } else {
            vec![]
        }
    }
    pub fn entries(self) -> Vec<Entry> {
        self.directories().into_iter().chain(self.files()).collect()
    }
    pub fn bytes(self, index: usize) -> Vec<u8> {
        assert!(index < 3);
        if self == Self::VisualV1 && index == 1 {
            png()
        } else {
            (0..FILE_BYTES)
                .map(|i| ((i + index * 17) % 251) as u8)
                .collect()
        }
    }
}

// A real 64x64 RGB PNG, with an uncompressed zlib stream and a legal tEXt
// ancillary chunk making the complete PNG exactly 64 KiB. No bytes after IEND.
fn png() -> Vec<u8> {
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], bytes: &[u8]) {
        out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(bytes);
        let mut crc = 0xffff_ffffu32;
        for &byte in &out[start..] {
            crc ^= byte as u32;
            for _ in 0..8 {
                crc = (crc >> 1) ^ if crc & 1 != 0 { 0xedb8_8320 } else { 0 };
            }
        }
        out.extend_from_slice(&(!crc).to_be_bytes());
    }
    let mut raw = Vec::with_capacity(64 * 193);
    for y in 0..64u8 {
        raw.push(0);
        for x in 0..64u8 {
            raw.extend_from_slice(&[x * 4, y * 4, (x ^ y) * 4]);
        }
    }
    let mut zlib = vec![0x78, 0x01, 0x01];
    let length = raw.len() as u16;
    zlib.extend_from_slice(&length.to_le_bytes());
    zlib.extend_from_slice(&(!length).to_le_bytes());
    zlib.extend_from_slice(&raw);
    let (mut a, mut b) = (1u32, 0u32);
    for byte in raw {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&64u32.to_be_bytes());
    ihdr.extend_from_slice(&64u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib);
    let mut text = vec![b'x'; FILE_BYTES - out.len() - 24];
    text[..9].copy_from_slice(b"TwoDrive\0");
    chunk(&mut out, b"tEXt", &text);
    chunk(&mut out, b"IEND", &[]);
    out
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundEntry {
    pub relative_path: String,
    pub file_id: u64,
    pub directory: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetentionManifest {
    pub schema_version: u32,
    pub dataset: Dataset,
    pub volume_serial: u32,
    pub root_file_id: u64,
    pub entries: Vec<BoundEntry>,
}
impl RetentionManifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 || self.root_file_id == 0 {
            return Err("unknown retention schema/root".into());
        }
        let expected = self.dataset.entries();
        if expected.len() != self.entries.len() {
            return Err("retention entry count differs".into());
        }
        let mut ids = std::collections::HashSet::new();
        let mut paths = std::collections::HashSet::new();
        for entry in &self.entries {
            if entry.file_id == 0
                || entry.file_id == self.root_file_id
                || !ids.insert(entry.file_id)
                || !paths.insert(&entry.relative_path)
                || !expected.iter().any(|e| {
                    e.relative_path == entry.relative_path && e.directory == entry.directory
                })
            {
                return Err("foreign/duplicate retention entry".into());
            }
        }
        Ok(())
    }
    pub fn authorizes(&self, relative: &str, volume: u32, file_id: u64, directory: bool) -> bool {
        self.validate().is_ok()
            && self.volume_serial == volume
            && self.entries.iter().any(|e| {
                e.relative_path == relative && e.file_id == file_id && e.directory == directory
            })
    }
}

#[derive(Default)]
pub struct DownloadCounters {
    provider: [AtomicU64; 3],
    cache: [AtomicU64; 3],
}
impl DownloadCounters {
    pub fn provider_written(&self, index: usize, bytes: usize) {
        self.provider[index].fetch_add(bytes as u64, Ordering::SeqCst);
    }
    pub fn cache_read(&self, index: usize, bytes: usize) {
        self.cache[index].fetch_add(bytes as u64, Ordering::SeqCst);
    }
    pub fn snapshot(&self) -> serde_json::Value {
        let provider = self.provider.each_ref().map(|v| v.load(Ordering::SeqCst));
        let cache = self.cache.each_ref().map(|v| v.load(Ordering::SeqCst));
        // Range bytes returned to the adapter, excluding whole-file integrity
        // hashing IO. Provider and native transfer totals are separate metrics.
        serde_json::json!({"provider_download_bytes":provider.iter().sum::<u64>(),"cache_served_bytes":cache.iter().sum::<u64>(),"provider_bytes_by_file":provider,"cache_served_bytes_by_file":cache})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn visual_layout_is_three_files_one_directory_and_a_real_png_envelope() {
        let dataset = Dataset::VisualV1;
        assert_eq!(dataset.files().len(), 3);
        assert_eq!(dataset.directories().len(), 1);
        assert_eq!(
            dataset.files().iter().map(|f| f.size).sum::<usize>(),
            3 * 65536
        );
        let bytes = dataset.bytes(1);
        assert_eq!(bytes.len(), 65536);
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&bytes[bytes.len() - 8..bytes.len() - 4], b"IEND");
        assert!(dataset.files()[1].relative_path.ends_with(".png"));
        assert_ne!(
            dataset.provider_binding(),
            Dataset::StandardV1.provider_binding()
        );
    }
    #[test]
    fn retention_binding_refuses_replacement_wrong_root_schema_and_duplicate_paths() {
        let mut m = RetentionManifest {
            schema_version: 1,
            dataset: Dataset::VisualV1,
            volume_serial: 17,
            root_file_id: 9,
            entries: Dataset::VisualV1
                .entries()
                .into_iter()
                .enumerate()
                .map(|(i, e)| BoundEntry {
                    relative_path: e.relative_path,
                    file_id: 100 + i as u64,
                    directory: e.directory,
                })
                .collect(),
        };
        assert!(m.validate().is_ok());
        let e = &m.entries[1];
        assert!(m.authorizes(&e.relative_path, 17, e.file_id, false));
        assert!(!m.authorizes(&e.relative_path, 18, e.file_id, false));
        assert!(!m.authorizes(&e.relative_path, 17, 999, false));
        let valid = m.clone();
        m.entries[0].relative_path = m.entries[1].relative_path.clone();
        assert!(m.validate().is_err());
        m = valid.clone();
        m.entries[0].file_id = m.entries[1].file_id;
        assert!(m.validate().is_err());
        m = valid.clone();
        m.entries[0].relative_path = "../other".into();
        assert!(m.validate().is_err());
        m = valid.clone();
        m.schema_version = 2;
        assert!(m.validate().is_err());
        m = valid;
        m.root_file_id = 0;
        assert!(m.validate().is_err());
    }
}
