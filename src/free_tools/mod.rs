pub mod exchange;
pub mod fetch;
pub mod search;

use chrono::Utc;
use reqwest::Client;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::config::{ExternalToolsConfig, SearchProviderConfig, SearchProviderId};
use crate::persistence::Database;

use exchange::FrankfurterProvider;
use fetch::{fetch_url_safe, format_untrusted_fetch_for_llm};
use search::{
    BraveProvider, LangSearchProvider, SearchProvider, SearchQuery, SearchResponse, TavilyProvider,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FreeToolError {
    NotConfigured(String),
    QuotaExceeded(String),
    RateLimited(String),
    Timeout(String),
    NetworkError(String),
    InvalidRequest(String),
    ProviderUnavailable(String),
    UnsafeUrl(String),
}

impl FreeToolError {
    pub fn kind_code(&self) -> &'static str {
        match self {
            Self::NotConfigured(_) => "NotConfigured",
            Self::QuotaExceeded(_) => "QuotaExceeded",
            Self::RateLimited(_) => "RateLimited",
            Self::Timeout(_) => "Timeout",
            Self::NetworkError(_) => "NetworkError",
            Self::InvalidRequest(_) => "InvalidRequest",
            Self::ProviderUnavailable(_) => "ProviderUnavailable",
            Self::UnsafeUrl(_) => "UnsafeUrl",
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::NotConfigured(m)
            | Self::QuotaExceeded(m)
            | Self::RateLimited(m)
            | Self::Timeout(m)
            | Self::NetworkError(m)
            | Self::InvalidRequest(m)
            | Self::ProviderUnavailable(m)
            | Self::UnsafeUrl(m) => m,
        }
    }

    /// Returns `true` when trying an alternative provider is safe and appropriate.
    pub fn allows_fallback(&self) -> bool {
        !matches!(self, Self::InvalidRequest(_) | Self::UnsafeUrl(_))
    }

    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::Timeout(_) | Self::NetworkError(_) | Self::ProviderUnavailable(_)
        )
    }
}

impl std::fmt::Display for FreeToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderHealthStatus {
    Available,
    NotConfigured,
    Disabled,
    QuotaExhausted,
    TemporaryError,
    NeedsVerification,
}

impl ProviderHealthStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Available => "Disponible",
            Self::NotConfigured => "Sin configurar",
            Self::Disabled => "Deshabilitado",
            Self::QuotaExhausted => "Cuota agotada",
            Self::TemporaryError => "Error temporal",
            Self::NeedsVerification => "Requiere verificación",
        }
    }
}

/// Per-turn call budget to prevent runaway autonomous external loops.
#[derive(Debug, Clone)]
pub struct TurnExternalLimits {
    pub max_external_calls: usize,
    pub max_web_searches: usize,
    pub max_fetches_after_search: usize,
    external_calls: usize,
    web_searches: usize,
    fetches_after_search: usize,
}

impl Default for TurnExternalLimits {
    fn default() -> Self {
        Self {
            max_external_calls: 3,
            max_web_searches: 2,
            max_fetches_after_search: 1,
            external_calls: 0,
            web_searches: 0,
            fetches_after_search: 0,
        }
    }
}

impl TurnExternalLimits {
    pub fn try_acquire(&mut self, tool_name: &str) -> Result<(), FreeToolError> {
        if self.external_calls >= self.max_external_calls {
            return Err(FreeToolError::QuotaExceeded(format!(
                "Límite por turno alcanzado (máximo {} llamadas externas).",
                self.max_external_calls
            )));
        }
        match tool_name {
            "web_search" => {
                if self.web_searches >= self.max_web_searches {
                    return Err(FreeToolError::QuotaExceeded(format!(
                        "Límite por turno alcanzado (máximo {} búsquedas web).",
                        self.max_web_searches
                    )));
                }
                self.web_searches += 1;
            }
            "web_fetch" if self.web_searches > 0 => {
                if self.fetches_after_search >= self.max_fetches_after_search {
                    return Err(FreeToolError::QuotaExceeded(format!(
                        "Límite por turno alcanzado (máximo {} lectura web tras una búsqueda).",
                        self.max_fetches_after_search
                    )));
                }
                self.fetches_after_search += 1;
            }
            _ => {}
        }
        self.external_calls += 1;
        Ok(())
    }
}

#[derive(Clone)]
struct CacheEntry {
    value: String,
    expires_at: Instant,
}

#[derive(Default)]
struct FreeToolsCache {
    entries: HashMap<String, CacheEntry>,
}

impl FreeToolsCache {
    fn get(&mut self, key: &str) -> Option<String> {
        let now = Instant::now();
        if let Some(entry) = self.entries.get(key) {
            if entry.expires_at > now {
                return Some(entry.value.clone());
            }
        }
        self.entries.remove(key);
        None
    }

    fn insert(&mut self, key: String, value: String, ttl: Duration) {
        if self.entries.len() > 64 {
            let now = Instant::now();
            self.entries.retain(|_, v| v.expires_at > now);
        }
        self.entries.insert(
            key,
            CacheEntry {
                value,
                expires_at: Instant::now() + ttl,
            },
        );
    }
}

pub fn current_period_key(reset_day: Option<u8>) -> Option<String> {
    let day = reset_day?;
    let now = Utc::now().date_naive();
    let clamped_day = u32::from(day.clamp(1, 28));
    // If today >= reset_day, current billing period started this month; else previous month.
    let (year, month) = if chrono::Datelike::day(&now) >= clamped_day {
        (chrono::Datelike::year(&now), chrono::Datelike::month(&now))
    } else if chrono::Datelike::month(&now) == 1 {
        (chrono::Datelike::year(&now) - 1, 12)
    } else {
        (
            chrono::Datelike::year(&now),
            chrono::Datelike::month(&now) - 1,
        )
    };
    Some(format!("{year:04}-{month:02}-d{clamped_day:02}"))
}

pub struct FreeToolsEngine {
    client: Client,
    db: Mutex<Database>,
    cache: Mutex<FreeToolsCache>,
    last_errors: Mutex<HashMap<String, String>>,
    #[cfg(test)]
    custom_search_providers: Mutex<HashMap<String, Arc<dyn SearchProvider>>>,
    #[cfg(test)]
    custom_exchange_url: Option<String>,
}

impl FreeToolsEngine {
    pub fn open_default() -> anyhow::Result<Arc<Self>> {
        let db = Database::open(&Database::default_db_path())?;
        Ok(Self::with_database(db))
    }

    pub fn with_database(db: Database) -> Arc<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(12))
            .user_agent("RustyDesk/0.3.0")
            .build()
            .unwrap_or_else(|_| Client::new());
        Arc::new(Self {
            client,
            db: Mutex::new(db),
            cache: Mutex::new(FreeToolsCache::default()),
            last_errors: Mutex::new(HashMap::new()),
            #[cfg(test)]
            custom_search_providers: Mutex::new(HashMap::new()),
            #[cfg(test)]
            custom_exchange_url: None,
        })
    }

    #[allow(dead_code)]
    pub fn open_with_path(path: PathBuf) -> anyhow::Result<Arc<Self>> {
        let db = Database::open(&path)?;
        Ok(Self::with_database(db))
    }

    #[cfg(test)]
    pub fn register_test_provider(&self, id: &str, provider: Arc<dyn SearchProvider>) {
        if let Ok(mut guard) = self.custom_search_providers.lock() {
            guard.insert(id.to_string(), provider);
        }
    }

    pub fn provider_usage(&self, provider_id: &str, cfg: &SearchProviderConfig) -> (u32, bool) {
        let Some(period) = current_period_key(cfg.reset_day) else {
            return (0, false);
        };
        let Ok(guard) = self.db.lock() else {
            return (0, false);
        };
        guard
            .sum_external_usage_for_period(provider_id, &period)
            .unwrap_or((0, false))
    }

    pub fn provider_status(
        &self,
        cfg: &ExternalToolsConfig,
        provider_id: SearchProviderId,
    ) -> (ProviderHealthStatus, u32, u32) {
        match provider_id {
            SearchProviderId::Tavily => {
                self.search_cfg_status(provider_id.as_str(), &cfg.tavily, false)
            }
            SearchProviderId::Brave => {
                self.search_cfg_status(provider_id.as_str(), &cfg.brave, true)
            }
            SearchProviderId::LangSearch => {
                self.search_cfg_status(provider_id.as_str(), &cfg.langsearch, false)
            }
            SearchProviderId::Searxng => {
                if !cfg.searxng.enabled {
                    (ProviderHealthStatus::Disabled, 0, 0)
                } else if cfg.searxng.base_url.trim().is_empty() {
                    (ProviderHealthStatus::NotConfigured, 0, 0)
                } else {
                    (ProviderHealthStatus::Available, 0, 0)
                }
            }
        }
    }

    fn search_cfg_status(
        &self,
        id: &str,
        cfg: &SearchProviderConfig,
        requires_reset_verification: bool,
    ) -> (ProviderHealthStatus, u32, u32) {
        if !cfg.enabled {
            return (ProviderHealthStatus::Disabled, 0, cfg.monthly_budget);
        }
        if cfg.api_key.trim().is_empty() {
            return (ProviderHealthStatus::NotConfigured, 0, cfg.monthly_budget);
        }
        if requires_reset_verification && cfg.reset_day.is_none() && !cfg.allow_paid_fallback {
            return (
                ProviderHealthStatus::NeedsVerification,
                0,
                cfg.monthly_budget,
            );
        }
        let (used, _) = self.provider_usage(id, cfg);
        if used >= cfg.monthly_budget {
            return (
                ProviderHealthStatus::QuotaExhausted,
                used,
                cfg.monthly_budget,
            );
        }
        if let Ok(errs) = self.last_errors.lock() {
            if errs.contains_key(id) {
                return (
                    ProviderHealthStatus::TemporaryError,
                    used,
                    cfg.monthly_budget,
                );
            }
        }
        (ProviderHealthStatus::Available, used, cfg.monthly_budget)
    }

    fn build_search_provider(
        &self,
        cfg: &ExternalToolsConfig,
        id: SearchProviderId,
    ) -> Arc<dyn SearchProvider> {
        #[cfg(test)]
        if let Ok(guard) = self.custom_search_providers.lock() {
            if let Some(p) = guard.get(id.as_str()) {
                return Arc::clone(p);
            }
        }
        match id {
            SearchProviderId::Tavily => Arc::new(TavilyProvider::new(
                self.client.clone(),
                &cfg.tavily.api_key,
            )),
            SearchProviderId::Brave => {
                Arc::new(BraveProvider::new(self.client.clone(), &cfg.brave.api_key))
            }
            SearchProviderId::LangSearch => Arc::new(LangSearchProvider::new(
                self.client.clone(),
                &cfg.langsearch.api_key,
            )),
            SearchProviderId::Searxng => Arc::new(search::SearxngProvider {
                client: self.client.clone(),
                base_url: cfg.searxng.base_url.clone(),
                timeout: Duration::from_secs(10),
            }),
        }
    }

    fn provider_order(&self, cfg: &ExternalToolsConfig) -> Vec<SearchProviderId> {
        match cfg.preferred_search_provider {
            SearchProviderId::Tavily => vec![
                SearchProviderId::Tavily,
                SearchProviderId::LangSearch,
                SearchProviderId::Brave,
            ],
            SearchProviderId::Brave => vec![
                SearchProviderId::Brave,
                SearchProviderId::LangSearch,
                SearchProviderId::Tavily,
            ],
            SearchProviderId::LangSearch => vec![
                SearchProviderId::LangSearch,
                SearchProviderId::Tavily,
                SearchProviderId::Brave,
            ],
            SearchProviderId::Searxng => vec![
                SearchProviderId::Searxng,
                SearchProviderId::Tavily,
                SearchProviderId::LangSearch,
                SearchProviderId::Brave,
            ],
        }
    }

    fn search_config_for(
        cfg: &ExternalToolsConfig,
        id: SearchProviderId,
    ) -> Option<&SearchProviderConfig> {
        match id {
            SearchProviderId::Tavily => Some(&cfg.tavily),
            SearchProviderId::Brave => Some(&cfg.brave),
            SearchProviderId::LangSearch => Some(&cfg.langsearch),
            SearchProviderId::Searxng => None,
        }
    }

    fn reserve_budget(
        &self,
        tool_name: &str,
        provider_id: SearchProviderId,
        cfg: &ExternalToolsConfig,
    ) -> Result<Option<String>, FreeToolError> {
        let Some(scfg) = Self::search_config_for(cfg, provider_id) else {
            return Ok(None);
        };
        if !scfg.enabled {
            return Err(FreeToolError::NotConfigured(format!(
                "{} está deshabilitado.",
                provider_id.label()
            )));
        }
        if scfg.api_key.trim().is_empty() {
            return Err(FreeToolError::NotConfigured(format!(
                "{} no tiene API key configurada.",
                provider_id.label()
            )));
        }
        let Some(period) = current_period_key(scfg.reset_day) else {
            return Err(FreeToolError::QuotaExceeded(format!(
                "{} requiere configurar el día de reinicio de cuota para evitar cargos.",
                provider_id.label()
            )));
        };
        let reservation_id = uuid::Uuid::new_v4().to_string();
        let reserved = {
            let mut db = self.db.lock().map_err(|_| {
                FreeToolError::ProviderUnavailable("Error interno de contabilidad.".to_string())
            })?;
            db.reserve_external_usage(
                &reservation_id,
                tool_name,
                provider_id.as_str(),
                &period,
                1,
                scfg.monthly_budget,
            )
            .map_err(|_| {
                FreeToolError::ProviderUnavailable("No se pudo registrar cuota local.".to_string())
            })?
        };
        if !reserved {
            return Err(FreeToolError::QuotaExceeded(format!(
                "{} alcanzó su presupuesto mensual ({} créditos estimados).",
                provider_id.label(),
                scfg.monthly_budget
            )));
        }
        Ok(Some(reservation_id))
    }

    fn finalize_budget(
        &self,
        reservation_id: Option<&str>,
        status: &str,
        confirmed_credits: Option<u32>,
        error_kind: Option<&str>,
    ) {
        let Some(id) = reservation_id else {
            return;
        };
        if let Ok(db) = self.db.lock() {
            let _ = db.finalize_external_usage(id, status, confirmed_credits, error_kind);
        }
    }

    /// Executes `web_search` with caching, atomic quota reservation, 1 transient retry, and safe fallback.
    pub async fn execute_web_search(
        &self,
        cfg: &ExternalToolsConfig,
        raw_query: &str,
        max_results: usize,
    ) -> Result<SearchResponse, FreeToolError> {
        let query = SearchQuery::new(raw_query, max_results)?;
        let cache_key = format!(
            "search:{}:{}:{}",
            cfg.preferred_search_provider.as_str(),
            query.max_results,
            query.query.to_lowercase()
        );
        if let Ok(mut cache) = self.cache.lock() {
            if let Some(cached) = cache.get(&cache_key) {
                if let Ok(results) = serde_json::from_str(&cached) {
                    return Ok(SearchResponse {
                        provider: cfg.preferred_search_provider.as_str().to_string(),
                        results,
                        elapsed_ms: 0,
                        confirmed_credits: Some(0),
                    });
                }
            }
        }

        let order = self.provider_order(cfg);
        let mut last_err: Option<FreeToolError> = None;

        for (idx, pid) in order.iter().copied().enumerate() {
            // Do not auto-fallback to Brave unless explicitly enabled & allowed without unexpected charges.
            if idx > 0
                && pid == SearchProviderId::Brave
                && (!cfg.brave.enabled
                    || (!cfg.brave.allow_paid_fallback && cfg.brave.reset_day.is_none()))
            {
                continue;
            }

            let reservation = match self.reserve_budget("web_search", pid, cfg) {
                Ok(r) => r,
                Err(e) => {
                    last_err = Some(e);
                    continue;
                }
            };

            let provider = self.build_search_provider(cfg, pid);
            let mut attempt = 0;
            let res = loop {
                attempt += 1;
                match provider.search(query.clone()).await {
                    Ok(ok) => break Ok(ok),
                    Err(err) if err.is_transient() && attempt == 1 => {
                        tokio::time::sleep(Duration::from_millis(250)).await;
                        continue;
                    }
                    Err(err) => break Err(err),
                }
            };

            match res {
                Ok(resp) => {
                    self.finalize_budget(
                        reservation.as_deref(),
                        "Success",
                        resp.confirmed_credits,
                        None,
                    );
                    if let Ok(mut errs) = self.last_errors.lock() {
                        errs.remove(pid.as_str());
                    }
                    if let Ok(json) = serde_json::to_string(&resp.results) {
                        if let Ok(mut cache) = self.cache.lock() {
                            cache.insert(cache_key, json, Duration::from_secs(300));
                        }
                    }
                    return Ok(resp);
                }
                Err(err) => {
                    // Refund local estimated credit if request failed before provider accepted it.
                    let refund = match err {
                        FreeToolError::RateLimited(_)
                        | FreeToolError::NotConfigured(_)
                        | FreeToolError::NetworkError(_)
                        | FreeToolError::Timeout(_) => Some(0),
                        _ => Some(0),
                    };
                    self.finalize_budget(
                        reservation.as_deref(),
                        "Failed",
                        refund,
                        Some(err.kind_code()),
                    );
                    if let Ok(mut errs) = self.last_errors.lock() {
                        errs.insert(pid.as_str().to_string(), err.kind_code().to_string());
                    }
                    let can_fallback = err.allows_fallback();
                    last_err = Some(err);
                    if !can_fallback {
                        break;
                    }
                }
            }
        }

        Err(last_err.unwrap_or_else(|| {
            FreeToolError::NotConfigured(
                "No pude realizar la búsqueda. Revisá la configuración de proveedores.".to_string(),
            )
        }))
    }

    pub async fn test_search_provider(
        &self,
        cfg: &ExternalToolsConfig,
        pid: SearchProviderId,
    ) -> Result<String, FreeToolError> {
        let q = SearchQuery::new("rust programming language", 1)?;
        let provider = self.build_search_provider(cfg, pid);
        let resp = provider.search(q).await?;
        Ok(format!(
            "{} respondió OK ({} resultado(s) en {} ms).",
            pid.label(),
            resp.results.len(),
            resp.elapsed_ms
        ))
    }

    pub async fn execute_web_fetch(
        &self,
        cfg: &ExternalToolsConfig,
        url: &str,
    ) -> Result<String, FreeToolError> {
        if !cfg.web_fetch_enabled {
            return Err(FreeToolError::NotConfigured(
                "La lectura web (`web_fetch`) está deshabilitada en Configuración.".to_string(),
            ));
        }
        let fetched = fetch_url_safe(url, cfg.web_fetch_max_chars).await?;
        Ok(format_untrusted_fetch_for_llm(&fetched))
    }

    pub async fn execute_weather(
        &self,
        cfg: &ExternalToolsConfig,
        location: &str,
    ) -> Result<String, FreeToolError> {
        if !cfg.weather_enabled {
            return Err(FreeToolError::NotConfigured(
                "La consulta de clima (Open-Meteo) está deshabilitada.".to_string(),
            ));
        }
        let loc = location.trim();
        if loc.is_empty() {
            return Err(FreeToolError::InvalidRequest(
                "Indicá una ciudad o ubicación para consultar el clima.".to_string(),
            ));
        }
        let cache_key = format!("weather:{}", loc.to_lowercase());
        if let Ok(mut cache) = self.cache.lock() {
            if let Some(cached) = cache.get(&cache_key) {
                return Ok(cached);
            }
        }
        let report = crate::weather::fetch_weather(loc)
            .await
            .map_err(FreeToolError::ProviderUnavailable)?;
        let formatted = format!(
            "{}\nConsultado: {} UTC (Open-Meteo)",
            report.format_message(),
            Utc::now().format("%Y-%m-%d %H:%M")
        );
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(cache_key, formatted.clone(), Duration::from_secs(600));
        }
        Ok(formatted)
    }

    pub async fn execute_exchange_rate(
        &self,
        cfg: &ExternalToolsConfig,
        base: &str,
        target: &str,
        amount: f64,
    ) -> Result<String, FreeToolError> {
        if !cfg.exchange_enabled {
            return Err(FreeToolError::NotConfigured(
                "La consulta de cotizaciones (Frankfurter) está deshabilitada.".to_string(),
            ));
        }
        let cache_key = format!(
            "fx:{}:{}:{:.4}",
            base.trim().to_ascii_uppercase(),
            target.trim().to_ascii_uppercase(),
            amount
        );
        if let Ok(mut cache) = self.cache.lock() {
            if let Some(cached) = cache.get(&cache_key) {
                return Ok(cached);
            }
        }
        let provider = {
            #[cfg(test)]
            if let Some(url) = &self.custom_exchange_url {
                FrankfurterProvider::with_base_url(self.client.clone(), url)
            } else {
                FrankfurterProvider::new(self.client.clone())
            }
            #[cfg(not(test))]
            FrankfurterProvider::new(self.client.clone())
        };
        let res = provider.convert(base, target, amount).await?;
        let formatted = res.format_message();
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(cache_key, formatted.clone(), Duration::from_secs(1800));
        }
        Ok(formatted)
    }
}

pub fn format_search_response_for_user_and_llm(resp: &SearchResponse) -> String {
    if resp.results.is_empty() {
        return format!(
            "No encontré resultados para la búsqueda (proveedor: {}).",
            resp.provider
        );
    }
    let mut lines = vec![format!(
        "✓ Encontré {} resultado(s) vía {} ({} ms):",
        resp.results.len(),
        resp.provider,
        resp.elapsed_ms
    )];
    for (i, r) in resp.results.iter().enumerate() {
        let date_suffix = r
            .published_at
            .as_deref()
            .map(|d| format!(" · {d}"))
            .unwrap_or_default();
        lines.push(format!(
            "{}. [{}]({}){}\n   {}",
            i + 1,
            r.title,
            r.url,
            date_suffix,
            r.snippet
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use search::SearchResult;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    struct StubSearchProvider {
        id: &'static str,
        calls: AtomicUsize,
        mode: StubMode,
    }

    #[derive(Clone, Copy)]
    enum StubMode {
        Ok,
        RateLimited,
        Invalid,
    }

    #[async_trait]
    impl SearchProvider for StubSearchProvider {
        fn id(&self) -> &'static str {
            self.id
        }

        async fn search(&self, query: SearchQuery) -> Result<SearchResponse, FreeToolError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match self.mode {
                StubMode::Ok => Ok(SearchResponse {
                    provider: self.id.to_string(),
                    results: vec![SearchResult {
                        title: format!("Result for {}", query.query),
                        url: "https://example.com".to_string(),
                        snippet: "Snippet".to_string(),
                        source: self.id.to_string(),
                        published_at: None,
                    }],
                    elapsed_ms: 12,
                    confirmed_credits: Some(1),
                }),
                StubMode::RateLimited => Err(FreeToolError::RateLimited("429".into())),
                StubMode::Invalid => Err(FreeToolError::InvalidRequest("bad query".into())),
            }
        }
    }

    #[tokio::test]
    async fn selector_falls_back_on_rate_limit_and_tracks_budget() {
        let db = Database::open_in_memory().unwrap();
        let engine = FreeToolsEngine::with_database(db);

        let tavily = Arc::new(StubSearchProvider {
            id: "tavily",
            calls: AtomicUsize::new(0),
            mode: StubMode::RateLimited,
        });
        let brave = Arc::new(StubSearchProvider {
            id: "brave",
            calls: AtomicUsize::new(0),
            mode: StubMode::Ok,
        });
        engine.register_test_provider("tavily", tavily.clone());
        engine.register_test_provider("brave", brave.clone());

        let mut cfg = ExternalToolsConfig::default();
        cfg.tavily.api_key = "tvly-test".into();
        cfg.brave.enabled = true;
        cfg.brave.api_key = "brave-test".into();
        cfg.brave.reset_day = Some(1);
        cfg.brave.allow_paid_fallback = true;

        let resp = engine
            .execute_web_search(&cfg, "rust 2026", 3)
            .await
            .unwrap();
        assert_eq!(resp.provider, "brave");
        assert_eq!(tavily.calls.load(Ordering::SeqCst), 1);
        assert_eq!(brave.calls.load(Ordering::SeqCst), 1);

        // Tavily failed -> refunded (0 used); Brave succeeded -> 1 used
        assert_eq!(engine.provider_usage("tavily", &cfg.tavily).0, 0);
        assert_eq!(engine.provider_usage("brave", &cfg.brave).0, 1);
    }

    #[tokio::test]
    async fn concurrent_reservations_never_exceed_budget() {
        let db = Database::open_in_memory().unwrap();
        let engine = FreeToolsEngine::with_database(db);

        let tavily = Arc::new(StubSearchProvider {
            id: "tavily",
            calls: AtomicUsize::new(0),
            mode: StubMode::Ok,
        });
        engine.register_test_provider("tavily", tavily.clone());

        let mut cfg = ExternalToolsConfig::default();
        cfg.tavily.api_key = "tvly-test".into();
        cfg.tavily.monthly_budget = 2;
        cfg.tavily.reset_day = Some(1);

        let mut handles = Vec::new();
        for i in 0..5 {
            let eng = Arc::clone(&engine);
            let c = cfg.clone();
            handles.push(tokio::spawn(async move {
                eng.execute_web_search(&c, &format!("query {i}"), 2).await
            }));
        }

        let mut ok_count = 0;
        let mut quota_err_count = 0;
        for h in handles {
            match h.await.unwrap() {
                Ok(_) => ok_count += 1,
                Err(FreeToolError::QuotaExceeded(_)) => quota_err_count += 1,
                Err(e) => panic!("unexpected error: {e}"),
            }
        }
        assert_eq!(ok_count, 2);
        assert_eq!(quota_err_count, 3);
        assert_eq!(engine.provider_usage("tavily", &cfg.tavily).0, 2);
    }

    #[tokio::test]
    async fn invalid_request_does_not_fallback_to_alternative_provider() {
        let db = Database::open_in_memory().unwrap();
        let engine = FreeToolsEngine::with_database(db);
        let tavily = Arc::new(StubSearchProvider {
            id: "tavily",
            calls: AtomicUsize::new(0),
            mode: StubMode::Invalid,
        });
        let brave = Arc::new(StubSearchProvider {
            id: "brave",
            calls: AtomicUsize::new(0),
            mode: StubMode::Ok,
        });
        engine.register_test_provider("tavily", tavily);
        engine.register_test_provider("brave", brave.clone());

        let mut cfg = ExternalToolsConfig::default();
        cfg.tavily.api_key = "tvly-test".into();
        cfg.brave.enabled = true;
        cfg.brave.api_key = "brave-test".into();
        cfg.brave.reset_day = Some(1);
        cfg.brave.allow_paid_fallback = true;

        let err = engine
            .execute_web_search(&cfg, "valid query", 3)
            .await
            .unwrap_err();
        assert!(matches!(err, FreeToolError::InvalidRequest(_)));
        assert_eq!(brave.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn mock_http_server_tests_tavily_429_and_fetch_size_limit() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            for _ in 0..2 {
                if let Ok((mut socket, _)) = listener.accept().await {
                    let mut req_buf = [0u8; 1024];
                    let _ = socket.read(&mut req_buf).await;
                    let req = String::from_utf8_lossy(&req_buf);
                    if req.contains("/tavily-429") {
                        let resp = "HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\n\r\n";
                        let _ = socket.write_all(resp.as_bytes()).await;
                    } else {
                        let big_body = "a".repeat(fetch::MAX_DOWNLOAD_BYTES + 100);
                        let header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n",
                            big_body.len()
                        );
                        let _ = socket.write_all(header.as_bytes()).await;
                        let _ = socket.write_all(big_body.as_bytes()).await;
                    }
                }
            }
        });

        let provider = TavilyProvider::with_base_url(
            Client::new(),
            "key",
            format!("http://{addr}/tavily-429"),
        );
        let err = provider
            .search(SearchQuery::new("rust", 3).unwrap())
            .await
            .unwrap_err();
        assert!(matches!(err, FreeToolError::RateLimited(_)));

        let fetch_err =
            fetch::fetch_url_safe_with_options(&format!("http://{addr}/huge"), 2000, false)
                .await
                .unwrap_err();
        assert!(matches!(fetch_err, FreeToolError::InvalidRequest(_)));
    }

    #[test]
    fn turn_external_limits_enforces_3_calls_2_searches_1_fetch() {
        let mut lim = TurnExternalLimits::default();
        assert!(lim.try_acquire("web_search").is_ok());
        assert!(lim.try_acquire("web_search").is_ok());
        assert!(matches!(
            lim.try_acquire("web_search"),
            Err(FreeToolError::QuotaExceeded(_))
        ));
        assert!(lim.try_acquire("web_fetch").is_ok());
        assert!(matches!(
            lim.try_acquire("get_weather"),
            Err(FreeToolError::QuotaExceeded(_))
        ));
    }
}
