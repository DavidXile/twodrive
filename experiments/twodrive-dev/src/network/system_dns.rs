//! Native address fallback only for the explicitly selected system/TUN route.
use futures_util::{FutureExt, future::BoxFuture};
use iroh::dns::{BoxIter, DnsError, DnsResolver, Resolver, TxtRecordData};
use n0_error::e;
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs},
    sync::Arc,
    time::Duration,
};

// Relay DNS has a three-second outer budget. Leave time for native fallback.
const PRIMARY_BUDGET: Duration = Duration::from_millis(500);
const BUDGET: Duration = Duration::from_secs(2);
type NativeLookup = fn(&str) -> std::io::Result<Vec<IpAddr>>;

#[derive(Clone, Debug)]
pub(super) struct SystemResolver {
    primary: DnsResolver,
    slots: Arc<tokio::sync::Semaphore>,
    native: NativeLookup,
    budget: Duration,
}

impl Default for SystemResolver {
    fn default() -> Self {
        Self {
            primary: DnsResolver::new(),
            slots: Arc::new(tokio::sync::Semaphore::new(8)),
            native: native_lookup,
            budget: BUDGET,
        }
    }
}

fn native_lookup(host: &str) -> std::io::Result<Vec<IpAddr>> {
    let mut values: Vec<_> = (host, 0).to_socket_addrs()?.map(|addr| addr.ip()).collect();
    values.sort_unstable();
    values.dedup();
    Ok(values)
}

impl SystemResolver {
    async fn addresses(&self, host: String, ipv4: bool) -> Result<Vec<IpAddr>, DnsError> {
        let primary = if ipv4 {
            self.primary
                .lookup_ipv4(&host, PRIMARY_BUDGET.min(self.budget))
                .await
                .map(|values| values.collect::<Vec<_>>())
        } else {
            self.primary
                .lookup_ipv6(&host, PRIMARY_BUDGET.min(self.budget))
                .await
                .map(|values| values.collect::<Vec<_>>())
        };
        if let Ok(values) = primary
            && !values.is_empty()
        {
            return Ok(values);
        }
        // A timed-out OS call retains its permit until it actually exits. Do
        // not use Tokio's blocking pool: runtime shutdown must remain bounded.
        let permit = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| e!(DnsError::NoResponse))?;
        let native = self.native;
        let (send, receive) = tokio::sync::oneshot::channel();
        std::thread::Builder::new()
            .name("td-system-dns".into())
            .spawn(move || {
                let _permit = permit;
                let _ = send.send(native(&host));
            })
            .map_err(|_| e!(DnsError::NoResponse))?;
        let values = tokio::time::timeout(self.budget, receive)
            .await
            .map_err(|_| e!(DnsError::Timeout))?
            .map_err(|_| e!(DnsError::NoResponse))?
            .map_err(|_| e!(DnsError::NoResponse))?;
        let values: Vec<_> = values
            .into_iter()
            .filter(|ip| ip.is_ipv4() == ipv4)
            .collect();
        // TUN fake-IP is meaningful only in this system-routing resolver.
        if !values.is_empty() {
            eprintln!("network: dns=system-native fallback; route=system/proxy");
        }
        Ok(values)
    }
}

impl Resolver for SystemResolver {
    fn lookup_ipv4(&self, host: String) -> BoxFuture<'static, Result<BoxIter<Ipv4Addr>, DnsError>> {
        let this = self.clone();
        async move {
            let values = this.addresses(host, true).await?;
            Ok(Box::new(values.into_iter().filter_map(|ip| match ip {
                IpAddr::V4(ip) => Some(ip),
                _ => None,
            })) as BoxIter<_>)
        }
        .boxed()
    }

    fn lookup_ipv6(&self, host: String) -> BoxFuture<'static, Result<BoxIter<Ipv6Addr>, DnsError>> {
        let this = self.clone();
        async move {
            let values = this.addresses(host, false).await?;
            Ok(Box::new(values.into_iter().filter_map(|ip| match ip {
                IpAddr::V6(ip) => Some(ip),
                _ => None,
            })) as BoxIter<_>)
        }
        .boxed()
    }

    fn lookup_txt(
        &self,
        host: String,
    ) -> BoxFuture<'static, Result<BoxIter<TxtRecordData>, DnsError>> {
        let this = self.clone();
        async move {
            let values = this.primary.lookup_txt(host, this.budget).await?;
            Ok(Box::new(values.collect::<Vec<_>>().into_iter()) as BoxIter<_>)
        }
        .boxed()
    }

    fn clear_cache(&self) {
        self.primary.clear_cache();
    }

    fn reset(&self) -> Box<dyn Resolver> {
        // Re-read raw DNS configuration; share the cap with detached old calls.
        Box::new(Self {
            primary: DnsResolver::new(),
            ..self.clone()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone)]
    struct Unavailable;
    impl Resolver for Unavailable {
        fn lookup_ipv4(
            &self,
            _: String,
        ) -> BoxFuture<'static, Result<BoxIter<Ipv4Addr>, DnsError>> {
            async { Err(e!(DnsError::NoResponse)) }.boxed()
        }
        fn lookup_ipv6(
            &self,
            _: String,
        ) -> BoxFuture<'static, Result<BoxIter<Ipv6Addr>, DnsError>> {
            async { Ok(Box::new(std::iter::empty()) as BoxIter<_>) }.boxed()
        }
        fn lookup_txt(
            &self,
            _: String,
        ) -> BoxFuture<'static, Result<BoxIter<TxtRecordData>, DnsError>> {
            async { Err(e!(DnsError::NoResponse)) }.boxed()
        }
        fn clear_cache(&self) {}
        fn reset(&self) -> Box<dyn Resolver> {
            Box::new(self.clone())
        }
    }

    #[derive(Debug, Clone)]
    struct Hanging;
    impl Resolver for Hanging {
        fn lookup_ipv4(
            &self,
            _: String,
        ) -> BoxFuture<'static, Result<BoxIter<Ipv4Addr>, DnsError>> {
            futures_util::future::pending().boxed()
        }
        fn lookup_ipv6(
            &self,
            host: String,
        ) -> BoxFuture<'static, Result<BoxIter<Ipv6Addr>, DnsError>> {
            Unavailable.lookup_ipv6(host)
        }
        fn lookup_txt(
            &self,
            host: String,
        ) -> BoxFuture<'static, Result<BoxIter<TxtRecordData>, DnsError>> {
            Unavailable.lookup_txt(host)
        }
        fn clear_cache(&self) {}
        fn reset(&self) -> Box<dyn Resolver> {
            Box::new(self.clone())
        }
    }

    #[tokio::test]
    async fn hanging_raw_dns_leaves_time_for_native_before_relay_deadline() {
        let primary = DnsResolver::custom(Hanging);
        // The old first-step budget cannot reach fallback within this window.
        assert!(
            tokio::time::timeout(
                Duration::from_secs(1),
                primary.lookup_ipv4("fixture.invalid", Duration::from_secs(3))
            )
            .await
            .is_err()
        );
        let resolver = DnsResolver::custom(SystemResolver {
            primary,
            native: |_| Ok(vec![IpAddr::V4(Ipv4Addr::LOCALHOST)]),
            ..Default::default()
        });
        let addresses = tokio::time::timeout(
            Duration::from_secs(1),
            resolver.lookup_ipv4("fixture.invalid", Duration::from_secs(3)),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            addresses.collect::<Vec<_>>(),
            vec![IpAddr::V4(Ipv4Addr::LOCALHOST)]
        );
    }

    #[tokio::test]
    async fn raw_failure_or_empty_uses_native_tun_address_only_in_system_resolver() {
        let primary = DnsResolver::custom(Unavailable);
        assert!(
            primary
                .lookup_ipv4("fixture.invalid", BUDGET)
                .await
                .is_err()
        );
        let resolver = SystemResolver {
            primary,
            native: |_| Ok(vec!["198.18.0.42".parse().unwrap(), "::1".parse().unwrap()]),
            ..Default::default()
        };
        let v4: Vec<_> = resolver
            .lookup_ipv4("fixture.invalid".into())
            .await
            .unwrap()
            .collect();
        assert_eq!(v4, vec![Ipv4Addr::new(198, 18, 0, 42)]);
        let v6: Vec<_> = resolver
            .lookup_ipv6("fixture.invalid".into())
            .await
            .unwrap()
            .collect();
        assert_eq!(v6, vec![Ipv6Addr::LOCALHOST]);
        assert!(resolver.lookup_txt("fixture.invalid".into()).await.is_err());
    }

    #[tokio::test]
    async fn native_hosts_lookup_works_when_raw_dns_fails() {
        let resolver = SystemResolver {
            primary: DnsResolver::custom(Unavailable),
            ..Default::default()
        };
        assert!(
            resolver
                .lookup_ipv4("localhost".into())
                .await
                .unwrap()
                .any(|ip| ip.is_loopback())
        );
    }

    #[tokio::test]
    async fn timed_out_native_call_keeps_slot_and_reset_cannot_bypass_limit() {
        let resolver = SystemResolver {
            primary: DnsResolver::custom(Unavailable),
            slots: Arc::new(tokio::sync::Semaphore::new(1)),
            native: |_| {
                std::thread::sleep(Duration::from_millis(200));
                Ok(vec![IpAddr::V4(Ipv4Addr::LOCALHOST)])
            },
            budget: Duration::from_millis(20),
        };
        assert!(matches!(
            resolver.lookup_ipv4("fixture.invalid".into()).await,
            Err(DnsError::Timeout { .. })
        ));
        assert_eq!(resolver.slots.available_permits(), 0);
        // reset shares the same concurrency cap even while old calls linger.
        let _replacement = resolver.reset();
        assert!(matches!(
            resolver.lookup_ipv4("fixture.invalid".into()).await,
            Err(DnsError::NoResponse { .. })
        ));
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert_eq!(resolver.slots.available_permits(), 1);
    }
}
