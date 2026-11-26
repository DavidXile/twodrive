use iroh::{PublicKey, SecretKey};
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use twodrive_dev::{
    state::{State, now},
    test_agent::{Report, Signed},
};
const BIN: &str = env!("CARGO_BIN_EXE_twodrive-dev");
struct Guard(Child);
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn report(path: &Path, device: PublicKey) -> Option<Report> {
    let value: Signed<Report> = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    let mut bytes = b"twodrive/test-agent/report/1\0".to_vec();
    bytes.extend(serde_json::to_vec(&value.payload).ok()?);
    device.verify(&bytes, &value.signature).ok()?;
    Some(value.payload)
}
fn wait(queue: &Path, device: PublicKey, seq: u64) -> Report {
    let deadline = Instant::now() + Duration::from_secs(35);
    loop {
        if let Some(r) = report(&queue.join("response.json"), device)
            && r.seq == seq
        {
            return r;
        }
        assert!(Instant::now() < deadline, "control response deadline");
        thread::sleep(Duration::from_millis(100));
    }
}
fn job(state: &State, queue: &Path, session: &str, seq: u64, action: Value) -> Report {
    let input = state.dir.join(format!("input-{seq}.json"));
    let output = state.dir.join(format!("signed-{seq}.json"));
    fs::write(
        &input,
        serde_json::to_vec(
            &json!({"version":1,"session":session,"seq":seq,"expires":now()+60,"command":action}),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(
        Command::new(BIN)
            .arg("--state")
            .arg(&state.dir)
            .args(["test-sign", "--input"])
            .arg(&input)
            .arg("--out")
            .arg(&output)
            .status()
            .unwrap()
            .success()
    );
    let _ = fs::remove_file(queue.join("command.json"));
    fs::rename(output, queue.join("command.json")).unwrap();
    let device: PublicKey = state.read("test-device.json").unwrap();
    wait(queue, device, seq)
}
#[test]
fn paired_control_recovers_child_failure_rejects_replay_and_cleans_up() {
    let temp = tempfile::tempdir().unwrap();
    let queue = temp
        .path()
        .join("td-control-00000000000000000000000000000000");
    fs::create_dir(&queue).unwrap();
    let runtime = temp.path().join("temp");
    fs::create_dir(&runtime).unwrap();
    let server = State::new(temp.path().join("server")).unwrap();
    let device = server.identity().unwrap().public();
    let owner = State::new(temp.path().join("owner")).unwrap();
    let key = owner.identity().unwrap();
    server
        .control(|c| {
            c.peers.insert(key.public().to_string());
            Ok(())
        })
        .unwrap();
    owner.save("test-device.json", &device).unwrap();
    let identity_before = fs::read(server.dir.join("identity.json")).unwrap();
    let grants_before = fs::read(server.dir.join("control.json")).unwrap();
    #[cfg(not(windows))]
    let mut command = {
        let mut c = Command::new(BIN);
        c.arg("--state")
            .arg(&server.dir)
            .arg("test-agent")
            .arg("--queue")
            .arg(&queue)
            .arg("--owner")
            .arg(key.public().to_string())
            .args(["--lifetime", "120"]);
        c
    };
    #[cfg(windows)]
    let mut command = {
        use sha2::{Digest, Sha256};
        let payload = queue.join("twodrive-dev.exe");
        fs::copy(BIN, &payload).unwrap();
        let sha = format!("{:x}", Sha256::digest(fs::read(&payload).unwrap()));
        let script = queue.join("start-test-agent.ps1");
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/start-test-agent.ps1"),
            &script,
        )
        .unwrap();
        let mut c = Command::new("powershell.exe");
        c.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(script)
            .arg("-State")
            .arg(&server.dir)
            .arg("-Owner")
            .arg(key.public().to_string())
            .arg("-ExpectedSha256")
            .arg(sha)
            .args(["-Lifetime", "120"]);
        c
    };
    let mut parent = Guard(
        command
            .env("TMPDIR", &runtime)
            .env("TEMP", &runtime)
            .env("TMP", &runtime)
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let ready = wait(&queue, device, 0);
    assert_eq!(ready.status, "ready");
    assert_eq!(
        ready.expires,
        ready.data["lease_expires"].as_u64().unwrap() + 300
    );
    // Exercise the actual CLI report verifier, not only the library signature.
    assert!(
        Command::new(BIN)
            .arg("--state")
            .arg(&owner.dir)
            .args(["test-verify", "--input"])
            .arg(queue.join("response.json"))
            .arg("--device")
            .arg(device.to_string())
            .arg("--out")
            .arg(owner.dir.join("verified.json"))
            .status()
            .unwrap()
            .success()
    );
    let session = ready.session;
    let start = |route, missing| json!({"action":"start","client":key.public(),"candidate_sha256":null,"route":route,"missing_interface":missing,"no_relay":true,"relay_only":false,"relays":[]});
    assert_eq!(
        job(&owner, &queue, &session, 1, start("direct", true)).status,
        "ok"
    );
    let replay = fs::read(queue.join("command.json")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut seq = 1;
    loop {
        seq += 1;
        let r = job(&owner, &queue, &session, seq, json!({"action":"snapshot"}));
        if r.data["exited"] == true {
            assert_eq!(r.data["exit_code"], 1);
            assert!(r.data["stderr"].as_str().unwrap().contains("Error:"));
            break;
        }
        assert!(Instant::now() < deadline);
    }
    // The controller remains responsive after a real serve startup failure.
    seq += 1;
    assert_eq!(
        job(&owner, &queue, &session, seq, start("system", false)).status,
        "ok"
    );
    loop {
        seq += 1;
        let r = job(&owner, &queue, &session, seq, json!({"action":"snapshot"}));
        if r.data["ready"] == true {
            assert!(r.data["address"]["id"].is_string());
            break;
        }
        assert!(Instant::now() < deadline);
    }
    // Replayed authenticated commands must not execute or update the response.
    let response = fs::read(queue.join("response.json")).unwrap();
    fs::write(queue.join("command.json"), replay).unwrap();
    thread::sleep(Duration::from_millis(400));
    assert_eq!(fs::read(queue.join("response.json")).unwrap(), response);
    seq += 1;
    let done = job(&owner, &queue, &session, seq, json!({"action":"finish"}));
    assert_eq!(done.status, "finished");
    assert_eq!(done.data["temporary_child_removed"], true);
    let deadline = Instant::now() + Duration::from_secs(10);
    while parent.0.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(100));
    }
    assert!(parent.0.wait().unwrap().success());
    #[cfg(windows)]
    {
        let receipt: Value =
            serde_json::from_slice(&fs::read(queue.join("bootstrap-cleanup.json")).unwrap())
                .unwrap();
        assert_eq!(receipt["agent_exit_code"], 0);
        for field in [
            "identity_and_grants_preserved",
            "bootstrap_runtime_removed",
            "executable_payload_removed",
        ] {
            assert_eq!(receipt[field], true);
        }
        assert!(!queue.join("twodrive-dev.exe").exists());
    }
    assert_eq!(fs::read_dir(&runtime).unwrap().count(), 0);
    assert_eq!(
        fs::read(server.dir.join("identity.json")).unwrap(),
        identity_before
    );
    assert_eq!(
        fs::read(server.dir.join("control.json")).unwrap(),
        grants_before
    );
}
#[test]
fn unauthorized_owner_cannot_start_agent() {
    let temp = tempfile::tempdir().unwrap();
    let state = State::new(temp.path().join("state")).unwrap();
    let queue = temp.path().join("queue");
    fs::create_dir(&queue).unwrap();
    let result = Command::new(BIN)
        .arg("--state")
        .arg(&state.dir)
        .arg("test-agent")
        .arg("--queue")
        .arg(&queue)
        .arg("--owner")
        .arg(SecretKey::generate().public().to_string())
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("not an authorized paired device"));
    assert!(!queue.join("response.json").exists());
}
