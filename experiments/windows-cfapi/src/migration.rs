//! Import planning only. No enumeration, hydration, copy, token or cache access.
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    OrdinaryDirectory,
    OtherClientSyncRoot,
    PrivateCache,
    VirtualMount,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    FullFileNeedsApprovedPlan,
    PlaceholderNeedsOriginalClientExport,
    UnsupportedCache,
    UnsupportedVirtualMount,
    UnknownProvenance,
    SkipDirectory,
    SkipReparseOrOffline,
}

/// Windows file attributes are metadata, not proof a private cache fragment is
/// a whole file or that a managed file has finished uploading.
pub fn classify(kind: SourceKind, attributes: u32, is_file: bool) -> Disposition {
    match kind {
        SourceKind::PrivateCache => return Disposition::UnsupportedCache,
        SourceKind::VirtualMount => return Disposition::UnsupportedVirtualMount,
        SourceKind::Unknown => return Disposition::UnknownProvenance,
        _ => {}
    }
    // Reparse, offline, recall-on-open, recall-on-data-access, sparse and EFS.
    // Fail closed for unknown reparse tags too; do not follow them to classify.
    if attributes & (0x400 | 0x1000 | 0x40000 | 0x400000 | 0x200 | 0x4000) != 0 {
        return if kind == SourceKind::OtherClientSyncRoot {
            Disposition::PlaceholderNeedsOriginalClientExport
        } else {
            Disposition::SkipReparseOrOffline
        };
    }
    if !is_file {
        return Disposition::SkipDirectory;
    }
    Disposition::FullFileNeedsApprovedPlan
}

#[cfg(windows)]
pub fn preflight(path: &Path, kind: SourceKind) -> std::io::Result<Disposition> {
    use std::os::windows::fs::MetadataExt;
    if matches!(
        kind,
        SourceKind::PrivateCache | SourceKind::VirtualMount | SourceKind::Unknown
    ) {
        return Ok(classify(kind, 0, false));
    }
    let metadata = std::fs::symlink_metadata(path)?;
    Ok(classify(
        kind,
        metadata.file_attributes(),
        metadata.is_file(),
    ))
}

#[cfg(not(windows))]
pub fn preflight(_path: &Path, _kind: SourceKind) -> std::io::Result<Disposition> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Windows metadata preflight only",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn never_treat_cache_virtual_or_placeholder_as_portable_data() {
        for kind in [
            SourceKind::PrivateCache,
            SourceKind::VirtualMount,
            SourceKind::Unknown,
        ] {
            assert_ne!(
                classify(kind, 0x80, true),
                Disposition::FullFileNeedsApprovedPlan
            );
        }
        for flag in [0x400, 0x1000, 0x40000, 0x400000, 0x200, 0x4000] {
            assert_eq!(
                classify(SourceKind::OtherClientSyncRoot, flag, true),
                Disposition::PlaceholderNeedsOriginalClientExport
            );
        }
    }
    #[test]
    fn full_file_still_requires_a_plan_and_approval() {
        assert_eq!(
            classify(SourceKind::OrdinaryDirectory, 0x80, true),
            Disposition::FullFileNeedsApprovedPlan
        );
        assert_eq!(
            classify(SourceKind::OtherClientSyncRoot, 0x80, true),
            Disposition::FullFileNeedsApprovedPlan
        );
        assert_eq!(
            classify(SourceKind::OrdinaryDirectory, 0x10, false),
            Disposition::SkipDirectory
        );
    }
    #[cfg(windows)]
    #[test]
    fn metadata_preflight_on_generated_complete_file() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "twodrive-preflight-{}-{unique}.bin",
            std::process::id()
        ));
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        file.write_all(b"generated import fixture").unwrap();
        file.sync_all().unwrap();
        drop(file);
        assert_eq!(
            preflight(&path, SourceKind::OrdinaryDirectory).unwrap(),
            Disposition::FullFileNeedsApprovedPlan
        );
        assert_eq!(
            preflight(&path, SourceKind::PrivateCache).unwrap(),
            Disposition::UnsupportedCache
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"generated import fixture");
        std::fs::remove_file(&path).unwrap();
    }
}
