//! Per-account permissions. Credentials and identity fields never come from UI.
use super::*;
use twodrive_dev::write_lock;
impl App {
    pub(super) fn change_access(
        &mut self,
        a: &Value,
        cancel: &AtomicBool,
    ) -> anyhow::Result<Value> {
        let old = self.profile(string(a, "profile")?)?;
        ensure!(
            a["revision"].as_str() == Some(&old.revision),
            "连接配置已改变，请刷新后重试"
        );
        let mode = string(a, "mode")?;
        ensure!(
            matches!(mode, "readonly" | "editable" | "locked"),
            "无效的访问权限"
        );
        let descriptor = providers::descriptor(&old.kind)?;
        if mode == "editable" {
            ensure!(
                descriptor["capabilities"]["upload"] == true,
                "此网盘尚未支持编辑"
            );
            write_lock::verify(&self.state, a["password"].as_str().unwrap_or_default())?;
        }
        self.reap_jobs();
        ensure!(
            !self.jobs.values().any(|j| j.belongs_to(&old.id)),
            "此网盘仍有操作，请先完成或取消"
        );
        let online = self.online(&old.id);
        let mounted = if self.connected.as_ref().is_some_and(|p| p.id == old.id) {
            self.mounted.is_some()
        } else {
            self.inactive.get(&old.id).is_some_and(|p| p.is_mounted())
        };
        let selected = self.connected.as_ref().map(|p| p.id.clone());
        let unlocked = write_lock::unlocked(&self.state, &old);
        let saved = self.saved_session(&old);
        // Disconnect checks pending writes before revoking any permit. Another
        // account's connection, transfers and mount are never torn down.
        if online {
            self.disconnect_profile(&old.id)?;
        }
        let mut p = old.clone();
        p.revision = twodrive_dev::state::random_secret();
        p.writable = mode != "readonly";
        let session = saved.clone().map(|mut s| {
            s["revision"] = json!(p.revision);
            s["recipe"]["revision"] = json!(p.revision);
            s
        });
        let mut next = self.config.clone();
        *next.profiles.iter_mut().find(|x| x.id == p.id).unwrap() = p.clone();
        let outcome = (|| -> anyhow::Result<Option<(providers::Connection, Arc<Diagnostics>)>> {
            providers::validate(&p)?;
            if mode == "editable" {
                write_lock::permit(&self.state, &p)?;
            } else {
                write_lock::revoke(&self.state, &p)?;
            }
            let connection = if online {
                let network: Arc<Diagnostics> = Arc::default();
                Some((
                    providers::resume(
                        &p,
                        &self.args,
                        session.as_ref().context("登录不可恢复，请先手动连接")?,
                        cancel,
                        network.clone(),
                    )?,
                    network,
                ))
            } else {
                None
            };
            let published = connection
                .as_ref()
                .map(|(c, _)| providers::session(c, &p))
                .or_else(|| session.clone());
            if let Some(s) = &published {
                self.state.save(&format!("session-{}.json", p.id), s)?;
            }
            self.state.save("profiles.json", &next)?;
            Ok(connection)
        })();
        let connection = match outcome {
            Ok(c) => c,
            Err(error) => {
                if let Some(saved) = saved {
                    self.state
                        .save(&format!("session-{}.json", old.id), &saved)?;
                }
                self.state.save("profiles.json", &self.config)?;
                if unlocked {
                    write_lock::permit(&self.state, &old)?;
                } else {
                    write_lock::revoke(&self.state, &old)?;
                }
                if online
                    && self
                        .open_connection(&old.id, &AtomicBool::new(false), true)
                        .is_ok()
                    && mounted
                {
                    let _ = self.mount_current(&AtomicBool::new(false));
                }
                self.restore_access_view(selected.as_deref(), &old.id);
                return Err(error);
            }
        };
        self.config = next;
        if let Some((connection, network)) = connection {
            self.park_active();
            self.network = network;
            self.backend = Some(connection.backend);
            self.gateway = connection.gateway;
            self.connected = Some(p.clone());
            self.start_backup()?;
            let remount = if mounted {
                self.mount_current(cancel)
            } else {
                Ok(())
            };
            self.restore_access_view(selected.as_deref(), &p.id);
            remount?;
        }
        self.status()
    }
    fn restore_access_view(&mut self, selected: Option<&str>, target: &str) {
        if let Some(previous) = selected
            && previous != target
        {
            self.park_active();
            self.restore_active(previous);
        }
    }
}
