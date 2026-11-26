use iroh::{
    SecretKey,
    address_lookup::{
        AddrFilter, AddressLookup, AddressLookupBuilder, AddressLookupBuilderError, EndpointData,
        EndpointInfo,
    },
};
use std::{fmt, time::Duration};
use tokio::{sync::watch, task::AbortHandle};

pub struct PublisherBuilder {
    pub url: url::Url,
    pub allow_fallback: bool,
}
impl fmt::Debug for PublisherBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DirectPkarrPublisherBuilder")
    }
}
struct Publisher {
    updates: watch::Sender<Option<EndpointData>>,
    task: AbortHandle,
}
impl fmt::Debug for Publisher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DirectPkarrPublisher")
    }
}
impl Drop for Publisher {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl AddressLookupBuilder for PublisherBuilder {
    fn into_address_lookup(
        self,
        endpoint: &iroh::Endpoint,
    ) -> Result<impl AddressLookup, AddressLookupBuilderError> {
        let client = client(&self.url)
            .map_err(|error| AddressLookupBuilderError::from_err("physical-pkarr", error))?;
        let secret = endpoint.secret_key().clone();
        let allow_fallback = self.allow_fallback;
        let (updates, mut receiver) = watch::channel::<Option<EndpointData>>(None);
        let task = tokio::spawn(async move {
            let mut timer = tokio::time::interval(Duration::from_secs(300));
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut failures = 0u32;
            loop {
                tokio::select! {
                    changed = receiver.changed() => { if changed.is_err() { break; } },
                    _ = timer.tick() => {},
                }
                let data = receiver.borrow_and_update().clone();
                let Some(data) = data else {
                    continue;
                };
                let result = publish(&client, &secret, data.clone()).await;
                let result = if let Err(ref error) = result
                    && allow_fallback
                    && !error
                        .downcast_ref::<reqwest::Error>()
                        .is_some_and(|error| error.is_status())
                {
                    eprintln!(
                        "network: route=system/proxy fallback; component=signed address publication; physical request failed"
                    );
                    match reqwest::Client::builder()
                        .https_only(true)
                        .timeout(Duration::from_secs(15))
                        .redirect(reqwest::redirect::Policy::none())
                        .build()
                    {
                        Ok(client) => publish(&client, &secret, data).await,
                        Err(error) => Err(error.into()),
                    }
                } else {
                    result
                };
                if result.is_err() {
                    failures = failures.saturating_add(1);
                    timer.reset_after(Duration::from_secs((5u64 << failures.min(6)).min(300)));
                    eprintln!(
                        "network: signed address publication failed; will retry on change or timer"
                    );
                } else {
                    failures = 0;
                    timer.reset_after(Duration::from_secs(300));
                }
            }
        });
        Ok(Publisher {
            updates,
            task: task.abort_handle(),
        })
    }
}
fn client(adapter: &url::Url) -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .no_proxy()
        .proxy(reqwest::Proxy::all(adapter.as_str())?)
        .https_only(true)
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
}

fn payload(secret: &SecretKey, data: EndpointData) -> anyhow::Result<Vec<u8>> {
    let info = EndpointInfo::from_parts(secret.public(), data);
    Ok(info.to_pkarr_signed_packet(secret, 30)?.to_relay_payload())
}
impl AddressLookup for Publisher {
    fn publish(&self, data: &EndpointData) {
        self.updates.send_replace(Some(
            data.apply_filter(&AddrFilter::relay_only()).into_owned(),
        ));
    }
}
async fn publish(
    client: &reqwest::Client,
    secret: &SecretKey,
    data: EndpointData,
) -> anyhow::Result<()> {
    let url = format!("https://dns.iroh.link/pkarr/{}", secret.public().to_z32());
    client
        .put(url)
        .body(payload(secret, data)?)
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn publication_preserves_signature_and_device_identity() {
        let secret = SecretKey::from_bytes(&rand::random());
        let data = EndpointData::from_iter([iroh::TransportAddr::Relay(
            "https://relay.example/".parse().unwrap(),
        )]);
        let expected = EndpointInfo::from_parts(secret.public(), data.clone());
        let payload = payload(&secret, data).unwrap();
        let signed =
            iroh_dns::pkarr::SignedPacket::from_relay_payload(&secret.public(), &payload).unwrap();
        assert_eq!(
            EndpointInfo::from_pkarr_signed_packet(&signed).unwrap(),
            expected
        );
        let other = SecretKey::from_bytes(&rand::random());
        assert!(
            iroh_dns::pkarr::SignedPacket::from_relay_payload(&other.public(), &payload).is_err()
        );
        let mut changed = payload.to_vec();
        *changed.last_mut().unwrap() ^= 1;
        assert!(
            iroh_dns::pkarr::SignedPacket::from_relay_payload(&secret.public(), &changed).is_err()
        );
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn publication_client_uses_authenticated_connector_and_requires_tls() {
        use super::super::{Physical, dns::Backend, proxy::Adapter};
        use std::{net::Ipv4Addr, sync::Arc};
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpListener,
        };
        let physical = Arc::new(Physical {
            name: "lo".into(),
            index: 1,
            ipv4: Some(Ipv4Addr::new(127, 0, 0, 2)),
            ipv6: None,
            dns_servers: vec![],
            allow_fallback: false,
        });
        let adapter = Adapter::start(physical.clone(), Arc::new(Backend::new(physical)))
            .await
            .unwrap();
        let client = client(&adapter.url).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let service = tokio::spawn(async move {
            let (mut stream, source) = listener.accept().await.unwrap();
            assert_eq!(source.ip(), Ipv4Addr::new(127, 0, 0, 2));
            assert_eq!(stream.read_u8().await.unwrap(), 0x16); // TLS handshake record
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .await
                .unwrap();
        });
        assert!(
            client
                .put(format!("https://{addr}/pkarr/key"))
                .send()
                .await
                .is_err()
        );
        service.await.unwrap();
        assert!(
            client
                .put(format!("http://{addr}/pkarr/key"))
                .send()
                .await
                .is_err()
        );
    }
}
