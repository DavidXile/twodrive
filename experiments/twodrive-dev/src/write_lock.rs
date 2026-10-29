//! Local accidental-write protection. This password is never a cloud password.
//! Each engine run invalidates previous permits; workers share only its token.
use crate::{
    profile::Profile,
    state::{State, random_secret},
};
use anyhow::{Context, ensure};
use ring::pbkdf2::{self, PBKDF2_HMAC_SHA256};
use serde::{Deserialize, Serialize};
use std::{
    num::NonZeroU32,
    path::{Path, PathBuf},
};
const FILE: &str = "write-password.secret";
const ITERATIONS: u32 = 600_000;
#[derive(Serialize, Deserialize)]
struct Password {
    salt: String,
    hash: Vec<u8>,
}
pub fn configured(state: &State) -> bool {
    state.dir.join(FILE).is_file()
}
pub fn set_password(state: &State, password: &str) -> anyhow::Result<()> {
    ensure!(!configured(state), "本机写入密码已设置");
    ensure!(
        (8..=256).contains(&password.len()),
        "写入密码至少 8 个字符，最长 256 字节"
    );
    let salt = random_secret();
    let mut hash = vec![0; 32];
    pbkdf2::derive(
        PBKDF2_HMAC_SHA256,
        NonZeroU32::new(ITERATIONS).unwrap(),
        salt.as_bytes(),
        password.as_bytes(),
        &mut hash,
    );
    twodrive_core::private_file::write_secret(
        &state.dir.join(FILE),
        &serde_json::to_vec(&Password { salt, hash })?,
    )?;
    Ok(())
}
pub fn verify(state: &State, password: &str) -> anyhow::Result<()> {
    ensure!(password.len() <= 256, "写入密码错误");
    let bytes = twodrive_core::private_file::read_secret(&state.dir.join(FILE))
        .context("请先设置本机写入密码")?;
    let saved: Password = serde_json::from_slice(&bytes)?;
    ensure!(
        pbkdf2::verify(
            PBKDF2_HMAC_SHA256,
            NonZeroU32::new(ITERATIONS).unwrap(),
            saved.salt.as_bytes(),
            password.as_bytes(),
            &saved.hash
        )
        .is_ok(),
        "写入密码错误"
    );
    Ok(())
}
pub fn begin_run(state: &State) -> anyhow::Result<()> {
    state.save("write-run.json", &random_secret())
}
pub fn permit_path(state: &State, p: &Profile) -> PathBuf {
    state.dir.join(format!("write-permit-{}.json", p.id))
}
pub fn permit(state: &State, p: &Profile) -> anyhow::Result<()> {
    let run: String = state.read("write-run.json")?;
    state.save(
        &format!("write-permit-{}.json", p.id),
        &format!("{run}:{}", p.revision),
    )
}
pub fn revoke(state: &State, p: &Profile) -> anyhow::Result<()> {
    let path = permit_path(state, p);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}
pub fn guard(state: &State, p: &Profile) -> anyhow::Result<(PathBuf, String)> {
    let run: String = state.read("write-run.json")?;
    Ok((permit_path(state, p), format!("{run}:{}", p.revision)))
}
pub fn allowed(path: &Path, token: &str) -> bool {
    crate::provider_guard::write_permit_allowed(path, token)
}
pub fn unlocked(state: &State, p: &Profile) -> bool {
    p.writable
        && configured(state)
        && guard(state, p).is_ok_and(|(path, token)| allowed(&path, &token))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn passwords_are_not_persisted_and_permits_are_account_revision_and_run_bound() {
        let root = tempfile::tempdir().unwrap();
        let state = State::new(root.path().join("private")).unwrap();
        begin_run(&state).unwrap();
        set_password(&state, "synthetic-only-password").unwrap();
        assert!(verify(&state, "wrong-password").is_err());
        verify(&state, "synthetic-only-password").unwrap();
        let mut p: Profile =
            serde_json::from_value(serde_json::json!({"id":"one","revision":"r1","writable":true}))
                .unwrap();
        assert!(!unlocked(&state, &p));
        permit(&state, &p).unwrap();
        assert!(unlocked(&state, &p));
        p.id = "two".into();
        assert!(!unlocked(&state, &p));
        p.id = "one".into();
        p.revision = "r2".into();
        assert!(!unlocked(&state, &p));
        p.revision = "r1".into();
        begin_run(&state).unwrap();
        assert!(!unlocked(&state, &p));
        let raw = twodrive_core::private_file::read_secret(&state.dir.join(FILE)).unwrap();
        assert!(
            !String::from_utf8(raw)
                .unwrap()
                .contains("synthetic-only-password")
        );
    }
}
