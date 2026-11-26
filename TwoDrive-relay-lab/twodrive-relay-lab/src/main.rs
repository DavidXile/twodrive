mod crypto;
mod graph;
mod identity;
mod oauth;
mod onedrive;
mod paths;
mod transfer;
mod util;
mod vps;

use anyhow::bail;
use clap::{Parser, Subcommand};
use crypto::{decode_envelope, encode_envelope};
use graph::GraphClient;
use identity::{DeviceIdentity, DeviceManifest};
use oauth::{DEFAULT_CLIENT_ID, TokenManager};
use onedrive::OneDriveRelay;
use paths::LabPaths;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;
use transfer::{accept_envelope, stream_file_envelopes};
use util::{parse_bytes, parse_duration};
use vps::{VpsRelayClient, VpsServerConfig};

#[derive(Parser)]
#[command(name = "twodrive-relay-lab", version, about = "Experimental encrypted OneDrive/VPS relay for TwoDrive")]
struct Cli {
    #[arg(long, default_value = DEFAULT_CLIENT_ID, global = true)]
    client_id: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Sign in to Microsoft using the same public TwoDrive application ID and OAuth PKCE flow.
    Login,
    /// Create/update this device identity and publish its signed public manifest to the OneDrive app folder.
    Init { #[arg(long)] name: String },
    /// List valid same-account device manifests discovered through OneDrive.
    Peers,
    /// Send a file through the encrypted OneDrive mailbox.
    OnedriveSend {
        #[arg(long)] to: String,
        #[arg(long)] file: PathBuf,
        #[arg(long, default_value = "1GiB")] quota: String,
        /// Minimum free space to preserve for the user's whole OneDrive. Use 0 only to disable this safety guard.
        #[arg(long, default_value = "512MiB")] reserve_free: String,
        #[arg(long, default_value = "24h")] ttl: String,
        /// Maximum time to wait for bounded relay-cache space while the receiver drains it.
        #[arg(long, default_value = "30m")] cache_wait: String,
    },
    /// Receive all currently pending OneDrive relay chunks for this device, then delete each verified chunk remotely.
    OnedriveRecv {
        #[arg(long)] out: PathBuf,
        #[arg(long, default_value = "1GiB")] quota: String,
        #[arg(long)] name: Option<String>,
        /// Keep polling for new relay chunks; intended for a backup/storage machine.
        #[arg(long)] watch: bool,
        #[arg(long, default_value = "5s")] poll: String,
    },
    /// Remove expired OneDrive relay data and report cache usage.
    OnedriveClean {
        #[arg(long, default_value = "1GiB")] quota: String,
        /// Explicitly delete every relay message, including unexpired/unreceived ciphertext.
        #[arg(long)] purge_all: bool,
    },
    /// Run the opaque VPS mailbox relay. The VPS never receives file-decryption keys.
    VpsServe {
        #[arg(long, default_value = "0.0.0.0:4433")] listen: String,
        #[arg(long)] data_dir: PathBuf,
        #[arg(long, env = "TWODRIVE_RELAY_TOKEN")] token: String,
        #[arg(long, default_value = "4GiB")] quota: String,
        #[arg(long, default_value = "8MiB")] max_message: String,
    },
    /// Send a file through a VPS relay. Peer identity is still discovered/pinned via the same OneDrive account in this experiment.
    VpsSend {
        #[arg(long)] relay: String,
        #[arg(long, env = "TWODRIVE_RELAY_TOKEN")] token: String,
        #[arg(long)] to: String,
        #[arg(long)] file: PathBuf,
        #[arg(long, default_value = "24h")] ttl: String,
        #[arg(long, default_value = "30m")] cache_wait: String,
    },
    /// Receive pending VPS relay messages, verify/decrypt them, then ACK deletion.
    VpsRecv {
        #[arg(long)] relay: String,
        #[arg(long, env = "TWODRIVE_RELAY_TOKEN")] token: String,
        #[arg(long)] out: PathBuf,
        #[arg(long)] name: Option<String>,
        #[arg(long)] watch: bool,
        #[arg(long, default_value = "2s")] poll: String,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let paths = LabPaths::discover()?;
    paths.ensure()?;
    let tokens = TokenManager::new(&paths.token_path, cli.client_id)?;

    match cli.command {
        Command::Login => tokens.login(),
        Command::Init { name } => {
            let identity = DeviceIdentity::load_or_create(&paths.identity_path, Some(&name))?;
            let graph = GraphClient::new(&tokens)?;
            graph.publish_manifest(&identity.manifest())?;
            println!("Device initialized and published:\n  name: {}\n  id: {}", identity.device_name, identity.device_id());
            Ok(())
        }
        Command::Peers => {
            let identity = DeviceIdentity::load_or_create(&paths.identity_path, None)?;
            let graph = GraphClient::new(&tokens)?;
            let peers = graph.list_manifests()?;
            for peer in peers.iter().filter(|p| p.device_id != identity.device_id()) {
                println!("{}  {}", peer.device_id, peer.device_name);
            }
            Ok(())
        }
        Command::OnedriveSend { to, file, quota, reserve_free, ttl, cache_wait } => {
            let identity = DeviceIdentity::load_or_create(&paths.identity_path, None)?;
            let graph = GraphClient::new(&tokens)?;
            graph.publish_manifest(&identity.manifest())?;
            let manifests = graph.list_manifests()?;
            let peer = resolve_peer(&manifests, &identity.device_id(), &to)?;
            let quota = parse_bytes(&quota)?;
            let reserve_free = parse_bytes(&reserve_free)?;
            let ttl = parse_duration(&ttl)?;
            enforce_ttl(ttl)?;
            let cache_wait = duration_from_arg(&cache_wait)?;
            let relay = OneDriveRelay::new(&graph, quota, reserve_free)?;
            println!(
                "Streaming encrypted chunks to {} ({}) via OneDrive relay (cache cap={} bytes, preserving >= {} bytes free)...",
                peer.device_name, peer.device_id, quota, reserve_free
            );
            let chunks = stream_file_envelopes(&identity, &peer, &file, ttl, |idx, total, envelope| {
                relay.put_wait(envelope, cache_wait)?;
                println!("  uploaded encrypted chunk {}/{}", idx + 1, total);
                Ok(())
            })?;
            println!("Done: {chunks} chunk(s). OneDrive relay cache usage: {} bytes / {} bytes.", relay.usage()?, quota);
            Ok(())
        }
        Command::OnedriveRecv { out, quota, name, watch, poll } => {
            let identity = DeviceIdentity::load_or_create(&paths.identity_path, name.as_deref())?;
            let graph = GraphClient::new(&tokens)?;
            graph.publish_manifest(&identity.manifest())?;
            let relay = OneDriveRelay::new(&graph, parse_bytes(&quota)?, 0)?;
            let poll = duration_from_arg(&poll)?;
            loop {
                let trusted = graph.list_manifests()?;
                let pending = relay.pending_for(&identity.device_id())?;
                if pending.is_empty() {
                    if watch {
                        sleep(poll);
                        continue;
                    }
                    println!("No pending OneDrive relay messages.");
                    break;
                }
                for item in pending {
                    let envelope = relay.download_envelope(&item)?;
                    match accept_envelope(&identity, &trusted, &envelope, &paths.inbox_dir, &out) {
                        Ok(completed) => {
                            relay.ack_delete(&item)?;
                            if let Some(path) = completed {
                                println!("Received complete file: {}", path.display());
                            }
                        }
                        Err(err) => eprintln!(
                            "Keeping remote item {} because verification/decryption failed: {err:#}",
                            item.name
                        ),
                    }
                }
                if !watch {
                    break;
                }
            }
            Ok(())
        }
        Command::OnedriveClean { quota, purge_all } => {
            let graph = GraphClient::new(&tokens)?;
            let relay = OneDriveRelay::new(&graph, parse_bytes(&quota)?, 0)?;
            if purge_all {
                let deleted = relay.purge_all()?;
                println!("OneDrive relay purge: permanently deleted {deleted} relay item(s).");
            } else {
                let report = relay.cleanup()?;
                println!("OneDrive relay cleanup: deleted {} expired item(s), {} -> {} bytes.", report.deleted_items, report.before_bytes, report.after_bytes);
            }
            Ok(())
        }
        Command::VpsServe { listen, data_dir, token, quota, max_message } => {
            vps::serve(VpsServerConfig {
                listen,
                data_dir,
                token,
                quota_bytes: parse_bytes(&quota)?,
                max_message_bytes: parse_bytes(&max_message)?,
            })
        }
        Command::VpsSend { relay, token, to, file, ttl, cache_wait } => {
            let identity = DeviceIdentity::load_or_create(&paths.identity_path, None)?;
            let graph = GraphClient::new(&tokens)?;
            graph.publish_manifest(&identity.manifest())?;
            let trusted = graph.list_manifests()?;
            let peer = resolve_peer(&trusted, &identity.device_id(), &to)?;
            let ttl = parse_duration(&ttl)?;
            enforce_ttl(ttl)?;
            let cache_wait = duration_from_arg(&cache_wait)?;
            let client = VpsRelayClient::new(relay, token)?;
            let chunks = stream_file_envelopes(&identity, &peer, &file, ttl, |idx, total, envelope| {
                let body = encode_envelope(envelope)?;
                client.put_wait(&envelope.recipient_id, &envelope.message_id, envelope.expires_at_unix, &body, cache_wait)?;
                println!("  sent encrypted VPS chunk {}/{}", idx + 1, total);
                Ok(())
            })?;
            println!("Done: {chunks} chunk(s).");
            Ok(())
        }
        Command::VpsRecv { relay, token, out, name, watch, poll } => {
            let identity = DeviceIdentity::load_or_create(&paths.identity_path, name.as_deref())?;
            let graph = GraphClient::new(&tokens)?;
            graph.publish_manifest(&identity.manifest())?;
            let client = VpsRelayClient::new(relay, token)?;
            let poll = duration_from_arg(&poll)?;
            let mut received = 0;
            loop {
                let trusted = graph.list_manifests()?;
                let Some((message_id, _expires, body)) = client.get(&identity.device_id())? else {
                    if watch {
                        sleep(poll);
                        continue;
                    }
                    break;
                };
                let envelope = match decode_envelope(&body) {
                    Ok(v) => v,
                    Err(err) => {
                        eprintln!("Invalid VPS envelope {message_id}: {err}");
                        break;
                    }
                };
                if envelope.message_id != message_id {
                    eprintln!("VPS mailbox id does not match encrypted envelope id; refusing to ACK");
                    break;
                }
                match accept_envelope(&identity, &trusted, &envelope, &paths.inbox_dir, &out) {
                    Ok(completed) => {
                        client.ack(&identity.device_id(), &message_id)?;
                        received += 1;
                        if let Some(path) = completed {
                            println!("Received complete file: {}", path.display());
                        }
                    }
                    Err(err) => {
                        eprintln!(
                            "Not ACKing VPS message {message_id} because verification/decryption failed: {err:#}"
                        );
                        break;
                    }
                }
            }
            println!("Processed {received} VPS relay message(s).");
            Ok(())
        }
    }
}

fn duration_from_arg(value: &str) -> anyhow::Result<Duration> {
    let seconds = parse_duration(value)?;
    if seconds < 0 {
        bail!("duration must be non-negative");
    }
    Ok(Duration::from_secs(seconds as u64))
}

fn resolve_peer(manifests: &[DeviceManifest], self_id: &str, selector: &str) -> anyhow::Result<DeviceManifest> {
    let peers: Vec<_> = manifests.iter().filter(|m| m.device_id != self_id).cloned().collect();
    if selector.eq_ignore_ascii_case("auto") {
        return match peers.as_slice() {
            [one] => Ok(one.clone()),
            [] => bail!("no other same-account TwoDrive Relay Lab device is published"),
            _ => bail!("more than one peer is available; pass --to with a device id prefix or exact device name"),
        };
    }
    let selector_lower = selector.to_ascii_lowercase();
    let matches: Vec<_> = peers.into_iter().filter(|p| {
        p.device_id.starts_with(&selector_lower) || p.device_name.eq_ignore_ascii_case(selector)
    }).collect();
    match matches.as_slice() {
        [one] => Ok(one.clone()),
        [] => bail!("peer `{selector}` was not found in the same OneDrive account"),
        _ => bail!("peer selector `{selector}` is ambiguous"),
    }
}

fn enforce_ttl(ttl: i64) -> anyhow::Result<()> {
    if ttl < 5 * 60 { bail!("relay TTL must be at least 5 minutes"); }
    if ttl > 7 * 24 * 60 * 60 { bail!("relay TTL must not exceed 7 days"); }
    Ok(())
}
