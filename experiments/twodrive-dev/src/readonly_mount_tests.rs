use super::*;
use crate::{
    model::DeltaResult,
    provider::{Capabilities, Progress, Provider},
};
use std::sync::atomic::{AtomicUsize, Ordering};
const NAME: &str = "你好 & <tag>.txt";
#[derive(Clone)]
struct Fixture {
    data: Arc<Mutex<(Vec<u8>, u64)>>,
    downloads: Arc<AtomicUsize>,
    change_during_read: Arc<AtomicBool>,
}
impl Fixture {
    fn new(bytes: &[u8]) -> Self {
        Self {
            data: Arc::new(Mutex::new((bytes.to_vec(), 1))),
            downloads: Arc::default(),
            change_during_read: Arc::default(),
        }
    }
    fn meta(&self) -> MetadataEntry {
        let data = self.data.lock().unwrap();
        MetadataEntry::new_file(
            "opaque-id-never-a-path",
            format!("/{NAME}"),
            data.0.len() as u64,
            1,
            data.1.to_string(),
        )
    }
    fn change(&self, bytes: &[u8]) {
        let mut data = self.data.lock().unwrap();
        data.0 = bytes.to_vec();
        data.1 += 1;
    }
}
impl CloudBackend for Fixture {
    fn is_conflict_error(&self, _: &anyhow::Error) -> bool {
        false
    }
    fn list_all(&self) -> anyhow::Result<Vec<MetadataEntry>> {
        panic!("whole-drive scans forbidden")
    }
    fn list_delta(&self, _: Option<&str>) -> anyhow::Result<DeltaResult> {
        panic!("delta not needed")
    }
    fn get_metadata(&self, id: &str) -> anyhow::Result<Option<MetadataEntry>> {
        ensure!(id == "opaque-id-never-a-path");
        Ok(Some(self.meta()))
    }
    fn get_metadata_by_path(&self, path: &str) -> anyhow::Result<Option<MetadataEntry>> {
        Ok(if path == "/" {
            Some(MetadataEntry::new_dir("root", "/", 1, "root"))
        } else if path == format!("/{NAME}") {
            Some(self.meta())
        } else {
            None
        })
    }
    fn download(&self, _: &str) -> anyhow::Result<Vec<u8>> {
        panic!("must stream")
    }
    fn download_to(
        &self,
        id: &str,
        writer: &mut dyn Write,
        progress: &mut dyn FnMut(u64) -> anyhow::Result<()>,
    ) -> anyhow::Result<u64> {
        ensure!(id == "opaque-id-never-a-path");
        self.downloads.fetch_add(1, Ordering::SeqCst);
        let data = self.data.lock().unwrap().0.clone();
        progress(0)?;
        writer.write_all(&data)?;
        if self.change_during_read.swap(false, Ordering::SeqCst) {
            self.change(b"changed while reading");
        }
        progress(data.len() as u64)?;
        Ok(data.len() as u64)
    }
    fn upload(&self, _: &str, _: Vec<u8>) -> anyhow::Result<MetadataEntry> {
        panic!("mount must never write")
    }
    fn upload_with_etag(
        &self,
        _: &str,
        _: Vec<u8>,
        _: Option<&str>,
    ) -> anyhow::Result<MetadataEntry> {
        panic!("mount must never write")
    }
    fn upload_file_with_version(
        &self,
        _: &str,
        _: &Path,
        _: Option<&str>,
        _: Option<&str>,
        _: &mut dyn FnMut(u64, u64) -> anyhow::Result<()>,
    ) -> anyhow::Result<MetadataEntry> {
        panic!("mount must never write")
    }
    fn create_folder(&self, _: &str) -> anyhow::Result<MetadataEntry> {
        panic!("mount must never write")
    }
    fn rename(&self, _: &str, _: &str) -> anyhow::Result<MetadataEntry> {
        panic!("mount must never write")
    }
    fn delete(&self, _: &str) -> anyhow::Result<()> {
        panic!("mount must never write")
    }
}
impl Provider for Fixture {
    fn storage_usage(
        &self,
        check: &mut dyn FnMut() -> anyhow::Result<()>,
    ) -> anyhow::Result<Option<crate::provider::StorageUsage>> {
        check()?;
        Ok(crate::provider::StorageUsage::account(
            Some(1000),
            Some(10),
            Some(990),
        ))
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            upload: true,
            conditional_upload: true,
            mkdir: true,
            rename: true,
            delete: true,
        }
    }
    fn list_directory(&self, path: &str) -> anyhow::Result<Vec<MetadataEntry>> {
        ensure!(path == "/");
        Ok(vec![self.meta()])
    }
    fn upload_controlled(
        &self,
        _: &str,
        _: &Path,
        _: Option<&str>,
        _: Option<&str>,
        _: Progress,
    ) -> anyhow::Result<MetadataEntry> {
        panic!("mount must never write")
    }
}
fn client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
}
fn file_url(bridge: &Bridge) -> String {
    format!(
        "http://{}/{}",
        bridge.address,
        percent_encoding::utf8_percent_encode(NAME, percent_encoding::NON_ALPHANUMERIC)
    )
}
fn request(
    c: &reqwest::blocking::Client,
    bridge: &Bridge,
    method: &str,
    path: &str,
) -> reqwest::blocking::RequestBuilder {
    c.request(
        reqwest::Method::from_bytes(method.as_bytes()).unwrap(),
        format!("http://{}{path}", bridge.address),
    )
    .bearer_auth(&bridge.token)
}
#[test]
fn loopback_auth_readonly_methods_depth_and_xml_escaping() {
    let root = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(b"0123456789");
    let bridge = Bridge::start(Backend::new(fixture.clone()), root.path()).unwrap();
    let c = client();
    assert!(bridge.address.ip().is_loopback());
    assert_eq!(c.get(file_url(&bridge)).send().unwrap().status(), 401);
    for method in [
        "PUT",
        "DELETE",
        "MOVE",
        "MKCOL",
        "COPY",
        "LOCK",
        "PROPPATCH",
        "POST",
    ] {
        assert_eq!(
            request(&c, &bridge, method, "/").send().unwrap().status(),
            403
        );
    }
    assert_eq!(
        request(&c, &bridge, "PROPFIND", "/")
            .header("Depth", "infinity")
            .send()
            .unwrap()
            .status(),
        403
    );
    let response = request(&c, &bridge, "PROPFIND", "/")
        .header("Depth", "1")
        .send()
        .unwrap();
    assert_eq!(response.status(), 207);
    let xml = response.text().unwrap();
    let document = roxmltree::Document::parse(&xml).unwrap();
    assert!(
        document
            .descendants()
            .filter(|n| n.has_tag_name(("DAV:", "href")))
            .any(|n| {
                percent_encoding::percent_decode_str(n.text().unwrap_or_default())
                    .decode_utf8()
                    .is_ok_and(|v| v == format!("/{NAME}"))
            })
    );
    assert!(!document.descendants().any(|n| n.has_tag_name("tag")));
    assert_eq!(fixture.downloads.load(Ordering::SeqCst), 0);
}
#[test]
fn independent_range_cursors_cache_versions_and_changed_download_rejection() {
    let root = tempfile::tempdir().unwrap();
    let fixture = Fixture::new(b"0123456789");
    let bridge = Bridge::start(Backend::new(fixture.clone()), root.path()).unwrap();
    let c = client();
    let head = c
        .head(file_url(&bridge))
        .bearer_auth(&bridge.token)
        .send()
        .unwrap();
    assert_eq!(head.headers()["Content-Length"], "10");
    let one = |range: &str| {
        let response = c
            .get(file_url(&bridge))
            .bearer_auth(&bridge.token)
            .header("Range", range)
            .send()
            .unwrap();
        assert_eq!(response.status(), 206);
        response.bytes().unwrap().to_vec()
    };
    thread::scope(|s| {
        let a = s.spawn(|| one("bytes=0-3"));
        let b = s.spawn(|| one("bytes=4-9"));
        assert_eq!(a.join().unwrap(), b"0123");
        assert_eq!(b.join().unwrap(), b"456789");
    });
    assert_eq!(fixture.downloads.load(Ordering::SeqCst), 1);
    fixture.change(b"new-version");
    assert_eq!(one("bytes=0-2"), b"new");
    assert_eq!(fixture.downloads.load(Ordering::SeqCst), 2);
    fixture.change(b"about-to-change");
    fixture.change_during_read.store(true, Ordering::SeqCst);
    let failed = c
        .get(file_url(&bridge))
        .bearer_auth(&bridge.token)
        .send()
        .unwrap();
    assert_eq!(failed.status(), 500);
    assert!(
        !failed
            .bytes()
            .unwrap()
            .windows(15)
            .any(|v| v == b"about-to-change")
    );
    assert_eq!(
        c.get(file_url(&bridge))
            .bearer_auth(&bridge.token)
            .send()
            .unwrap()
            .bytes()
            .unwrap()
            .as_ref(),
        b"changed while reading"
    );
}
#[test]
fn equal_paths_remain_scoped_to_their_provider_and_shutdown_closes_listener() {
    let root = tempfile::tempdir().unwrap();
    let a = Bridge::start(Backend::new(Fixture::new(b"account-a")), root.path()).unwrap();
    let b = Bridge::start(Backend::new(Fixture::new(b"account-b")), root.path()).unwrap();
    let c = client();
    for (bridge, expected) in [(&a, b"account-a"), (&b, b"account-b")] {
        assert_eq!(
            c.get(file_url(bridge))
                .bearer_auth(&bridge.token)
                .send()
                .unwrap()
                .bytes()
                .unwrap()
                .as_ref(),
            expected
        );
    }
    assert_eq!(
        c.get(file_url(&a))
            .bearer_auth(&b.token)
            .send()
            .unwrap()
            .status(),
        401
    );
    let address = a.address;
    drop(a);
    assert!(std::net::TcpStream::connect_timeout(&address, Duration::from_secs(1)).is_err());
}

#[test]
#[ignore = "requires the pinned official local Rclone executable; no cloud account"]
fn official_rclone_reads_the_private_provider_bridge() {
    let exe = std::env::var_os("TWODRIVE_TEST_RCLONE").expect("TWODRIVE_TEST_RCLONE required");
    let root = tempfile::tempdir().unwrap();
    let bridge = Bridge::start(Backend::new(Fixture::new(b"0123456789")), root.path()).unwrap();
    let run = |args: &[&str]| {
        let mut command = std::process::Command::new(&exe);
        for (key, _) in std::env::vars_os() {
            if key
                .to_string_lossy()
                .to_ascii_uppercase()
                .starts_with("RCLONE_")
            {
                command.env_remove(key);
            }
        }
        command
            .args(args)
            .arg(if cfg!(windows) {
                "--config=NUL"
            } else {
                "--config=/dev/null"
            })
            .args(["--ask-password=false", "--log-level=ERROR"])
            .env("RCLONE_CONFIG_TWODRIVE_TYPE", "webdav")
            .env(
                "RCLONE_CONFIG_TWODRIVE_URL",
                format!("http://{}/", bridge.address),
            )
            .env("RCLONE_CONFIG_TWODRIVE_BEARER_TOKEN", &bridge.token)
            .env("NO_PROXY", "127.0.0.1,localhost");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "official component failed with code {:?}",
            output.status.code()
        );
        output.stdout
    };
    let listing: serde_json::Value =
        serde_json::from_slice(&run(&["lsjson", "TWODRIVE:"])).unwrap();
    assert_eq!(listing[0]["Name"], NAME);
    let quota: serde_json::Value =
        serde_json::from_slice(&run(&["about", "TWODRIVE:", "--json"])).unwrap();
    assert_eq!(quota["used"], 10);
    assert_eq!(quota["total"], 1000);
    assert_eq!(
        run(&[
            "cat",
            &format!("TWODRIVE:{NAME}"),
            "--offset=3",
            "--count=4"
        ]),
        b"3456"
    );
}
