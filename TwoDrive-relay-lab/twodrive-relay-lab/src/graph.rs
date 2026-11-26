use crate::identity::DeviceManifest;
use crate::oauth::TokenManager;
use crate::util::{now_unix, safe_component};
use anyhow::{Context, bail};
use reqwest::{StatusCode, blocking::{Client, RequestBuilder, Response}};
use serde::{Deserialize, Serialize};
use std::thread::sleep;
use std::time::Duration;

const GRAPH: &str = "https://graph.microsoft.com/v1.0";

#[derive(Debug, Clone, Deserialize)]
pub struct GraphItem {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub size: Option<serde_json::Number>,
    #[serde(rename = "lastModifiedDateTime")]
    pub last_modified: Option<String>,
    pub folder: Option<serde_json::Value>,
    pub file: Option<serde_json::Value>,
}

impl GraphItem {
    pub fn size_u64(&self) -> u64 {
        self.size.as_ref().and_then(|n| n.as_u64()).unwrap_or(0)
    }
}


#[derive(Debug, Clone, Deserialize)]
pub struct DriveQuota {
    pub remaining: Option<u64>,
    pub total: Option<u64>,
    pub used: Option<u64>,
    pub deleted: Option<u64>,
    pub state: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DriveResource {
    quota: Option<DriveQuota>,
}

#[derive(Debug, Deserialize)]
struct ChildrenPage {
    value: Vec<GraphItem>,
    #[serde(rename = "@odata.nextLink")]
    next_link: Option<String>,
}

#[derive(Serialize)]
struct CreateFolder<'a> {
    name: &'a str,
    folder: serde_json::Value,
    #[serde(rename = "@microsoft.graph.conflictBehavior")]
    conflict_behavior: &'a str,
}

pub struct GraphClient<'a> {
    http: Client,
    tokens: &'a TokenManager,
}

impl<'a> GraphClient<'a> {
    pub fn new(tokens: &'a TokenManager) -> anyhow::Result<Self> {
        Ok(Self {
            http: Client::builder().timeout(Duration::from_secs(90)).build()?,
            tokens,
        })
    }

    pub fn app_root(&self) -> anyhow::Result<GraphItem> {
        self.send_json(|| {
            Ok(self.http.get(format!("{GRAPH}/me/drive/special/approot")).bearer_auth(self.tokens.access_token()?))
        })
    }


    pub fn drive_quota(&self) -> anyhow::Result<DriveQuota> {
        let drive: DriveResource = self.send_json(|| {
            Ok(self
                .http
                .get(format!("{GRAPH}/me/drive?$select=quota"))
                .bearer_auth(self.tokens.access_token()?))
        })?;
        drive.quota.context("Microsoft Graph did not return OneDrive quota information")
    }

    pub fn ensure_path(&self, root_id: &str, segments: &[&str]) -> anyhow::Result<GraphItem> {
        let mut current = GraphItem {
            id: root_id.to_string(), name: "approot".to_string(), size: None,
            last_modified: None, folder: Some(serde_json::json!({})), file: None,
        };
        for segment in segments {
            if !safe_component(segment) {
                bail!("unsafe Graph folder component: {segment}");
            }
            let children = self.list_children(&current.id)?;
            if let Some(found) = children.into_iter().find(|i| i.name == *segment && i.folder.is_some()) {
                current = found;
                continue;
            }
            current = self.create_folder(&current.id, segment)?;
        }
        Ok(current)
    }

    pub fn list_children(&self, parent_id: &str) -> anyhow::Result<Vec<GraphItem>> {
        let mut url = format!("{GRAPH}/me/drive/items/{parent_id}/children?$select=id,name,size,lastModifiedDateTime,file,folder");
        let mut out = Vec::new();
        loop {
            let page: ChildrenPage = self.send_json(|| {
                Ok(self.http.get(&url).bearer_auth(self.tokens.access_token()?))
            })?;
            out.extend(page.value);
            match page.next_link {
                Some(next) => url = next,
                None => break,
            }
        }
        Ok(out)
    }

    pub fn upload(&self, parent_id: &str, name: &str, bytes: &[u8]) -> anyhow::Result<GraphItem> {
        if !safe_component(name) {
            bail!("unsafe Graph file name: {name}");
        }
        let url = format!("{GRAPH}/me/drive/items/{parent_id}:/{name}:/content");
        self.send_json(|| {
            Ok(self.http.put(&url)
                .bearer_auth(self.tokens.access_token()?)
                .header("Content-Type", "application/octet-stream")
                .body(bytes.to_vec()))
        })
    }

    pub fn download(&self, item_id: &str) -> anyhow::Result<Vec<u8>> {
        let url = format!("{GRAPH}/me/drive/items/{item_id}/content");
        let response = self.send(|| Ok(self.http.get(&url).bearer_auth(self.tokens.access_token()?)))?;
        Ok(response.bytes()?.to_vec())
    }

    pub fn permanent_delete(&self, item_id: &str) -> anyhow::Result<()> {
        let url = format!("{GRAPH}/me/drive/items/{item_id}/permanentDelete");
        let mut delay = Duration::from_millis(500);
        for attempt in 0..5 {
            let response = self.http
                .post(&url)
                .bearer_auth(self.tokens.access_token()?)
                .send()?;
            let status = response.status();
            if status.is_success() || status == StatusCode::NOT_FOUND {
                return Ok(());
            }
            if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                if attempt == 4 {
                    let text = response.text().unwrap_or_default();
                    bail!("Graph permanentDelete failed after retries with HTTP {status}: {text}");
                }
                let retry = response.headers().get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .map(Duration::from_secs)
                    .unwrap_or(delay);
                sleep(retry.min(Duration::from_secs(30)));
                delay = (delay * 2).min(Duration::from_secs(8));
                continue;
            }
            let text = response.text().unwrap_or_default();
            bail!("Graph permanentDelete failed with HTTP {status}: {text}. The relay object was not moved to the recycle bin because recycle-bin items still consume OneDrive quota.");
        }
        unreachable!()
    }

    pub fn publish_manifest(&self, manifest: &DeviceManifest) -> anyhow::Result<()> {
        manifest.verify()?;
        let root = self.app_root()?;
        let devices = self.ensure_path(&root.id, &["control", "v1", "devices"])?;
        let name = format!("{}.json", manifest.device_id);
        self.upload(&devices.id, &name, &serde_json::to_vec_pretty(manifest)?)?;
        Ok(())
    }

    pub fn list_manifests(&self) -> anyhow::Result<Vec<DeviceManifest>> {
        let root = self.app_root()?;
        let devices = self.ensure_path(&root.id, &["control", "v1", "devices"])?;
        let mut out = Vec::new();
        for item in self.list_children(&devices.id)? {
            if item.file.is_none() || !item.name.ends_with(".json") { continue; }
            let bytes = match self.download(&item.id) { Ok(v) => v, Err(_) => continue };
            let manifest: DeviceManifest = match serde_json::from_slice(&bytes) { Ok(v) => v, Err(_) => continue };
            if manifest.verify().is_ok() {
                out.push(manifest);
            }
        }
        Ok(out)
    }

    fn create_folder(&self, parent_id: &str, name: &str) -> anyhow::Result<GraphItem> {
        let url = format!("{GRAPH}/me/drive/items/{parent_id}/children");
        let body = CreateFolder { name, folder: serde_json::json!({}), conflict_behavior: "fail" };
        let token = self.tokens.access_token()?;
        let response = self.http.post(&url).bearer_auth(token).json(&body).send()?;
        if response.status() == StatusCode::CONFLICT {
            return self.list_children(parent_id)?
                .into_iter()
                .find(|i| i.name == name && i.folder.is_some())
                .context("Graph reported folder conflict but existing folder was not found");
        }
        parse_json_response(response)
    }

    fn send_json<T: for<'de> Deserialize<'de>, F>(&self, build: F) -> anyhow::Result<T>
    where F: Fn() -> anyhow::Result<RequestBuilder> {
        parse_json_response(self.send(build)?)
    }

    fn send<F>(&self, build: F) -> anyhow::Result<Response>
    where F: Fn() -> anyhow::Result<RequestBuilder> {
        let mut delay = Duration::from_millis(500);
        for attempt in 0..5 {
            let response = build()?.send()?;
            if response.status().is_success() {
                return Ok(response);
            }
            if response.status() == StatusCode::TOO_MANY_REQUESTS || response.status().is_server_error() {
                if attempt == 4 {
                    let status = response.status();
                    let text = response.text().unwrap_or_default();
                    bail!("Graph request failed after retries with HTTP {status}: {text}");
                }
                let retry = response.headers().get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .map(Duration::from_secs)
                    .unwrap_or(delay);
                sleep(retry.min(Duration::from_secs(30)));
                delay = (delay * 2).min(Duration::from_secs(8));
                continue;
            }
            let status = response.status();
            let text = response.text().unwrap_or_default();
            bail!("Graph request failed with HTTP {status}: {text}");
        }
        unreachable!()
    }
}

fn parse_json_response<T: for<'de> Deserialize<'de>>(response: Response) -> anyhow::Result<T> {
    let status = response.status();
    let text = response.text()?;
    if !status.is_success() {
        bail!("Graph request failed with HTTP {status}: {text}");
    }
    serde_json::from_str(&text).with_context(|| format!("invalid Graph JSON response: {}", &text[..text.len().min(300)]))
}

pub fn relay_file_name(recipient_id: &str, expires_at: i64, message_id: &str) -> anyhow::Result<String> {
    if !safe_component(recipient_id) || !safe_component(message_id) {
        bail!("unsafe relay identifier");
    }
    Ok(format!("{recipient_id}__{expires_at}__{message_id}.tdr"))
}

pub fn parse_relay_file_name(name: &str) -> Option<(&str, i64, &str)> {
    let stem = name.strip_suffix(".tdr")?;
    let mut parts = stem.splitn(3, "__");
    let recipient = parts.next()?;
    let expires = parts.next()?.parse().ok()?;
    let message_id = parts.next()?;
    if !safe_component(recipient) || !safe_component(message_id) { return None; }
    Some((recipient, expires, message_id))
}

pub fn is_expired_relay_name(name: &str) -> bool {
    parse_relay_file_name(name).map(|(_, expiry, _)| expiry <= now_unix()).unwrap_or(false)
}
