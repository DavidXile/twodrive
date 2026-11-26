use crate::util::{atomic_write, now_unix, random_ascii};
use anyhow::{Context, bail};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use tiny_http::{Response, Server};
use url::Url;

pub const DEFAULT_CLIENT_ID: &str = "178705ac-2286-441b-9652-1a4d86be2c51";
pub const REDIRECT_URI: &str = "http://localhost:53682";
pub const SCOPES: &str = "Files.ReadWrite User.Read offline_access openid profile";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenData {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at_unix: i64,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
}

pub struct TokenManager {
    client: Client,
    token_path: std::path::PathBuf,
    client_id: String,
}

impl TokenManager {
    pub fn new(token_path: impl Into<std::path::PathBuf>, client_id: String) -> anyhow::Result<Self> {
        Ok(Self {
            client: Client::builder().timeout(std::time::Duration::from_secs(60)).build()?,
            token_path: token_path.into(),
            client_id,
        })
    }

    pub fn login(&self) -> anyhow::Result<()> {
        let verifier = random_ascii(64);
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let state = random_ascii(32);
        let mut auth = Url::parse("https://login.microsoftonline.com/common/oauth2/v2.0/authorize")?;
        auth.query_pairs_mut()
            .append_pair("client_id", &self.client_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", REDIRECT_URI)
            .append_pair("response_mode", "query")
            .append_pair("scope", SCOPES)
            .append_pair("state", &state)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256");

        let server = Server::http("127.0.0.1:53682")
            .map_err(|e| anyhow::anyhow!("cannot listen on 127.0.0.1:53682 for OAuth callback: {e}"))?;
        println!("Open this URL to sign in:\n{auth}\n");
        let _ = open::that(auth.as_str());
        let request = server.recv()?;
        let callback = Url::parse(&format!("http://localhost:53682{}", request.url()))?;
        let params: HashMap<String, String> = callback.query_pairs().into_owned().collect();
        let ok = params.get("state") == Some(&state) && params.contains_key("code");
        let body = if ok { "TwoDrive Relay Lab login complete. You can close this tab." } else { "TwoDrive Relay Lab login failed. Return to the terminal." };
        let status: u16 = if ok { 200 } else { 400 };
        let _ = request.respond(Response::from_string(body).with_status_code(status));
        if params.get("state") != Some(&state) {
            bail!("OAuth state mismatch");
        }
        if let Some(error) = params.get("error") {
            bail!("OAuth failed: {}", params.get("error_description").unwrap_or(error));
        }
        let code = params.get("code").context("OAuth callback did not include a code")?;
        let token = self.exchange_code(code, &verifier)?;
        self.save(&token)?;
        println!("Microsoft login succeeded; token stored in the relay-lab config directory.");
        Ok(())
    }

    pub fn access_token(&self) -> anyhow::Result<String> {
        let mut token = self.load()?;
        if token.expires_at_unix > now_unix() + 60 {
            return Ok(token.access_token);
        }
        let refresh = token.refresh_token.clone().context("token expired and no refresh token is stored; run login again")?;
        let response = self.client
            .post("https://login.microsoftonline.com/common/oauth2/v2.0/token")
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("scope", SCOPES),
                ("refresh_token", refresh.as_str()),
                ("grant_type", "refresh_token"),
            ])
            .send()?;
        let status = response.status();
        let text = response.text()?;
        if !status.is_success() {
            bail!("OAuth refresh failed with HTTP {status}: {text}");
        }
        let refreshed: TokenResponse = serde_json::from_str(&text)?;
        token.access_token = refreshed.access_token;
        token.refresh_token = refreshed.refresh_token.or(token.refresh_token);
        token.expires_at_unix = now_unix() + refreshed.expires_in.unwrap_or(3600);
        self.save(&token)?;
        Ok(token.access_token)
    }

    fn exchange_code(&self, code: &str, verifier: &str) -> anyhow::Result<TokenData> {
        let response = self.client
            .post("https://login.microsoftonline.com/common/oauth2/v2.0/token")
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("scope", SCOPES),
                ("code", code),
                ("redirect_uri", REDIRECT_URI),
                ("grant_type", "authorization_code"),
                ("code_verifier", verifier),
            ])
            .send()?;
        let status = response.status();
        let text = response.text()?;
        if !status.is_success() {
            bail!("OAuth token exchange failed with HTTP {status}: {text}");
        }
        let response: TokenResponse = serde_json::from_str(&text)?;
        Ok(TokenData {
            access_token: response.access_token,
            refresh_token: response.refresh_token,
            expires_at_unix: now_unix() + response.expires_in.unwrap_or(3600),
        })
    }

    fn load(&self) -> anyhow::Result<TokenData> {
        let bytes = std::fs::read(&self.token_path)
            .with_context(|| format!("no relay-lab token found at {}; run `login`", self.token_path.display()))?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    fn save(&self, token: &TokenData) -> anyhow::Result<()> {
        atomic_write(&self.token_path, &serde_json::to_vec_pretty(token)?, true)
    }
}
