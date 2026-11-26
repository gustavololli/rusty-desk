//! One short title per chat, after the first user/assistant exchange.
//!
//! Dreaming availability reuses the memory worker's gate (`enabled` plus
//! `dreaming::validate_config`). A short `GET /models` probe then checks that
//! the server answers; a timeout, a connection error or a non-2xx status falls
//! back to the chat provider. The title itself is one tool-free completion.

use crate::ai::{AiProvider, ChatMessage};
use crate::config::DreamingConfig;
use crate::dreaming::validate_config;
use anyhow::anyhow;
use std::sync::Arc;
use std::time::Duration;

pub const PROBE_TIMEOUT: Duration = Duration::from_millis(800);
pub const TITLE_MAX_TOKENS: u32 = 24;
const COMPLETION_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_TITLE_WORDS: usize = 6;
const MAX_TITLE_CHARS: usize = 60;
const EXCERPT_CHARS: usize = 500;

const TITLE_SYSTEM_PROMPT: &str = "\
Respondé solo con un título de 3 a 6 palabras, en el idioma del mensaje del usuario. \
Sin comillas, sin punto final y sin markdown.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleProviderKind {
    Dreaming,
    ActiveChat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleTrigger {
    pub user_text_count: usize,
    pub assistant_text_count: usize,
    pub auto_title: Option<String>,
    pub title_custom: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingTitle {
    pub conversation_id: String,
    pub user_text: String,
    pub assistant_text: String,
}

/// History label: a generated title when we have one, otherwise the stored fallback.
pub fn history_label<'a>(
    fallback: &'a str,
    auto_title: Option<&'a str>,
    title_custom: bool,
) -> &'a str {
    if title_custom {
        return fallback;
    }
    auto_title
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .unwrap_or(fallback)
}

/// True only for the first completed text exchange, and never again.
pub fn should_request_title(trigger: &TitleTrigger) -> bool {
    if trigger.title_custom || trigger.auto_title.is_some() {
        return false;
    }
    trigger.user_text_count == 1 && trigger.assistant_text_count == 1
}

pub fn sanitize_title(raw: &str) -> Option<String> {
    let line = raw.lines().next()?;
    let stripped: String = line
        .chars()
        .filter(|c| {
            !matches!(
                c,
                '*' | '#' | '`' | '"' | '\'' | '«' | '»' | '“' | '”' | '‘' | '’'
            )
        })
        .collect();
    let words: Vec<&str> = stripped.split_whitespace().collect();
    if words.is_empty() {
        return None;
    }
    let mut title = words
        .into_iter()
        .take(MAX_TITLE_WORDS)
        .collect::<Vec<_>>()
        .join(" ");
    while title
        .chars()
        .last()
        .is_some_and(|ch| matches!(ch, '.' | '。' | ',' | ';' | ':' | ' '))
    {
        title.pop();
    }
    title = title.trim().to_string();
    if title.chars().count() > MAX_TITLE_CHARS {
        let cut: String = title.chars().take(MAX_TITLE_CHARS).collect();
        title = cut
            .rsplit_once(' ')
            .map(|(head, _)| head.to_string())
            .unwrap_or(cut);
        title = title.trim().to_string();
    }
    if title.is_empty() {
        None
    } else {
        Some(title)
    }
}

pub async fn select_title_provider(
    dreaming: &DreamingConfig,
    probe_timeout: Duration,
) -> TitleProviderKind {
    if dreaming_reachable(dreaming, probe_timeout).await {
        TitleProviderKind::Dreaming
    } else {
        TitleProviderKind::ActiveChat
    }
}

async fn dreaming_reachable(dreaming: &DreamingConfig, probe_timeout: Duration) -> bool {
    if !dreaming.enabled || validate_config(dreaming).is_err() {
        return false;
    }
    probe_models(&dreaming.base_url, probe_timeout).await
}

async fn probe_models(base_url: &str, probe_timeout: Duration) -> bool {
    let Ok(client) = reqwest::Client::builder()
        .timeout(probe_timeout)
        .connect_timeout(probe_timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
    else {
        return false;
    };
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    match client.get(url).send().await {
        Ok(response) => response.error_for_status().is_ok(),
        Err(_) => false,
    }
}

/// One completion. `None` means keep the truncated first message.
pub async fn generate_chat_title(
    dreaming: &DreamingConfig,
    active: &dyn AiProvider,
    user_text: &str,
    assistant_text: &str,
    probe_timeout: Duration,
) -> Option<String> {
    let result = match select_title_provider(dreaming, probe_timeout).await {
        TitleProviderKind::Dreaming => {
            let provider = dreaming_title_provider(dreaming);
            request_title(provider.as_ref(), user_text, assistant_text).await
        }
        TitleProviderKind::ActiveChat => request_title(active, user_text, assistant_text).await,
    };
    match result {
        Ok(title) => Some(title),
        Err(err) => {
            eprintln!("Título automático omitido: {err:#}");
            None
        }
    }
}

async fn request_title(
    provider: &dyn AiProvider,
    user_text: &str,
    assistant_text: &str,
) -> anyhow::Result<String> {
    let messages = [
        ChatMessage::system(TITLE_SYSTEM_PROMPT),
        ChatMessage::user(format!(
            "Usuario:\n{}\n\nAsistente:\n{}",
            excerpt(user_text),
            excerpt(assistant_text)
        )),
    ];
    let response = match tokio::time::timeout(
        COMPLETION_TIMEOUT,
        provider.chat_brief(&messages, TITLE_MAX_TOKENS),
    )
    .await
    {
        Ok(Ok(response)) => response,
        Ok(Err(err)) => return Err(err),
        Err(_) => return Err(anyhow!("el modelo no respondió a tiempo")),
    };
    let raw = response.content.unwrap_or_default();
    sanitize_title(&raw).ok_or_else(|| anyhow!("el modelo no devolvió un título usable"))
}

fn excerpt(text: &str) -> String {
    text.chars().take(EXCERPT_CHARS).collect()
}

fn dreaming_title_provider(cfg: &DreamingConfig) -> Arc<dyn AiProvider> {
    if cfg.base_url.contains("generativelanguage.googleapis.com") {
        Arc::new(crate::ai::GoogleGeminiProvider::new(
            cfg.base_url
                .trim_end_matches('/')
                .trim_end_matches("/openai")
                .to_owned(),
            cfg.api_key.clone(),
            cfg.model.clone(),
        ))
    } else {
        Arc::new(crate::ai::OpenAiCompatibleProvider::new_with_timeout(
            cfg.base_url.clone(),
            cfg.api_key.clone(),
            cfg.model.clone(),
            String::new(),
            COMPLETION_TIMEOUT.as_secs(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{AiResponse, ToolDef};
    use async_trait::async_trait;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use std::time::Instant;

    #[test]
    fn sanitize_strips_quotes_markdown_and_the_trailing_period() {
        assert_eq!(
            sanitize_title("  \"Música en casa.\"  ").as_deref(),
            Some("Música en casa")
        );
        assert_eq!(
            sanitize_title("**Playlist local**").as_deref(),
            Some("Playlist local")
        );
        assert_eq!(sanitize_title("`código`").as_deref(), Some("código"));
        assert_eq!(
            sanitize_title("# Título\nsegunda línea").as_deref(),
            Some("Título")
        );
        assert_eq!(
            sanitize_title("«Hola mundo»").as_deref(),
            Some("Hola mundo")
        );
    }

    #[test]
    fn sanitize_limits_length_and_drops_empty_output() {
        assert_eq!(
            sanitize_title("uno dos tres cuatro cinco seis siete").as_deref(),
            Some("uno dos tres cuatro cinco seis")
        );
        let long =
            "aaaaaaaaaaaaaaaaaaaa bbbbbbbbbbbbbbbbbbbb cccccccccccccccccccc dddddddddddddddddddd";
        let limited = sanitize_title(long).unwrap();
        assert!(limited.chars().count() <= MAX_TITLE_CHARS);
        assert!(limited.starts_with("aaaaaaaaaaaaaaaaaaaa"));
        assert!(limited.split_whitespace().count() <= MAX_TITLE_WORDS);
        assert_eq!(sanitize_title("   "), None);
        assert_eq!(sanitize_title("\"\""), None);
        assert_eq!(sanitize_title("..."), None);
        assert_eq!(sanitize_title("```"), None);
    }

    #[test]
    fn history_falls_back_when_there_is_no_usable_title() {
        assert_eq!(history_label("pone musica", None, false), "pone musica");
        assert_eq!(
            history_label("pone musica", Some("   "), false),
            "pone musica"
        );
        assert_eq!(history_label("pone musica", Some(""), false), "pone musica");
        assert_eq!(
            history_label("pone musica", Some("Música en casa"), false),
            "Música en casa"
        );
        assert_eq!(
            history_label("Editado a mano", Some("Música en casa"), true),
            "Editado a mano"
        );
    }

    #[test]
    fn title_is_requested_once_after_the_first_exchange() {
        let fresh = TitleTrigger {
            user_text_count: 1,
            assistant_text_count: 1,
            auto_title: None,
            title_custom: false,
        };
        assert!(should_request_title(&fresh));
        assert!(!should_request_title(&TitleTrigger {
            user_text_count: 1,
            assistant_text_count: 0,
            ..fresh.clone()
        }));
        assert!(!should_request_title(&TitleTrigger {
            user_text_count: 2,
            assistant_text_count: 2,
            ..fresh.clone()
        }));
        assert!(!should_request_title(&TitleTrigger {
            auto_title: Some(String::new()),
            ..fresh.clone()
        }));
        assert!(!should_request_title(&TitleTrigger {
            auto_title: Some("Música en casa".into()),
            ..fresh.clone()
        }));
        assert!(!should_request_title(&TitleTrigger {
            title_custom: true,
            ..fresh
        }));
    }

    #[tokio::test]
    async fn dreaming_is_used_when_the_probe_answers() {
        let seen = std::sync::Arc::new(Mutex::new(Vec::new()));
        let base = spawn_server(Arc::clone(&seen), ServerMode::Reply);
        let dreaming = DreamingConfig {
            enabled: true,
            base_url: base,
            model: "local-model".into(),
            ..DreamingConfig::default()
        };
        let active = ScriptedProvider::reply("No deberías llamarme");
        let title = generate_chat_title(
            &dreaming,
            &active,
            "pone musica",
            "Reproduciendo la playlist de estudio.",
            Duration::from_millis(500),
        )
        .await;
        assert_eq!(title.as_deref(), Some("Música en casa"));
        assert_eq!(active.calls(), 0);
        let requests = seen.lock().expect("requests");
        assert_eq!(requests.len(), 2, "{requests:?}");
        assert!(requests[0].starts_with("GET "), "{requests:?}");
        assert!(requests[0].contains("/models"), "{requests:?}");
        assert!(requests[1].starts_with("POST "), "{requests:?}");
        assert!(requests[1].contains("/chat/completions"), "{requests:?}");
        assert!(
            requests[1].contains(&format!("\"max_tokens\":{TITLE_MAX_TOKENS}")),
            "{requests:?}"
        );
        assert!(!requests[1].contains("\"tools\""), "{requests:?}");
    }

    #[tokio::test]
    async fn active_provider_is_used_when_dreaming_is_down() {
        let dreaming = DreamingConfig {
            enabled: true,
            base_url: closed_port_url(),
            model: "local-model".into(),
            ..DreamingConfig::default()
        };
        let active = ScriptedProvider::reply("\"Clima local.\"");
        let title = generate_chat_title(
            &dreaming,
            &active,
            "¿qué clima hace?",
            "22 grados y soleado.",
            Duration::from_millis(300),
        )
        .await;
        assert_eq!(title.as_deref(), Some("Clima local"));
        assert_eq!(active.calls(), 1);
        assert_eq!(active.tools_seen(), 0);
    }

    #[tokio::test]
    async fn probe_timeout_falls_back_without_waiting() {
        let seen = std::sync::Arc::new(Mutex::new(Vec::new()));
        let base = spawn_server(Arc::clone(&seen), ServerMode::Hang);
        let dreaming = DreamingConfig {
            enabled: true,
            base_url: base,
            model: "local-model".into(),
            ..DreamingConfig::default()
        };
        let started = Instant::now();
        let kind = select_title_provider(&dreaming, Duration::from_millis(120)).await;
        assert_eq!(kind, TitleProviderKind::ActiveChat);
        assert!(started.elapsed() < Duration::from_millis(800));
        assert!(!seen.lock().expect("requests").is_empty());
    }

    #[tokio::test]
    async fn unconfigured_dreaming_is_not_probed() {
        let seen = std::sync::Arc::new(Mutex::new(Vec::new()));
        let base = spawn_server(Arc::clone(&seen), ServerMode::Hang);
        let disabled = DreamingConfig {
            enabled: false,
            base_url: base.clone(),
            model: "local-model".into(),
            ..DreamingConfig::default()
        };
        let started = Instant::now();
        let kind = tokio::time::timeout(
            Duration::from_millis(200),
            select_title_provider(&disabled, Duration::from_secs(5)),
        )
        .await
        .expect("disabled dreaming must not probe");
        assert_eq!(kind, TitleProviderKind::ActiveChat);
        assert!(started.elapsed() < Duration::from_millis(200));

        let empty_model = DreamingConfig {
            enabled: true,
            base_url: base,
            model: "  ".into(),
            ..DreamingConfig::default()
        };
        let kind = tokio::time::timeout(
            Duration::from_millis(200),
            select_title_provider(&empty_model, Duration::from_secs(5)),
        )
        .await
        .expect("an empty model must not probe");
        assert_eq!(kind, TitleProviderKind::ActiveChat);
        assert!(seen.lock().expect("requests").is_empty());
    }

    #[tokio::test]
    async fn failed_generation_keeps_the_fallback() {
        let dreaming = DreamingConfig::default();
        let active = ScriptedProvider::fail();
        let title = generate_chat_title(
            &dreaming,
            &active,
            "pone musica",
            "Listo.",
            Duration::from_millis(50),
        )
        .await;
        assert_eq!(title, None);
        assert_eq!(active.calls(), 1);
        assert_eq!(
            history_label("pone musica", title.as_deref(), false),
            "pone musica"
        );

        let blank = ScriptedProvider::reply("   \"\"  ");
        let title = generate_chat_title(
            &dreaming,
            &blank,
            "pone musica",
            "Listo.",
            Duration::from_millis(50),
        )
        .await;
        assert_eq!(title, None);
        assert_eq!(history_label("pone musica", Some(""), false), "pone musica");
    }

    struct ScriptedProvider {
        calls: AtomicUsize,
        tools_seen: AtomicUsize,
        reply: Option<String>,
        fail: bool,
    }

    impl ScriptedProvider {
        fn reply(text: &str) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                tools_seen: AtomicUsize::new(0),
                reply: Some(text.to_string()),
                fail: false,
            }
        }

        fn fail() -> Self {
            Self {
                calls: AtomicUsize::new(0),
                tools_seen: AtomicUsize::new(0),
                reply: None,
                fail: true,
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }

        fn tools_seen(&self) -> usize {
            self.tools_seen.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl AiProvider for ScriptedProvider {
        async fn chat(
            &self,
            _messages: &[ChatMessage],
            tools: &[ToolDef],
        ) -> anyhow::Result<AiResponse> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.tools_seen.fetch_add(tools.len(), Ordering::SeqCst);
            if self.fail {
                anyhow::bail!("sin modelo");
            }
            Ok(AiResponse::from_parts(self.reply.clone(), Vec::new(), None))
        }

        async fn transcribe(&self, _wav_bytes: Vec<u8>) -> anyhow::Result<String> {
            anyhow::bail!("sin transcripción")
        }
    }

    #[derive(Clone, Copy)]
    enum ServerMode {
        Reply,
        Hang,
    }

    fn spawn_server(seen: Arc<Mutex<Vec<String>>>, mode: ServerMode) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        std::thread::spawn(move || {
            for _ in 0..4 {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                let request = read_http_request(&mut stream);
                if !request.is_empty() {
                    if let Ok(mut guard) = seen.lock() {
                        guard.push(request.clone());
                    }
                }
                match mode {
                    ServerMode::Hang => {
                        std::thread::sleep(Duration::from_secs(3));
                    }
                    ServerMode::Reply => {
                        let body = if request.starts_with("GET") {
                            r#"{"data":[]}"#
                        } else {
                            r#"{"choices":[{"message":{"content":"\"Música en casa.\""}}]}"#
                        };
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = stream.write_all(response.as_bytes());
                    }
                }
            }
        });
        format!("http://{addr}/v1")
    }

    fn read_http_request(stream: &mut TcpStream) -> String {
        stream
            .set_read_timeout(Some(Duration::from_millis(400)))
            .ok();
        let mut buf = Vec::new();
        let mut tmp = [0u8; 1024];
        loop {
            match stream.read(&mut tmp) {
                Ok(0) => break,
                Ok(n) => {
                    buf.extend_from_slice(&tmp[..n]);
                    if let Some(header_end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&buf[..header_end]);
                        let content_length = header
                            .lines()
                            .find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                if name.eq_ignore_ascii_case("content-length") {
                                    value.trim().parse::<usize>().ok()
                                } else {
                                    None
                                }
                            })
                            .unwrap_or(0);
                        if buf.len() >= header_end + 4 + content_length {
                            break;
                        }
                    }
                }
                Err(_) => break,
            }
        }
        String::from_utf8_lossy(&buf).into_owned()
    }

    fn closed_port_url() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        drop(listener);
        format!("http://{addr}/v1")
    }
}
