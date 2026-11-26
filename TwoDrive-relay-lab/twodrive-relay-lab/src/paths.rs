use anyhow::Context;
use directories::ProjectDirs;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct LabPaths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub token_path: PathBuf,
    pub identity_path: PathBuf,
    pub inbox_dir: PathBuf,
}

impl LabPaths {
    pub fn discover() -> anyhow::Result<Self> {
        let dirs = ProjectDirs::from("io", "TwoDrive", "TwoDriveRelayLab")
            .context("unable to determine per-user application directories")?;
        let config_dir = dirs.config_dir().to_path_buf();
        let data_dir = dirs.data_local_dir().to_path_buf();
        Ok(Self {
            token_path: config_dir.join("tokens.json"),
            identity_path: config_dir.join("identity.json"),
            inbox_dir: data_dir.join("inbox"),
            config_dir,
            data_dir,
        })
    }

    pub fn ensure(&self) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.config_dir)?;
        std::fs::create_dir_all(&self.data_dir)?;
        std::fs::create_dir_all(&self.inbox_dir)?;
        Ok(())
    }
}
