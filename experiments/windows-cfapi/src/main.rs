#[cfg(windows)]
fn probe(root: &std::path::Path) -> Result<(), String> {
    use std::{
        fs,
        io::{Read, Seek, SeekFrom, Write},
        path::Path,
    };
    use twodrive_cfapi_probe::native::{Session, inspect, payload, pin, release};
    use twodrive_core::FileState;
    let mut session = Session::create(root)?;
    let result = (|| {
        let plain = session.placeholder(Path::new("generated.bin"))?;
        let chinese = session.placeholder(Path::new("中文测试文件.bin"))?;
        let long = session.placeholder(Path::new(&format!("{}.bin", "long-".repeat(48))))?;
        let before = session.calls();
        for path in [&plain, &chinese, &long] {
            let initial = inspect(path, FileState::OnlineOnly)?;
            require(
                !initial.resident && initial.os_in_sync,
                "online-only placeholder",
            )?;
            let _ = fs::metadata(path).map_err(|e| e.to_string())?;
        }
        require(session.calls() == before, "metadata must not hydrate")?;
        check("placeholder_metadata_no_hydration");

        // A normal ReadFile through std::fs, not a direct provider/mock call.
        for path in [&plain, &chinese, &long] {
            require(
                fs::read(path).map_err(|e| e.to_string())? == payload(),
                "native read bytes",
            )?;
            require(
                inspect(path, FileState::Cached)?.resident,
                "resident after read",
            )?;
        }
        require(session.calls() > before, "FETCH_DATA actually called")?;
        check("native_read_chinese_long_path");

        pin(&plain, true)?;
        require(inspect(&plain, FileState::Cached)?.pinned, "pin state")?;
        require(
            release(&plain, FileState::Cached).is_err(),
            "pinned release rejected",
        )?;
        pin(&plain, false)?;
        // A pin-attribute change can itself invalidate in-sync tracking. This
        // fixture deliberately does not mark any changed file back in sync.
        check("pin_intent_and_release_guard");

        release(&chinese, FileState::Cached)?;
        require(
            !inspect(&chinese, FileState::OnlineOnly)?.resident,
            "release removed local bytes",
        )?;
        check("clean_release");

        session.offline(true);
        require(fs::read(&chinese).is_err(), "offline read returns error")?;
        require(
            !inspect(&chinese, FileState::Error)?.resident,
            "offline failure not cached",
        )?;
        session.offline(false);
        require(
            fs::read(&chinese).map_err(|e| e.to_string())? == payload(),
            "offline recovery bytes",
        )?;
        check("offline_failure_and_retry");

        session.disconnect()?;
        require(
            fs::read(&long).map_err(|e| e.to_string())? == payload(),
            "disconnected cached read",
        )?;
        session.connect()?;
        require(
            inspect(&long, FileState::Cached)?.resident,
            "reconnect preserved resident state",
        )?;
        check("disconnect_reconnect_preserves_residency");

        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&long)
            .map_err(|e| e.to_string())?;
        file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        file.write_all(b"UNUPLOADED-GENERATED-DATA")
            .map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        // Also try a stale engine Cached label: OS must independently veto.
        drop(file);
        let dirty = inspect(&long, FileState::Dirty)?;
        require(
            !dirty.os_in_sync || dirty.modified_bytes > 0,
            "OS detects dirty data",
        )?;
        require(
            release(&long, FileState::Dirty).is_err(),
            "engine dirty release rejected",
        )?;
        require(
            release(&long, FileState::Cached).is_err(),
            "stale engine state release rejected",
        )?;
        let mut actual = Vec::new();
        fs::File::open(&long)
            .map_err(|e| e.to_string())?
            .read_to_end(&mut actual)
            .map_err(|e| e.to_string())?;
        let mut expected = payload();
        expected[..b"UNUPLOADED-GENERATED-DATA".len()]
            .copy_from_slice(b"UNUPLOADED-GENERATED-DATA");
        require(actual == expected, "entire dirty file preserved")?;
        check("dirty_release_refused_bytes_preserved");
        println!(
            "{}",
            serde_json::json!({"fetch_callbacks": session.calls(), "failed_callbacks": session.failures()})
        );
        Ok(())
    })();
    // Disconnect/unregister the generated root only. Keep all fixture files for
    // inspection; never recursively delete a test root in this executable.
    let cleanup = session.finish();
    cleanup?;
    check("generated_root_unregistered");
    result
}

#[cfg(windows)]
fn require(value: bool, name: &str) -> Result<(), String> {
    if value {
        Ok(())
    } else {
        Err(format!("assertion failed: {name}"))
    }
}
#[cfg(windows)]
fn check(name: &str) {
    println!("{}", serde_json::json!({"check":name,"result":"passed"}));
}

fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        #[cfg(windows)]
        match twodrive_cfapi_probe::native::platform() {
            Ok(p) => println!(
                "{}",
                serde_json::json!({"cfapi_platform_build":p.BuildNumber,"cfapi_integration":p.IntegrationNumber})
            ),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        println!(
            "{}",
            serde_json::json!({"version":env!("CARGO_PKG_VERSION"), "schema_version":1,
            "experimental":true,"production_mount":false,"cloud_writes":false,
            "native_cfapi":cfg!(windows),"registration_requires_explicit_opt_in":true,
            "usage":"--allow-isolated-registration NEW_ABSOLUTE_FIXTURE_ROOT"})
        );
        return;
    }
    #[cfg(windows)]
    if args.len() == 2 && args[0] == "--allow-isolated-registration" {
        let root = std::path::Path::new(&args[1]);
        if !root
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("twodrive-cfapi-fixture-"))
        {
            eprintln!("root leaf must begin twodrive-cfapi-fixture-");
            std::process::exit(2);
        }
        match probe(root) {
            Ok(()) => {
                println!(
                    "{}",
                    serde_json::json!({"passed":true,"explorer_visual_acceptance":"unverified","real_cloud":"not_tested"})
                );
                return;
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
    }
    eprintln!("unsupported arguments; run without arguments for capabilities");
    std::process::exit(2);
}
