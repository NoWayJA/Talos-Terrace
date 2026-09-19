//! Read-only Ollama client: `/api/version`, `/api/tags`, `/api/ps`.
//! Shape lifted from Modz (NoWayJA/Modz, MIT) ai/mod.rs `OllamaClient`, without the runtime
//! abstraction and with the two things Modz lacked in its first cut: a timeout and a
//! configurable URL. No inference happens in the console (principle 6); chat arrives in M2 and
//! still only calls the server.

use std::time::Duration;

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::error::{Result, TerraceError};

const TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OllamaModel {
    pub name: String,
    pub size: u64,
    pub digest: String,
    pub family: Option<String>,
    pub parameter_size: Option<String>,
    pub quantization: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OllamaLoadedModel {
    pub name: String,
    pub size_vram: u64,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaStatus {
    pub url: String,
    pub version: String,
    pub models: Vec<OllamaModel>,
    pub loaded: Vec<OllamaLoadedModel>,
}

#[derive(Deserialize)]
struct VersionResponse {
    version: String,
}

#[derive(Deserialize)]
struct ModelDetails {
    family: Option<String>,
    parameter_size: Option<String>,
    quantization_level: Option<String>,
}

#[derive(Deserialize)]
struct TagModel {
    name: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    digest: String,
    details: Option<ModelDetails>,
}

#[derive(Deserialize)]
struct TagsResponse {
    #[serde(default)]
    models: Vec<TagModel>,
}

#[derive(Deserialize)]
struct PsModel {
    name: String,
    #[serde(default)]
    size_vram: u64,
    expires_at: Option<String>,
}

#[derive(Deserialize)]
struct PsResponse {
    #[serde(default)]
    models: Vec<PsModel>,
}

impl From<TagModel> for OllamaModel {
    fn from(model: TagModel) -> Self {
        let details = model.details;
        Self {
            name: model.name,
            size: model.size,
            digest: model.digest,
            family: details.as_ref().and_then(|d| d.family.clone()),
            parameter_size: details.as_ref().and_then(|d| d.parameter_size.clone()),
            quantization: details.as_ref().and_then(|d| d.quantization_level.clone()),
        }
    }
}

impl From<PsModel> for OllamaLoadedModel {
    fn from(model: PsModel) -> Self {
        Self {
            name: model.name,
            size_vram: model.size_vram,
            expires_at: model.expires_at,
        }
    }
}

pub struct OllamaClient {
    base_url: String,
    http: reqwest::Client,
}

impl OllamaClient {
    pub fn new(base_url: &str) -> Result<Self> {
        let base_url = base_url.trim().trim_end_matches('/').to_string();
        validate_url(&base_url)?;
        let http = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| TerraceError::Ollama(format!("http client: {error}")))?;
        Ok(Self { base_url, http })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The URL as it may be shown: a `user:password@` in it (a reverse proxy's basic auth)
    /// never reaches a message, a log line or the canvas.
    pub fn display_url(&self) -> String {
        redact_userinfo(&self.base_url)
    }

    fn map_error(&self, error: reqwest::Error) -> TerraceError {
        let shown = self.display_url();
        let error = error.without_url();
        if error.is_connect() || error.is_timeout() {
            TerraceError::NotConnected(format!("Ollama at {shown}: {error}"))
        } else {
            TerraceError::Ollama(format!("Ollama at {shown}: {error}"))
        }
    }

    async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{path}", self.base_url);
        let shown = format!("{}{path}", self.display_url());
        let response = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| self.map_error(e))?;
        let response = response
            .error_for_status()
            .map_err(|error| TerraceError::Ollama(format!("{shown}: {}", error.without_url())))?;
        if response.status().is_redirection() {
            return Err(TerraceError::Ollama(format!(
                "{shown}: redirects are disabled; configure the final server URL"
            )));
        }
        response
            .json::<T>()
            .await
            .map_err(|error| TerraceError::Ollama(format!("{shown}: {}", error.without_url())))
    }

    /// Version, available models and loaded models in one call.
    pub async fn status(&self) -> Result<OllamaStatus> {
        let version: VersionResponse = self.get_json("/api/version").await?;
        let tags: TagsResponse = self.get_json("/api/tags").await?;
        let ps: PsResponse = self.get_json("/api/ps").await?;
        Ok(OllamaStatus {
            url: self.display_url(),
            version: version.version,
            models: tags.models.into_iter().map(OllamaModel::from).collect(),
            loaded: ps.models.into_iter().map(OllamaLoadedModel::from).collect(),
        })
    }
}

/// URLs are plain settings, so embedded credentials and query tokens cannot be accepted.
/// Never include the rejected input in the error returned to the webview or logs.
pub fn validate_url(value: &str) -> Result<()> {
    let invalid = || {
        TerraceError::Config(
        "Ollama URL must be an http or https server URL without credentials, query parameters or a fragment".into(),
    )
    };
    let url = reqwest::Url::parse(value.trim()).map_err(|_| invalid())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid());
    }
    Ok(())
}

/// `scheme://user:secret@host/...` becomes `scheme://host/...`; anything that is not a URL
/// with userinfo comes back unchanged.
fn redact_userinfo(text: &str) -> String {
    match reqwest::Url::parse(text) {
        Ok(mut url) if !url.username().is_empty() || url.password().is_some() => {
            let _ = url.set_username("");
            let _ = url.set_password(None);
            url.to_string().trim_end_matches('/').to_string()
        }
        _ => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_in_the_url_are_never_shown() {
        for url in [
            "http://user:secret@ollama.example:11434",
            "https://ollama.example?token=secret",
            "https://ollama.example/#secret",
            "file:///secret",
        ] {
            let error = validate_url(url).expect_err("reject unsafe setting");
            assert!(!error.to_string().contains("secret"));
        }
        assert_eq!(redact_userinfo("not a url"), "not a url");
        assert_eq!(
            redact_userinfo("http://plain.example"),
            "http://plain.example"
        );
    }

    #[test]
    fn trims_trailing_slashes_and_rejects_empty_urls() {
        let client = OllamaClient::new("http://localhost:11434/").expect("client");
        assert_eq!(client.base_url(), "http://localhost:11434");
        assert!(OllamaClient::new("   ").is_err());
    }

    #[test]
    fn maps_the_tags_response() {
        let text = r#"{"models":[{"name":"qwen3.8:27b","model":"qwen3.8:27b","size":17000000000,"digest":"abc","details":{"family":"qwen3","parameter_size":"27B","quantization_level":"Q4_K_M"}}]}"#;
        let tags: TagsResponse = serde_json::from_str(text).expect("parse");
        let model = OllamaModel::from(tags.models.into_iter().next().expect("one"));
        assert_eq!(model.name, "qwen3.8:27b");
        assert_eq!(model.family.as_deref(), Some("qwen3"));
        assert_eq!(model.quantization.as_deref(), Some("Q4_K_M"));
    }

    #[test]
    fn maps_an_empty_ps_response() {
        let ps: PsResponse = serde_json::from_str(r#"{"models":[]}"#).expect("parse");
        assert!(ps.models.is_empty());
    }

    #[tokio::test]
    async fn a_closed_port_is_not_connected() {
        // A port the OS just handed out and we released: nothing listens there.
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
            listener.local_addr().expect("addr").port()
        };
        let client = OllamaClient::new(&format!("http://127.0.0.1:{port}")).expect("client");
        let error = client.status().await.expect_err("closed");
        assert_eq!(error.code(), "NOT_CONNECTED");
    }
}
