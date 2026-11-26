use crate::crypto::{RelayEnvelope, decode_envelope, encode_envelope};
use crate::graph::{
    GraphClient, GraphItem, is_expired_relay_name, parse_relay_file_name, relay_file_name,
};
use crate::util::now_unix;
use anyhow::{Context, bail};
use std::thread::sleep;
use std::time::{Duration, Instant};

pub const DEFAULT_ONEDRIVE_QUOTA: u64 = 1024 * 1024 * 1024; // 1 GiB hard cap
pub const DEFAULT_ONEDRIVE_RESERVE_FREE: u64 = 512 * 1024 * 1024; // preserve 512 MiB for the user

pub struct OneDriveRelay<'g, 't> {
    graph: &'g GraphClient<'t>,
    quota_bytes: u64,
    reserve_free_bytes: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct CleanupReport {
    pub before_bytes: u64,
    pub after_bytes: u64,
    pub deleted_items: usize,
}

impl<'g, 't> OneDriveRelay<'g, 't> {
    pub fn new(
        graph: &'g GraphClient<'t>,
        quota_bytes: u64,
        reserve_free_bytes: u64,
    ) -> anyhow::Result<Self> {
        if quota_bytes < 64 * 1024 * 1024 {
            bail!("OneDrive relay quota must be at least 64 MiB");
        }
        Ok(Self {
            graph,
            quota_bytes,
            reserve_free_bytes,
        })
    }

    /// Store one encrypted envelope while maintaining a hard active-cache cap.
    ///
    /// If the cache is full, wait for the receiver to consume/delete messages rather than
    /// evicting unreceived data. This turns the OneDrive folder into a bounded relay buffer:
    /// arbitrarily large files can pass through while the active cache remains capped.
    pub fn put_wait(&self, envelope: &RelayEnvelope, max_wait: Duration) -> anyhow::Result<()> {
        if envelope.expires_at_unix <= now_unix() {
            bail!("refusing to upload an already-expired relay envelope");
        }
        let bytes = encode_envelope(envelope)?;
        if bytes.len() as u64 > self.quota_bytes {
            bail!("one encrypted relay chunk is larger than the entire configured OneDrive relay quota");
        }
        let folder = self.message_folder()?;
        let started = Instant::now();
        let mut wait = Duration::from_secs(3);
        loop {
            let mut items = self.graph.list_children(&folder.id)?;
            self.delete_expired(&mut items)?;
            items = self.graph.list_children(&folder.id)?;
            let used: u64 = items
                .iter()
                .filter(|i| i.file.is_some())
                .map(|i| i.size_u64())
                .sum();
            let cache_has_space = used.saturating_add(bytes.len() as u64) <= self.quota_bytes;
            let drive_has_space = self.drive_has_space_for(bytes.len() as u64)?;
            if cache_has_space && drive_has_space {
                let name = relay_file_name(
                    &envelope.recipient_id,
                    envelope.expires_at_unix,
                    &envelope.message_id,
                )?;
                let uploaded = self.graph.upload(&folder.id, &name, &bytes)?;
                let after: u64 = self
                    .graph
                    .list_children(&folder.id)?
                    .iter()
                    .filter(|i| i.file.is_some())
                    .map(|i| i.size_u64())
                    .sum();
                let drive_still_safe = self.drive_has_space_for(0)?;
                if after <= self.quota_bytes && drive_still_safe {
                    return Ok(());
                }
                // A concurrent sender or another OneDrive client raced the preflight. Roll back
                // our item, wait for space, then retry; never knowingly cross either hard guard.
                self.graph.permanent_delete(&uploaded.id)?;
            }

            if started.elapsed() >= max_wait {
                bail!(
                    "OneDrive relay could not obtain safe space for {:?}; receiver must drain pending data or the OneDrive account must regain free space (relay_limit={} bytes, reserve_free={} bytes)",
                    max_wait,
                    self.quota_bytes,
                    self.reserve_free_bytes
                );
            }
            sleep(wait);
            wait = (wait * 2).min(Duration::from_secs(30));
        }
    }


    fn drive_has_space_for(&self, additional_bytes: u64) -> anyhow::Result<bool> {
        if self.reserve_free_bytes == 0 {
            return Ok(true);
        }
        let quota = self.graph.drive_quota()?;
        let remaining = quota.remaining.ok_or_else(|| {
            anyhow::anyhow!(
                "OneDrive quota response did not include `remaining`; refusing relay upload while --reserve-free is non-zero"
            )
        })?;
        Ok(remaining >= self.reserve_free_bytes.saturating_add(additional_bytes))
    }

    pub fn pending_for(&self, recipient_id: &str) -> anyhow::Result<Vec<GraphItem>> {
        let folder = self.message_folder()?;
        let mut items = self.graph.list_children(&folder.id)?;
        self.delete_expired(&mut items)?;
        let mut filtered: Vec<_> = self
            .graph
            .list_children(&folder.id)?
            .into_iter()
            .filter(|i| i.file.is_some())
            .filter(|i| {
                parse_relay_file_name(&i.name)
                    .map(|(r, _, _)| r == recipient_id)
                    .unwrap_or(false)
            })
            .collect();
        filtered.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(filtered)
    }

    pub fn download_envelope(&self, item: &GraphItem) -> anyhow::Result<RelayEnvelope> {
        let bytes = self.graph.download(&item.id)?;
        decode_envelope(&bytes).context("invalid OneDrive relay envelope")
    }

    pub fn ack_delete(&self, item: &GraphItem) -> anyhow::Result<()> {
        self.graph.permanent_delete(&item.id)
    }

    pub fn cleanup(&self) -> anyhow::Result<CleanupReport> {
        let folder = self.message_folder()?;
        let mut items = self.graph.list_children(&folder.id)?;
        let before: u64 = items
            .iter()
            .filter(|i| i.file.is_some())
            .map(|i| i.size_u64())
            .sum();
        let deleted = self.delete_expired(&mut items)?;
        let after_items = self.graph.list_children(&folder.id)?;
        let after: u64 = after_items
            .iter()
            .filter(|i| i.file.is_some())
            .map(|i| i.size_u64())
            .sum();
        Ok(CleanupReport {
            before_bytes: before,
            after_bytes: after,
            deleted_items: deleted,
        })
    }

    pub fn usage(&self) -> anyhow::Result<u64> {
        let folder = self.message_folder()?;
        Ok(self
            .graph
            .list_children(&folder.id)?
            .into_iter()
            .filter(|i| i.file.is_some())
            .map(|i| i.size_u64())
            .sum())
    }

    pub fn purge_all(&self) -> anyhow::Result<usize> {
        let folder = self.message_folder()?;
        let mut deleted = 0;
        for item in self.graph.list_children(&folder.id)? {
            if item.file.is_some() && parse_relay_file_name(&item.name).is_some() {
                self.graph.permanent_delete(&item.id)?;
                deleted += 1;
            }
        }
        Ok(deleted)
    }

    fn message_folder(&self) -> anyhow::Result<GraphItem> {
        let root = self.graph.app_root()?;
        self.graph
            .ensure_path(&root.id, &["relay", "v1", "messages"])
    }

    fn delete_expired(&self, items: &mut [GraphItem]) -> anyhow::Result<usize> {
        let mut deleted = 0;
        for item in items.iter() {
            if item.file.is_some() && is_expired_relay_name(&item.name) {
                self.graph.permanent_delete(&item.id)?;
                deleted += 1;
            }
        }
        Ok(deleted)
    }
}
