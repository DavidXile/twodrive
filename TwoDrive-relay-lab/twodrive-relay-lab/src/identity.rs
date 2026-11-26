use crate::util::{atomic_write, now_unix, safe_component};
use anyhow::{Context, bail};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use x25519_dalek::{PublicKey as BoxPublicKey, StaticSecret};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceManifest {
    pub version: u8,
    pub device_id: String,
    pub device_name: String,
    pub signing_public_b64: String,
    pub box_public_b64: String,
    pub updated_unix: i64,
    pub signature_b64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredIdentity {
    version: u8,
    device_name: String,
    signing_secret_b64: String,
    box_secret_b64: String,
}

#[derive(Serialize)]
struct UnsignedManifest<'a> {
    version: u8,
    device_id: &'a str,
    device_name: &'a str,
    signing_public_b64: &'a str,
    box_public_b64: &'a str,
    updated_unix: i64,
}

pub struct DeviceIdentity {
    pub device_name: String,
    signing: SigningKey,
    box_secret: StaticSecret,
}

impl DeviceIdentity {
    pub fn create(device_name: String) -> Self {
        let signing = SigningKey::generate(&mut OsRng);
        let box_secret = StaticSecret::random_from_rng(OsRng);
        Self { device_name, signing, box_secret }
    }

    pub fn load_or_create(path: &Path, requested_name: Option<&str>) -> anyhow::Result<Self> {
        if path.exists() {
            let data = std::fs::read(path)?;
            let stored: StoredIdentity = serde_json::from_slice(&data)?;
            let signing_bytes = decode_32(&stored.signing_secret_b64, "signing secret")?;
            let box_bytes = decode_32(&stored.box_secret_b64, "box secret")?;
            let signing = SigningKey::from_bytes(&signing_bytes);
            let box_secret = StaticSecret::from(box_bytes);
            let mut identity = Self { device_name: stored.device_name, signing, box_secret };
            if let Some(name) = requested_name.filter(|v| !v.trim().is_empty()) {
                validate_device_name(name)?;
                if identity.device_name != name {
                    identity.device_name = name.to_string();
                    identity.save(path)?;
                }
            }
            return Ok(identity);
        }
        let name = requested_name
            .filter(|v| !v.trim().is_empty())
            .context("identity does not exist yet; pass --name")?;
        validate_device_name(name)?;
        let identity = Self::create(name.to_string());
        identity.save(path)?;
        Ok(identity)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let stored = StoredIdentity {
            version: 1,
            device_name: self.device_name.clone(),
            signing_secret_b64: URL_SAFE_NO_PAD.encode(self.signing.to_bytes()),
            box_secret_b64: URL_SAFE_NO_PAD.encode(self.box_secret.to_bytes()),
        };
        atomic_write(path, &serde_json::to_vec_pretty(&stored)?, true)
    }

    pub fn device_id(&self) -> String {
        device_id_from_verifying_key(&self.signing.verifying_key())
    }

    pub fn signing_public(&self) -> VerifyingKey {
        self.signing.verifying_key()
    }

    pub fn box_secret(&self) -> &StaticSecret {
        &self.box_secret
    }

    pub fn box_public(&self) -> BoxPublicKey {
        BoxPublicKey::from(&self.box_secret)
    }

    pub fn manifest(&self) -> DeviceManifest {
        let mut manifest = DeviceManifest {
            version: 1,
            device_id: self.device_id(),
            device_name: self.device_name.clone(),
            signing_public_b64: URL_SAFE_NO_PAD.encode(self.signing.verifying_key().to_bytes()),
            box_public_b64: URL_SAFE_NO_PAD.encode(self.box_public().as_bytes()),
            updated_unix: now_unix(),
            signature_b64: String::new(),
        };
        let bytes = manifest_signing_bytes(&manifest);
        manifest.signature_b64 = URL_SAFE_NO_PAD.encode(self.signing.sign(&bytes).to_bytes());
        manifest
    }

    pub fn sign(&self, bytes: &[u8]) -> Signature {
        self.signing.sign(bytes)
    }
}

impl DeviceManifest {
    pub fn verify(&self) -> anyhow::Result<()> {
        if self.version != 1 {
            bail!("unsupported device manifest version {}", self.version);
        }
        validate_device_name(&self.device_name)?;
        if self.device_id.len() != 32 || !safe_component(&self.device_id) {
            bail!("invalid device id");
        }
        let public = VerifyingKey::from_bytes(&decode_32(&self.signing_public_b64, "signing public key")?)?;
        if device_id_from_verifying_key(&public) != self.device_id {
            bail!("device id does not match signing public key");
        }
        let signature = Signature::from_slice(
            &URL_SAFE_NO_PAD.decode(&self.signature_b64).context("invalid manifest signature encoding")?,
        )?;
        public.verify(&manifest_signing_bytes(self), &signature)?;
        let _ = self.box_public_key()?;
        Ok(())
    }

    pub fn signing_key(&self) -> anyhow::Result<VerifyingKey> {
        Ok(VerifyingKey::from_bytes(&decode_32(&self.signing_public_b64, "signing public key")?)?)
    }

    pub fn box_public_key(&self) -> anyhow::Result<BoxPublicKey> {
        Ok(BoxPublicKey::from(decode_32(&self.box_public_b64, "box public key")?))
    }
}

fn manifest_signing_bytes(m: &DeviceManifest) -> Vec<u8> {
    serde_json::to_vec(&UnsignedManifest {
        version: m.version,
        device_id: &m.device_id,
        device_name: &m.device_name,
        signing_public_b64: &m.signing_public_b64,
        box_public_b64: &m.box_public_b64,
        updated_unix: m.updated_unix,
    })
    .expect("serializing a device manifest cannot fail")
}

fn validate_device_name(name: &str) -> anyhow::Result<()> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > 128 || trimmed.chars().any(char::is_control) {
        bail!("device name must be 1..128 visible characters");
    }
    Ok(())
}

pub fn device_id_from_verifying_key(key: &VerifyingKey) -> String {
    let digest = Sha256::digest(key.as_bytes());
    hex::encode(&digest[..16])
}

fn decode_32(value: &str, label: &str) -> anyhow::Result<[u8; 32]> {
    let bytes = URL_SAFE_NO_PAD.decode(value).with_context(|| format!("invalid {label} encoding"))?;
    bytes.try_into().map_err(|_| anyhow::anyhow!("{label} must be 32 bytes"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_roundtrip_verifies() {
        let id = DeviceIdentity::create("test-device".to_string());
        let m = id.manifest();
        m.verify().unwrap();
    }

    #[test]
    fn manifest_tamper_fails() {
        let id = DeviceIdentity::create("test-device".to_string());
        let mut m = id.manifest();
        m.device_name = "evil".to_string();
        assert!(m.verify().is_err());
    }
}
