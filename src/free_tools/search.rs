use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

use super::FreeToolError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    pub query: String,
    pub max_results: usize,
    pub language: Option<String>,
    pub region: Option<String>,
}

impl SearchQuery {
    pub fn new(query: impl Into<String>, max_results: usize) -> Result<Self, FreeToolError> {
        let trimmed = query.into().trim().to_string();
        if trimmed.is_empty() {
            return Err(FreeToolError::InvalidRequest(
                "La consulta de búsqueda no puede estar vacía.".to_string(),
            ));
        }
        if trimmed.chars().count() > 400 {
            return Err(FreeToolError::InvalidRequest(
                "La consulta de búsqueda supera el máximo de 400 caracteres.".to_string(),
            ));
        }
        Ok(Self {
            query: trimmed,
            max_results: max_results.clamp(1, 10),
            language: None,
            region: None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub source: String,
    pub published_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResponse {
    pub provider: String,
    pub results: Vec<SearchResult>,
    pub elapsed_ms: u64,
    pub confirmed_credits: Option<u32>,
}

#[async_trait]
pub trait SearchProvider: Send + Sync {
    #[allow(dead_code)]
    fn id(&self) -> &'static str;
    async fn search(&self, query: SearchQuery) -> Result<SearchResponse, FreeToolError>;
}

pub struct TavilyProvider {
    pub client: Client,
    pub api_key: String,
    pub base_url: String,
    pub timeout: Duration,
}

impl TavilyProvider {
    pub fn new(client: Client, api_key: impl Into<String>) -> Self {
        Self {
            client,
            api_key: api_key.into(),
            base_url: "https://api.tavily.com/search".to_string(),
            timeout: Duration::from_secs(10),
        }
    }

    #[allow(dead_code)]
    pub fn with_base_url(
        client: Client,
        api_key: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Self {
        Self {
            client,
            api_key: api_key.into(),
            base_url: base_url.into(),
            timeout: Duration::from_secs(5),
        }
    }
}

#[derive(Deserialize)]
struct TavilyApiResponse {
    #[serde(default)]
    results: Vec<TavilyApiResult>,
}

#[derive(Deserialize)]
struct TavilyApiResult {
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    content: String,
    published_date: Option<String>,
}

pub fn parse_tavily_response(
    body: &str,
    max_results: usize,
    elapsed_ms: u64,
) -> Result<SearchResponse, FreeToolError> {
    let parsed: TavilyApiResponse = serde_json::from_str(body).map_err(|_| {
        FreeToolError::ProviderUnavailable("Respuesta JSON inesperada de Tavily.".to_string())
    })?;
    let results = parsed
        .results
        .into_iter()
        .filter(|r| !r.url.trim().is_empty())
        .take(max_results)
        .map(|r| SearchResult {
            title: if r.title.trim().is_empty() {
                r.url.clone()
            } else {
                r.title.trim().to_string()
            },
            url: r.url.trim().to_string(),
            snippet: r.content.trim().to_string(),
            source: "Tavily".to_string(),
            published_at: r
                .published_date
                .map(|d| d.trim().to_string())
                .filter(|d| !d.is_empty()),
        })
        .collect();
    Ok(SearchResponse {
        provider: "tavily".to_string(),
        results,
        elapsed_ms,
        confirmed_credits: Some(1),
    })
}

#[async_trait]
impl SearchProvider for TavilyProvider {
    fn id(&self) -> &'static str {
        "tavily"
    }

    async fn search(&self, query: SearchQuery) -> Result<SearchResponse, FreeToolError> {
        let key = self.api_key.trim();
        if key.is_empty() {
            return Err(FreeToolError::NotConfigured(
                "Tavily no tiene API key configurada.".to_string(),
            ));
        }

        let start = Instant::now();
        let payload = serde_json::json!({
            "api_key": key,
            "query": query.query,
            "search_depth": "basic",
            "max_results": query.max_results,
            "include_answer": false,
            "include_raw_content": false
        });

        let resp = self
            .client
            .post(&self.base_url)
            .timeout(self.timeout)
            .json(&payload)
            .send()
            .await
            .map_err(map_reqwest_error)?;

        let status = resp.status();
        if status.as_u16() == 429 {
            return Err(FreeToolError::RateLimited(
                "Tavily alcanzó el límite temporal de solicitudes (HTTP 429).".to_string(),
            ));
        }
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(FreeToolError::NotConfigured(
                "Tavily rechazó la credencial configurada.".to_string(),
            ));
        }
        if !status.is_success() {
            return Err(FreeToolError::ProviderUnavailable(format!(
                "Tavily devolvió HTTP {}.",
                status.as_u16()
            )));
        }

        let body = resp.text().await.map_err(map_reqwest_error)?;
        let elapsed_ms = start.elapsed().as_millis() as u64;
        parse_tavily_response(&body, query.max_results, elapsed_ms)
    }
}

/// LangSearch Web Search API. Uses snippets to minimize the free daily token allowance.
pub struct LangSearchProvider {
    pub client: Client,
    pub api_key: String,
    pub base_url: String,
    pub timeout: Duration,
}

impl LangSearchProvider {
    pub fn new(client: Client, api_key: impl Into<String>) -> Self {
        Self {
            client,
            api_key: api_key.into(),
            base_url: "https://api.langsearch.com/v1/web-search".to_string(),
            timeout: Duration::from_secs(10),
        }
    }
}

#[derive(Deserialize)]
struct LangSearchApiResponse {
    data: Option<LangSearchData>,
}

#[derive(Deserialize)]
struct LangSearchData {
    #[serde(rename = "webPages")]
    web_pages: Option<LangSearchPages>,
}

#[derive(Deserialize)]
struct LangSearchPages {
    #[serde(default)]
    value: Vec<LangSearchResult>,
}

#[derive(Deserialize)]
struct LangSearchResult {
    #[serde(default)]
    name: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    snippet: String,
    #[serde(default)]
    text: String,
    #[serde(rename = "datePublished")]
    date_published: Option<String>,
}

pub fn parse_langsearch_response(
    body: &str,
    max_results: usize,
    elapsed_ms: u64,
) -> Result<SearchResponse, FreeToolError> {
    let parsed: LangSearchApiResponse = serde_json::from_str(body).map_err(|_| {
        FreeToolError::ProviderUnavailable("Respuesta JSON inesperada de LangSearch.".to_string())
    })?;
    let pages = parsed.data.and_then(|d| d.web_pages).ok_or_else(|| {
        FreeToolError::ProviderUnavailable(
            "LangSearch no devolvió la sección webPages.".to_string(),
        )
    })?;
    let results = pages
        .value
        .into_iter()
        .filter(|r| !r.url.trim().is_empty())
        .take(max_results)
        .map(|r| SearchResult {
            title: if r.name.trim().is_empty() {
                r.url.trim().to_string()
            } else {
                r.name.trim().to_string()
            },
            url: r.url.trim().to_string(),
            snippet: if r.snippet.trim().is_empty() {
                r.text.trim().to_string()
            } else {
                r.snippet.trim().to_string()
            },
            source: "LangSearch".to_string(),
            published_at: r.date_published.filter(|date| !date.trim().is_empty()),
        })
        .collect();
    Ok(SearchResponse {
        provider: "langsearch".to_string(),
        results,
        elapsed_ms,
        // LangSearch's free quota is token-based, not credits per request.
        // The local request reservation still counts each successful call as one.
        confirmed_credits: Some(1),
    })
}

#[async_trait]
impl SearchProvider for LangSearchProvider {
    fn id(&self) -> &'static str {
        "langsearch"
    }

    async fn search(&self, query: SearchQuery) -> Result<SearchResponse, FreeToolError> {
        if self.api_key.trim().is_empty() {
            return Err(FreeToolError::NotConfigured(
                "LangSearch no tiene API key configurada.".to_string(),
            ));
        }
        let start = Instant::now();
        let response = self
            .client
            .post(&self.base_url)
            .timeout(self.timeout)
            .bearer_auth(self.api_key.trim())
            .json(&serde_json::json!({
                "query": query.query,
                "count": query.max_results,
                "freshness": "noLimit",
            }))
            .send()
            .await
            .map_err(map_reqwest_error)?;
        match response.status().as_u16() {
            401 | 403 => {
                return Err(FreeToolError::NotConfigured(
                    "LangSearch rechazó la API key.".to_string(),
                ))
            }
            429 => {
                return Err(FreeToolError::RateLimited(
                    "LangSearch alcanzó su límite de solicitudes o tokens.".to_string(),
                ))
            }
            status if !(200..300).contains(&status) => {
                return Err(FreeToolError::ProviderUnavailable(format!(
                    "LangSearch devolvió HTTP {status}."
                )))
            }
            _ => {}
        }
        let body = response.text().await.map_err(map_reqwest_error)?;
        parse_langsearch_response(&body, query.max_results, start.elapsed().as_millis() as u64)
    }
}

pub struct BraveProvider {
    pub client: Client,
    pub api_key: String,
    pub base_url: String,
    pub timeout: Duration,
}

impl BraveProvider {
    pub fn new(client: Client, api_key: impl Into<String>) -> Self {
        Self {
            client,
            api_key: api_key.into(),
            base_url: "https://api.search.brave.com/res/v1/web/search".to_string(),
            timeout: Duration::from_secs(10),
        }
    }

    #[allow(dead_code)]
    pub fn with_base_url(
        client: Client,
        api_key: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Self {
        Self {
            client,
            api_key: api_key.into(),
            base_url: base_url.into(),
            timeout: Duration::from_secs(5),
        }
    }
}

#[derive(Deserialize)]
struct BraveApiResponse {
    web: Option<BraveWebSection>,
}

#[derive(Deserialize)]
struct BraveWebSection {
    #[serde(default)]
    results: Vec<BraveApiResult>,
}

#[derive(Deserialize)]
struct BraveApiResult {
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    description: String,
    page_age: Option<String>,
}

pub fn parse_brave_response(
    body: &str,
    max_results: usize,
    elapsed_ms: u64,
) -> Result<SearchResponse, FreeToolError> {
    let parsed: BraveApiResponse = serde_json::from_str(body).map_err(|_| {
        FreeToolError::ProviderUnavailable("Respuesta JSON inesperada de Brave Search.".to_string())
    })?;
    let raw = parsed.web.map(|w| w.results).unwrap_or_default();
    let results = raw
        .into_iter()
        .filter(|r| !r.url.trim().is_empty())
        .take(max_results)
        .map(|r| SearchResult {
            title: if r.title.trim().is_empty() {
                r.url.clone()
            } else {
                r.title.trim().to_string()
            },
            url: r.url.trim().to_string(),
            snippet: r.description.trim().to_string(),
            source: "Brave Search".to_string(),
            published_at: r
                .page_age
                .map(|d| d.trim().to_string())
                .filter(|d| !d.is_empty()),
        })
        .collect();
    Ok(SearchResponse {
        provider: "brave".to_string(),
        results,
        elapsed_ms,
        confirmed_credits: Some(1),
    })
}

#[async_trait]
impl SearchProvider for BraveProvider {
    fn id(&self) -> &'static str {
        "brave"
    }

    async fn search(&self, query: SearchQuery) -> Result<SearchResponse, FreeToolError> {
        let key = self.api_key.trim();
        if key.is_empty() {
            return Err(FreeToolError::NotConfigured(
                "Brave Search no tiene API key configurada.".to_string(),
            ));
        }

        let start = Instant::now();
        let mut req = self
            .client
            .get(&self.base_url)
            .timeout(self.timeout)
            .header("Accept", "application/json")
            .header("X-Subscription-Token", key)
            .query(&[
                ("q", query.query.as_str()),
                ("count", &query.max_results.to_string()),
            ]);

        if let Some(lang) = query.language.as_deref() {
            req = req.query(&[("search_lang", lang)]);
        }
        if let Some(reg) = query.region.as_deref() {
            req = req.query(&[("country", reg)]);
        }

        let resp = req.send().await.map_err(map_reqwest_error)?;
        let status = resp.status();
        if status.as_u16() == 429 {
            return Err(FreeToolError::RateLimited(
                "Brave Search alcanzó el límite de solicitudes (HTTP 429).".to_string(),
            ));
        }
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(FreeToolError::NotConfigured(
                "Brave Search rechazó la credencial configurada.".to_string(),
            ));
        }
        if !status.is_success() {
            return Err(FreeToolError::ProviderUnavailable(format!(
                "Brave Search devolvió HTTP {}.",
                status.as_u16()
            )));
        }

        let body = resp.text().await.map_err(map_reqwest_error)?;
        let elapsed_ms = start.elapsed().as_millis() as u64;
        parse_brave_response(&body, query.max_results, elapsed_ms)
    }
}

/// Self-hosted SearXNG provider contract (endpoint expected: `<base_url>/search?q=...&format=json`).
/// Never uses arbitrary public instances as automatic fallback.
#[allow(dead_code)]
pub struct SearxngProvider {
    pub client: Client,
    pub base_url: String,
    pub timeout: Duration,
}

#[derive(Deserialize)]
struct SearxngApiResponse {
    #[serde(default)]
    results: Vec<SearxngApiResult>,
}

#[derive(Deserialize)]
struct SearxngApiResult {
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    content: String,
    published_date: Option<String>,
}

#[async_trait]
impl SearchProvider for SearxngProvider {
    fn id(&self) -> &'static str {
        "searxng"
    }

    async fn search(&self, query: SearchQuery) -> Result<SearchResponse, FreeToolError> {
        let base = self.base_url.trim().trim_end_matches('/');
        if base.is_empty() {
            return Err(FreeToolError::NotConfigured(
                "SearXNG requiere una URL base autoalojada configurada.".to_string(),
            ));
        }
        let endpoint = format!("{base}/search");
        let start = Instant::now();
        let resp = self
            .client
            .get(&endpoint)
            .timeout(self.timeout)
            .query(&[("q", query.query.as_str()), ("format", "json")])
            .send()
            .await
            .map_err(map_reqwest_error)?;

        let status = resp.status();
        if status.as_u16() == 429 {
            return Err(FreeToolError::RateLimited(
                "SearXNG devolvió HTTP 429.".to_string(),
            ));
        }
        if !status.is_success() {
            return Err(FreeToolError::ProviderUnavailable(format!(
                "SearXNG devolvió HTTP {}.",
                status.as_u16()
            )));
        }
        let body = resp.text().await.map_err(map_reqwest_error)?;
        let parsed: SearxngApiResponse = serde_json::from_str(&body).map_err(|_| {
            FreeToolError::ProviderUnavailable("Respuesta JSON inesperada de SearXNG.".to_string())
        })?;
        let results = parsed
            .results
            .into_iter()
            .filter(|r| !r.url.trim().is_empty())
            .take(query.max_results)
            .map(|r| SearchResult {
                title: if r.title.trim().is_empty() {
                    r.url.clone()
                } else {
                    r.title.trim().to_string()
                },
                url: r.url.trim().to_string(),
                snippet: r.content.trim().to_string(),
                source: "SearXNG".to_string(),
                published_at: r.published_date.filter(|d| !d.trim().is_empty()),
            })
            .collect();
        Ok(SearchResponse {
            provider: "searxng".to_string(),
            results,
            elapsed_ms: start.elapsed().as_millis() as u64,
            confirmed_credits: Some(0),
        })
    }
}

pub fn map_reqwest_error(err: reqwest::Error) -> FreeToolError {
    if err.is_timeout() {
        FreeToolError::Timeout("Tiempo de espera agotado al consultar el proveedor.".to_string())
    } else {
        FreeToolError::NetworkError(
            "No se pudo conectar con el servicio externo (revisá tu conexión).".to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_langsearch_results_and_rejects_invalid_payloads() {
        let payload = r#"{"data":{"webPages":{"value":[
            {"name":"Official docs","url":"https://example.org/docs","snippet":"Reference","datePublished":"2026-11-01"},
            {"name":"","url":"https://example.org/2","text":"Fallback content"},
            {"name":"Invalid","url":"","snippet":"ignored"}
        ]}}}"#;
        let result = parse_langsearch_response(payload, 5, 12).unwrap();
        assert_eq!(result.provider, "langsearch");
        assert_eq!(result.results.len(), 2);
        assert_eq!(
            result.results[0].published_at.as_deref(),
            Some("2026-11-01")
        );
        assert_eq!(result.results[1].title, "https://example.org/2");
        assert_eq!(result.results[1].snippet, "Fallback content");
        assert!(
            parse_langsearch_response(r#"{"data":{"webPages":{"value":[]}}}"#, 5, 0)
                .unwrap()
                .results
                .is_empty()
        );
        assert!(parse_langsearch_response("{}", 5, 0).is_err());
        assert!(parse_langsearch_response("invalid", 5, 0).is_err());
    }

    #[test]
    fn normalizes_tavily_valid_empty_missing_and_unexpected_json() {
        let valid = r#"{
            "results": [
                { "title": "Rust 1.90", "url": "https://blog.rust-lang.org/", "content": "Release notes", "published_date": "2026-11-01" },
                { "title": "", "url": "https://example.com/no-date", "content": "Details" }
            ]
        }"#;
        let out = parse_tavily_response(valid, 5, 42).unwrap();
        assert_eq!(out.provider, "tavily");
        assert_eq!(out.results.len(), 2);
        assert_eq!(out.results[0].published_at.as_deref(), Some("2026-11-01"));
        assert_eq!(out.results[1].title, "https://example.com/no-date");
        assert_eq!(out.results[1].published_at, None);

        let empty = r#"{"results": []}"#;
        assert!(parse_tavily_response(empty, 5, 10)
            .unwrap()
            .results
            .is_empty());

        assert!(matches!(
            parse_tavily_response("not json", 5, 10),
            Err(FreeToolError::ProviderUnavailable(_))
        ));
    }

    #[test]
    fn normalizes_brave_valid_empty_and_unexpected_json() {
        let valid = r#"{
            "web": {
                "results": [
                    { "title": "Brave Result", "url": "https://rust-lang.org", "description": "Official site" }
                ]
            }
        }"#;
        let out = parse_brave_response(valid, 5, 30).unwrap();
        assert_eq!(out.provider, "brave");
        assert_eq!(out.results.len(), 1);
        assert_eq!(out.results[0].source, "Brave Search");
        assert_eq!(out.results[0].published_at, None);

        assert!(parse_brave_response("{}", 5, 10)
            .unwrap()
            .results
            .is_empty());
        assert!(matches!(
            parse_brave_response("{bad", 5, 10),
            Err(FreeToolError::ProviderUnavailable(_))
        ));
    }
}
