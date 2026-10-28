use anyhow::{Context, ensure};
use reqwest::blocking::{Client, Response};
use serde_json::Value;
use std::{io::Read, time::Duration};
use url::Url;

#[derive(Clone)]
pub(super) struct Api {
    pub client: Client,
    api: String,
    oauth: String,
    #[cfg(test)]
    pub test_origin: Option<String>,
}
impl Api {
    pub fn new(route: &str) -> anyhow::Result<Self> {
        ensure!(
            matches!(route, "system" | "direct"),
            "百度网盘请选择系统路由或仅直连"
        );
        let builder = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .user_agent("TwoDrive/0.1.7 pan.baidu.com");
        let client = if route == "direct" {
            builder.no_proxy()
        } else {
            builder
        }
        .build()
        .context("百度网络初始化失败")?;
        Ok(Self {
            client,
            api: "https://pan.baidu.com".into(),
            oauth: "https://openapi.baidu.com".into(),
            #[cfg(test)]
            test_origin: None,
        })
    }
    #[cfg(test)]
    pub fn local(origin: String) -> Self {
        let mut api = Self::new("direct").unwrap();
        api.api = origin.clone();
        api.oauth = origin.clone();
        api.test_origin = Some(origin);
        api
    }
    pub fn oauth(&self, path: &str, params: &[(&str, &str)]) -> anyhow::Result<Value> {
        // Match the Open Platform device-token protocol. Never format the URL
        // or upstream error body into an error: both can contain credentials.
        let response = self
            .client
            .get(format!("{}{path}", self.oauth))
            .query(params)
            .send()
            .map_err(|_| anyhow::anyhow!("百度授权网络暂不可用，请稍后重试"))?;
        bounded_json(response)
    }
    pub fn get(
        &self,
        path: &str,
        token: &str,
        params: &[(&str, &str)],
        check: &mut dyn FnMut() -> anyhow::Result<()>,
    ) -> anyhow::Result<Value> {
        for attempt in 0..3 {
            check()?;
            let response = self
                .client
                .get(format!("{}{path}", self.api))
                .query(params)
                .query(&[("access_token", token)])
                .send();
            match response {
                Ok(response) => {
                    let status = response.status();
                    if status.as_u16() == 429 || status.is_server_error() {
                        if attempt < 2 {
                            delay(Duration::from_millis(300 * (attempt + 1)), check)?;
                            continue;
                        }
                        anyhow::bail!("百度网盘服务繁忙或请求受限，请稍后重试");
                    }
                    let value = bounded_json(response)?;
                    check()?;
                    return Ok(value);
                }
                Err(_) if attempt < 2 => delay(Duration::from_millis(300 * (attempt + 1)), check)?,
                Err(_) => anyhow::bail!("百度网盘网络暂不可用，请稍后重试"),
            }
        }
        unreachable!()
    }
    pub fn trusted_download(&self, url: &Url) -> bool {
        #[cfg(test)]
        if let Some(origin) = &self.test_origin {
            return url.origin().ascii_serialization() == *origin;
        }
        let host = url.host_str().unwrap_or_default();
        url.scheme() == "https"
            && url.port_or_known_default() == Some(443)
            && url.username().is_empty()
            && url.password().is_none()
            && ["baidu.com", "baidupcs.com"]
                .iter()
                .any(|d| host == *d || host.ends_with(&format!(".{d}")))
    }
}
pub(super) fn bounded_json(response: Response) -> anyhow::Result<Value> {
    let status = response.status();
    ensure!(
        status.is_success() || status.as_u16() == 400,
        "百度请求未完成（HTTP {}）",
        status.as_u16()
    );
    let mut bytes = Vec::new();
    response
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| anyhow::anyhow!("百度响应读取失败"))?;
    ensure!(bytes.len() <= 4 * 1024 * 1024, "百度响应超过大小预算");
    serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("百度返回了无效响应"))
}
pub(super) fn errno(value: &Value) -> anyhow::Result<()> {
    let code = value["errno"]
        .as_i64()
        .or_else(|| value["error_code"].as_i64())
        .context("百度响应缺少明确状态码")?;
    match code {
        0 => Ok(()),
        -6 | 110 | 20016 | 20017 | 31045 => {
            anyhow::bail!("百度授权失效或权限不足，请重新授权并检查应用权限（代码 {code}）")
        }
        -9 => anyhow::bail!("百度文件或目录已不存在（代码 -9）"),
        6 | 31023 | 31024 | 31034 => {
            anyhow::bail!("百度拒绝访问或限制请求，请检查应用权限后重试（代码 {code}）")
        }
        _ => anyhow::bail!("百度网盘未完成请求（代码 {code}）"),
    }
}
pub(super) fn delay(
    duration: Duration,
    check: &mut dyn FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let start = std::time::Instant::now();
    while start.elapsed() < duration {
        check()?;
        std::thread::sleep(Duration::from_millis(30));
    }
    check()
}
