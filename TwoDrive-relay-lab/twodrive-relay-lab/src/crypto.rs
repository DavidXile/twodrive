use crate::identity::{DeviceIdentity, DeviceManifest, device_id_from_verifying_key};
use crate::util::{now_unix, random_ascii};
use anyhow::{Context, bail};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce, aead::Aead};
use ed25519_dalek::{Signature, Verifier};
use hkdf::Hkdf;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey as BoxPublicKey, StaticSecret};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayEnvelope {
    pub version: u8,
    pub recipient_id: String,
    pub message_id: String,
    pub expires_at_unix: i64,
    pub ephemeral_public_b64: String,
    pub nonce_b64: String,
    #[serde(with = "serde_bytes")]
    pub ciphertext: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayPayload {
    pub version: u8,
    pub sender_id: String,
    pub sender_signing_public_b64: String,
    pub file_id: String,
    pub chunk_index: u32,
    pub chunk_count: u32,
    pub original_name: String,
    pub original_size: u64,
    pub chunk_sha256: String,
    #[serde(with = "serde_bytes")]
    pub chunk_data: Vec<u8>,
    pub signature_b64: String,
}

#[derive(Serialize)]
struct UnsignedPayload<'a> {
    version: u8,
    sender_id: &'a str,
    sender_signing_public_b64: &'a str,
    file_id: &'a str,
    chunk_index: u32,
    chunk_count: u32,
    original_name: &'a str,
    original_size: u64,
    chunk_sha256: &'a str,
}

#[derive(Serialize)]
struct EnvelopeAad<'a> {
    version: u8,
    recipient_id: &'a str,
    message_id: &'a str,
    expires_at_unix: i64,
    ephemeral_public_b64: &'a str,
    nonce_b64: &'a str,
}

pub struct ChunkSpec<'a> {
    pub file_id: &'a str,
    pub chunk_index: u32,
    pub chunk_count: u32,
    pub original_name: &'a str,
    pub original_size: u64,
    pub data: &'a [u8],
    pub ttl_seconds: i64,
}

pub fn encrypt_chunk(
    sender: &DeviceIdentity,
    recipient: &DeviceManifest,
    spec: ChunkSpec<'_>,
) -> anyhow::Result<RelayEnvelope> {
    recipient.verify()?;
    if spec.ttl_seconds <= 0 {
        bail!("relay TTL must be positive");
    }
    let chunk_hash = hex::encode(Sha256::digest(spec.data));
    let sender_id = sender.device_id();
    let sender_signing_public_b64 = URL_SAFE_NO_PAD.encode(sender.signing_public().to_bytes());
    let unsigned = UnsignedPayload {
        version: 1,
        sender_id: &sender_id,
        sender_signing_public_b64: &sender_signing_public_b64,
        file_id: spec.file_id,
        chunk_index: spec.chunk_index,
        chunk_count: spec.chunk_count,
        original_name: spec.original_name,
        original_size: spec.original_size,
        chunk_sha256: &chunk_hash,
    };
    // The signature binds all metadata and the SHA-256 digest; the digest binds the raw chunk bytes.
    let unsigned_bytes = serde_json::to_vec(&unsigned)?;
    let signature_b64 = URL_SAFE_NO_PAD.encode(sender.sign(&unsigned_bytes).to_bytes());
    let payload = RelayPayload {
        version: 1,
        sender_id,
        sender_signing_public_b64,
        file_id: spec.file_id.to_string(),
        chunk_index: spec.chunk_index,
        chunk_count: spec.chunk_count,
        original_name: spec.original_name.to_string(),
        original_size: spec.original_size,
        chunk_sha256: chunk_hash,
        chunk_data: spec.data.to_vec(),
        signature_b64,
    };
    let mut plaintext = Vec::with_capacity(spec.data.len() + 1024);
    ciborium::ser::into_writer(&payload, &mut plaintext)?;

    let ephemeral_secret = StaticSecret::random_from_rng(OsRng);
    let ephemeral_public = BoxPublicKey::from(&ephemeral_secret);
    let shared = ephemeral_secret.diffie_hellman(&recipient.box_public_key()?);
    let message_id = random_message_id();
    let key = derive_key(shared.as_bytes(), &message_id)?;
    let cipher = XChaCha20Poly1305::new_from_slice(&key)
        .map_err(|_| anyhow::anyhow!("invalid relay encryption key length"))?;
    let mut nonce = [0_u8; 24];
    OsRng.fill_bytes(&mut nonce);
    let ephemeral_public_b64 = URL_SAFE_NO_PAD.encode(ephemeral_public.as_bytes());
    let nonce_b64 = URL_SAFE_NO_PAD.encode(nonce);
    let expires_at_unix = now_unix()
        .checked_add(spec.ttl_seconds)
        .context("relay expiry overflow")?;
    let aad = EnvelopeAad {
        version: 1,
        recipient_id: &recipient.device_id,
        message_id: &message_id,
        expires_at_unix,
        ephemeral_public_b64: &ephemeral_public_b64,
        nonce_b64: &nonce_b64,
    };
    let aad_bytes = serde_json::to_vec(&aad)?;
    let ciphertext = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            chacha20poly1305::aead::Payload {
                msg: &plaintext,
                aad: &aad_bytes,
            },
        )
        .map_err(|_| anyhow::anyhow!("relay encryption failed"))?;
    Ok(RelayEnvelope {
        version: 1,
        recipient_id: recipient.device_id.clone(),
        message_id,
        expires_at_unix,
        ephemeral_public_b64,
        nonce_b64,
        ciphertext,
    })
}

pub fn decrypt_envelope(
    recipient: &DeviceIdentity,
    envelope: &RelayEnvelope,
    trusted_senders: &[DeviceManifest],
) -> anyhow::Result<RelayPayload> {
    if envelope.version != 1 {
        bail!("unsupported relay envelope version {}", envelope.version);
    }
    if envelope.recipient_id != recipient.device_id() {
        bail!("relay envelope is addressed to a different device");
    }
    if envelope.expires_at_unix <= now_unix() {
        bail!("relay envelope has expired");
    }
    let ephemeral_bytes = decode_array::<32>(&envelope.ephemeral_public_b64, "ephemeral public key")?;
    let ephemeral_public = BoxPublicKey::from(ephemeral_bytes);
    let shared = recipient.box_secret().diffie_hellman(&ephemeral_public);
    let key = derive_key(shared.as_bytes(), &envelope.message_id)?;
    let cipher = XChaCha20Poly1305::new_from_slice(&key)
        .map_err(|_| anyhow::anyhow!("invalid relay encryption key length"))?;
    let nonce = decode_array::<24>(&envelope.nonce_b64, "nonce")?;
    let aad = EnvelopeAad {
        version: envelope.version,
        recipient_id: &envelope.recipient_id,
        message_id: &envelope.message_id,
        expires_at_unix: envelope.expires_at_unix,
        ephemeral_public_b64: &envelope.ephemeral_public_b64,
        nonce_b64: &envelope.nonce_b64,
    };
    let aad_bytes = serde_json::to_vec(&aad)?;
    let plaintext = cipher
        .decrypt(
            XNonce::from_slice(&nonce),
            chacha20poly1305::aead::Payload {
                msg: &envelope.ciphertext,
                aad: &aad_bytes,
            },
        )
        .map_err(|_| anyhow::anyhow!("relay envelope authentication/decryption failed"))?;
    let payload: RelayPayload = ciborium::de::from_reader(plaintext.as_slice())?;
    verify_payload(&payload, trusted_senders)?;
    Ok(payload)
}

fn verify_payload(payload: &RelayPayload, trusted_senders: &[DeviceManifest]) -> anyhow::Result<()> {
    if payload.version != 1 {
        bail!("unsupported relay payload version {}", payload.version);
    }
    if payload.chunk_count == 0 || payload.chunk_index >= payload.chunk_count {
        bail!("invalid relay chunk numbering");
    }
    let signing_bytes = decode_array::<32>(&payload.sender_signing_public_b64, "sender signing key")?;
    let signing = ed25519_dalek::VerifyingKey::from_bytes(&signing_bytes)?;
    if device_id_from_verifying_key(&signing) != payload.sender_id {
        bail!("sender id does not match sender signing key");
    }
    let trusted = trusted_senders
        .iter()
        .find(|m| m.device_id == payload.sender_id)
        .context("relay sender is not a trusted same-account peer")?;
    trusted.verify()?;
    if trusted.signing_public_b64 != payload.sender_signing_public_b64 {
        bail!("sender signing key does not match trusted manifest");
    }
    let unsigned = UnsignedPayload {
        version: payload.version,
        sender_id: &payload.sender_id,
        sender_signing_public_b64: &payload.sender_signing_public_b64,
        file_id: &payload.file_id,
        chunk_index: payload.chunk_index,
        chunk_count: payload.chunk_count,
        original_name: &payload.original_name,
        original_size: payload.original_size,
        chunk_sha256: &payload.chunk_sha256,
    };
    let signature_bytes = URL_SAFE_NO_PAD
        .decode(&payload.signature_b64)
        .context("invalid payload signature encoding")?;
    let signature = Signature::from_slice(&signature_bytes)?;
    signing.verify(&serde_json::to_vec(&unsigned)?, &signature)?;
    if hex::encode(Sha256::digest(&payload.chunk_data)) != payload.chunk_sha256 {
        bail!("relay chunk hash mismatch");
    }
    Ok(())
}

pub fn payload_data(payload: &RelayPayload) -> &[u8] {
    &payload.chunk_data
}

pub fn encode_envelope(envelope: &RelayEnvelope) -> anyhow::Result<Vec<u8>> {
    let mut out = Vec::with_capacity(envelope.ciphertext.len() + 512);
    ciborium::ser::into_writer(envelope, &mut out)?;
    Ok(out)
}

pub fn decode_envelope(bytes: &[u8]) -> anyhow::Result<RelayEnvelope> {
    Ok(ciborium::de::from_reader(bytes)?)
}

fn derive_key(shared_secret: &[u8; 32], message_id: &str) -> anyhow::Result<[u8; 32]> {
    let hk = Hkdf::<Sha256>::new(Some(message_id.as_bytes()), shared_secret);
    let mut key = [0_u8; 32];
    hk.expand(b"twodrive-relay-envelope-v1", &mut key)
        .map_err(|_| anyhow::anyhow!("relay key derivation failed"))?;
    Ok(key)
}

fn decode_array<const N: usize>(value: &str, label: &str) -> anyhow::Result<[u8; N]> {
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .with_context(|| format!("invalid {label} encoding"))?;
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("{label} has invalid length"))
}

fn random_message_id() -> String {
    let mut bytes = [0_u8; 16];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub fn random_file_id() -> String {
    format!("{}{}", now_unix(), random_ascii(12).to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypted_roundtrip_and_tamper_detection() {
        let a = DeviceIdentity::create("a".into());
        let b = DeviceIdentity::create("b".into());
        let am = a.manifest();
        let bm = b.manifest();
        let env = encrypt_chunk(
            &a,
            &bm,
            ChunkSpec {
                file_id: "file-1",
                chunk_index: 0,
                chunk_count: 1,
                original_name: "hello.txt",
                original_size: 5,
                data: b"hello",
                ttl_seconds: 60,
            },
        )
        .unwrap();
        let encoded = encode_envelope(&env).unwrap();
        let decoded = decode_envelope(&encoded).unwrap();
        let payload = decrypt_envelope(&b, &decoded, &[am]).unwrap();
        assert_eq!(payload_data(&payload), b"hello");

        let mut bad = decoded;
        bad.ciphertext[0] ^= 0x01;
        assert!(decrypt_envelope(&b, &bad, &[a.manifest()]).is_err());
    }
}
