//! Process-scoped Windows static HTTP proxy fallback; no configuration writes.
use iroh::endpoint::Builder;

pub(super) fn configure(builder: Builder, _relays: &[String]) -> Builder {
    let environment = environment_proxy(
        ["HTTP_PROXY", "http_proxy", "HTTPS_PROXY", "https_proxy"]
            .map(|name| std::env::var(name).ok()),
        std::env::var_os("REQUEST_METHOD").is_some(),
    );
    if let Some(proxy) = environment {
        eprintln!("network: proxy=environment");
        return builder.proxy_url(proxy);
    }
    #[cfg(windows)]
    {
        let settings = windows::read(_relays);
        eprintln!(
            "network: system proxy config manual={} usable={} bypass={} pac={} autodetect={} read_error={:?}",
            settings.manual,
            settings.proxy.is_some(),
            settings.excluded,
            settings.pac,
            settings.autodetect,
            settings.read_error
        );
        if let Some(proxy) = settings.proxy {
            eprintln!("network: proxy=windows-user-http");
            return builder.proxy_url(proxy);
        }
    }
    builder
}

fn environment_proxy(values: [Option<String>; 4], cgi: bool) -> Option<url::Url> {
    values
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !(cgi && *i == 0))
        .find_map(|(_, value)| value.and_then(|s| s.parse().ok()))
}

#[cfg(any(windows, test))]
fn bypasses(value: &str, host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    value
        .split(|c: char| c == ';' || c.is_whitespace())
        .any(|pattern| {
            if pattern.eq_ignore_ascii_case("<local>") {
                return !host.contains('.');
            }
            let pattern = pattern.to_ascii_lowercase();
            let (pattern, host) = (pattern.as_bytes(), host.as_bytes());
            let (mut p, mut h, mut star, mut retry) = (0, 0, None, 0);
            while h < host.len() {
                if p < pattern.len() && (pattern[p] == host[h] || pattern[p] == b'?') {
                    p += 1;
                    h += 1;
                } else if p < pattern.len() && pattern[p] == b'*' {
                    star = Some(p);
                    p += 1;
                    retry = h;
                } else if let Some(s) = star {
                    p = s + 1;
                    retry += 1;
                    h = retry;
                } else {
                    return false;
                }
            }
            while p < pattern.len() && pattern[p] == b'*' {
                p += 1;
            }
            p == pattern.len()
        })
}

#[cfg(any(windows, test))]
fn parse(value: &str) -> Option<url::Url> {
    // A protocol key chooses the origin protocol, not proxy TLS. Explicit
    // schemes inside its value choose the proxy transport.
    let entries: Vec<_> = value
        .split(|c: char| c == ';' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .collect();
    let entry = if entries.iter().any(|s| s.contains('=')) {
        entries.iter().find_map(|entry| {
            let (key, value) = entry.split_once('=')?;
            key.eq_ignore_ascii_case("https").then_some(value)
        })?
    } else {
        *entries.first()?
    };
    let url: url::Url = if entry.contains("://") {
        entry.parse().ok()?
    } else {
        format!("http://{entry}").parse().ok()?
    };
    (matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.port_or_known_default()? > 0
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none())
    .then_some(url)
}

#[cfg(windows)]
mod windows {
    use windows_sys::Win32::{
        Foundation::GlobalFree,
        Networking::WinHttp::{
            WINHTTP_CURRENT_USER_IE_PROXY_CONFIG, WinHttpGetIEProxyConfigForCurrentUser,
        },
    };

    struct Config(WINHTTP_CURRENT_USER_IE_PROXY_CONFIG);
    impl Drop for Config {
        fn drop(&mut self) {
            for pointer in [
                self.0.lpszProxy,
                self.0.lpszProxyBypass,
                self.0.lpszAutoConfigUrl,
            ] {
                if !pointer.is_null() {
                    unsafe {
                        GlobalFree(pointer.cast());
                    }
                }
            }
        }
    }
    pub(super) struct Settings {
        pub proxy: Option<url::Url>,
        pub manual: bool,
        pub excluded: bool,
        pub pac: bool,
        pub autodetect: bool,
        pub read_error: Option<i32>,
    }
    // WinHTTP returns valid NUL-terminated allocated strings. Bound the read
    // and reject overlong values; Config frees each returned allocation.
    unsafe fn text(pointer: *const u16) -> Option<String> {
        if pointer.is_null() {
            return None;
        }
        for length in 0..8192 {
            if unsafe { *pointer.add(length) } == 0 {
                return String::from_utf16(unsafe { std::slice::from_raw_parts(pointer, length) })
                    .ok();
            }
        }
        None
    }
    pub(super) fn read(relays: &[String]) -> Settings {
        let mut config = Config(WINHTTP_CURRENT_USER_IE_PROXY_CONFIG::default());
        if unsafe { WinHttpGetIEProxyConfigForCurrentUser(&mut config.0) } == 0 {
            return Settings {
                proxy: None,
                manual: false,
                excluded: false,
                pac: false,
                autodetect: false,
                read_error: std::io::Error::last_os_error().raw_os_error(),
            };
        }
        let mut hosts = if relays.is_empty() {
            [
                iroh::defaults::prod::AP_RELAY_HOSTNAME,
                iroh::defaults::prod::NA_EAST_RELAY_HOSTNAME,
                iroh::defaults::prod::NA_WEST_RELAY_HOSTNAME,
                iroh::defaults::prod::EU_RELAY_HOSTNAME,
            ]
            .map(str::to_owned)
            .to_vec()
        } else {
            relays
                .iter()
                .filter_map(|url| {
                    url.parse::<url::Url>()
                        .ok()
                        .and_then(|url| url.host_str().map(str::to_owned))
                })
                .collect()
        };
        hosts.push("dns.iroh.link".into());
        let bypass = unsafe { text(config.0.lpszProxyBypass) }.unwrap_or_default();
        let excluded = hosts.iter().any(|host| super::bypasses(&bypass, host));
        let proxy = (!excluded)
            .then(|| unsafe { text(config.0.lpszProxy) }.and_then(|value| super::parse(&value)))
            .flatten();
        Settings {
            proxy,
            manual: !config.0.lpszProxy.is_null(),
            excluded,
            pac: !config.0.lpszAutoConfigUrl.is_null(),
            autodetect: config.0.fAutoDetect != 0,
            read_error: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{bypasses, environment_proxy, parse};
    #[test]
    fn explicit_environment_priority_and_cgi_are_preserved() {
        let settings = [
            Some("http://first.test:7000".into()),
            Some("http://second.test:7000".into()),
            Some("http://third.test:7000".into()),
            None,
        ];
        assert_eq!(
            environment_proxy(settings.clone(), false)
                .unwrap()
                .host_str(),
            Some("first.test")
        );
        assert_eq!(
            environment_proxy(settings, true).unwrap().host_str(),
            Some("second.test")
        );
        assert!(environment_proxy([Some("invalid".into()), None, None, None], false).is_none());
    }
    #[test]
    fn bypass_rules_do_not_force_private_or_excluded_hosts_through_proxy() {
        assert!(bypasses("<local>;*.example.test", "printer"));
        assert!(bypasses("<local>;*.example.test", "relay.example.test."));
        assert!(bypasses("*.IROH.LINK", "aps1-1.relay.n0.iroh.link."));
        assert!(!bypasses(
            "<local>;*.example.test",
            "aps1-1.relay.n0.iroh.link."
        ));
        assert!(bypasses("a?s*", "aps1"));
        assert!(!bypasses("aps*test", "aps1"));
    }
    #[test]
    fn windows_static_https_origin_uses_http_connect_proxy() {
        assert_eq!(
            parse("http=127.0.0.1:7000;https=127.0.0.1:7890")
                .unwrap()
                .as_str(),
            "http://127.0.0.1:7890/"
        );
        assert_eq!(
            parse("127.0.0.1:7890").unwrap().as_str(),
            "http://127.0.0.1:7890/"
        );
        assert_eq!(
            parse("https=[::1]:7890").unwrap().as_str(),
            "http://[::1]:7890/"
        );
        assert_eq!(
            parse("https=https://proxy.test:8443").unwrap().scheme(),
            "https"
        );
    }
    #[test]
    fn unsupported_or_invalid_proxy_is_not_applied() {
        for value in [
            "",
            "socks=127.0.0.1:1080",
            "socks5://127.0.0.1:1080",
            "http=127.0.0.1:7000",
            "http://host:0",
            "http://host/path",
            "http://host/?secret=hidden",
            "http://host/#hidden",
        ] {
            assert!(parse(value).is_none(), "{value}");
        }
    }
}
