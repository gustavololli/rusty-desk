use async_trait::async_trait;
use reqwest::multipart;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: ToolFunctionCall,
    #[serde(skip)]
    pub thought_signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip)]
    pub raw_gemini_parts: Option<Vec<Value>>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            raw_gemini_parts: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            raw_gemini_parts: None,
        }
    }

    pub fn assistant_text(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            raw_gemini_parts: None,
        }
    }

    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".to_string(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
            raw_gemini_parts: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolDef {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub function: ToolFunctionDef,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolFunctionDef {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeToolSupport {
    /// The API accepts official tool schemas (Gemini `functionDeclarations` or OpenAI `tools`).
    Supported,
    /// An OpenAI-compatible server might accept `tools` (Ollama, llama.cpp, LM Studio). Try once.
    Probe,
    /// Never send tools. Never parse assistant text into tool calls.
    Unsupported,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub total: u64,
}

impl TokenUsage {
    pub fn accumulate(current: &mut Option<Self>, reported: Self) {
        let total = current.get_or_insert_with(Self::default);
        total.input = total.input.saturating_add(reported.input);
        total.output = total.output.saturating_add(reported.output);
        total.total = total.total.saturating_add(reported.total);
    }

    fn from_openai(usage: &Value) -> Option<Self> {
        Some(Self {
            input: usage["prompt_tokens"].as_u64()?,
            output: usage["completion_tokens"].as_u64()?,
            total: usage["total_tokens"].as_u64()?,
        })
    }

    fn from_gemini(usage: &Value) -> Option<Self> {
        Some(Self {
            input: usage["promptTokenCount"]
                .as_u64()?
                .saturating_add(usage["toolUsePromptTokenCount"].as_u64().unwrap_or(0)),
            output: usage["candidatesTokenCount"]
                .as_u64()
                .unwrap_or(0)
                .saturating_add(usage["thoughtsTokenCount"].as_u64().unwrap_or(0)),
            total: usage["totalTokenCount"].as_u64()?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct AiResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub raw_gemini_parts: Option<Vec<Value>>,
    pub usage: Option<TokenUsage>,
    /// The endpoint rejected a native tool payload. The text, if any, was not parsed into calls.
    pub tools_rejected: bool,
}

impl AiResponse {
    pub fn from_parts(
        content: Option<String>,
        tool_calls: Vec<ToolCall>,
        raw_gemini_parts: Option<Vec<Value>>,
    ) -> Self {
        Self {
            content,
            tool_calls,
            raw_gemini_parts,
            usage: None,
            tools_rejected: false,
        }
    }
}

#[async_trait]
pub trait AiProvider: Send + Sync {
    fn native_tool_support(&self) -> NativeToolSupport {
        NativeToolSupport::Probe
    }

    async fn chat(&self, messages: &[ChatMessage], tools: &[ToolDef])
        -> anyhow::Result<AiResponse>;

    async fn transcribe(&self, wav_bytes: Vec<u8>) -> anyhow::Result<String>;
}

/// How a turn should present tools to the provider. `Off` and `Unsupported` send no schemas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolDispatch {
    Native,
    Unavailable,
}

pub fn resolve_tool_dispatch(mode_off: bool, support: NativeToolSupport) -> ToolDispatch {
    if mode_off || support == NativeToolSupport::Unsupported {
        ToolDispatch::Unavailable
    } else {
        ToolDispatch::Native
    }
}

/// Keep each tool batch contiguous and in request order, including after reopening
/// an older or interrupted conversation.
fn ordered_tool_history(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    let mut history = Vec::with_capacity(messages.len());
    let mut remaining = messages.iter().peekable();
    while let Some(message) = remaining.next() {
        if message.role == "tool" {
            continue;
        }
        history.push(message.clone());
        let Some(calls) = &message.tool_calls else {
            continue;
        };
        let mut results = std::collections::HashMap::new();
        while let Some(result) = remaining.next_if(|result| result.role == "tool") {
            if let Some(id) = &result.tool_call_id {
                results.entry(id.as_str()).or_insert(result);
            }
        }
        for call in calls {
            let result = results
                .get(call.id.as_str())
                .map(|result| (*result).clone());
            history.push(result.unwrap_or_else(|| {
                ChatMessage::tool_result(
                    &call.id,
                    "La conversación se interrumpió antes de registrar un resultado para esta llamada.",
                )
            }));
        }
    }
    history
}

pub fn build_openai_chat_request(
    model: &str,
    messages: &[ChatMessage],
    tools: &[ToolDef],
) -> Value {
    let mut body = json!({
        "model": model,
        "messages": ordered_tool_history(messages),
    });
    if !tools.is_empty() {
        body["tools"] = serde_json::to_value(tools).unwrap_or_else(|_| json!([]));
        body["tool_choice"] = json!("auto");
    }
    body
}

pub fn parse_openai_chat_response(text: &str) -> anyhow::Result<AiResponse> {
    let parsed: ApiChatResponse = serde_json::from_str(text)
        .map_err(|e| anyhow::anyhow!("Respuesta JSON inválida: {e} — {text}"))?;
    let first = parsed
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("La API no devolvió choices"))?;
    let mut response = AiResponse::from_parts(
        first.message.content,
        first.message.tool_calls.unwrap_or_default(),
        None,
    );
    response.usage = TokenUsage::from_openai(&parsed.usage);
    Ok(response)
}

pub fn build_gemini_request(messages: &[ChatMessage], tools: &[ToolDef]) -> Value {
    let system_text: String = messages
        .iter()
        .filter(|m| m.role == "system")
        .filter_map(|m| m.content.as_deref())
        .collect::<Vec<_>>()
        .join("\n");

    let history = ordered_tool_history(messages);
    let mut contents: Vec<Value> = Vec::new();
    let mut active_calls: &[ToolCall] = &[];
    let mut response_parts = Vec::new();
    for msg in history.iter().filter(|m| m.role != "system") {
        if msg.role != "tool" && !response_parts.is_empty() {
            contents.push(json!({ "role": "user", "parts": std::mem::take(&mut response_parts) }));
        }
        match msg.role.as_str() {
            "user" => {
                if let Some(text) = &msg.content {
                    contents.push(json!({
                        "role": "user",
                        "parts": [{ "text": text }]
                    }));
                }
            }
            "assistant" => {
                active_calls = msg.tool_calls.as_deref().unwrap_or_default();
                if let Some(raw_parts) = &msg.raw_gemini_parts {
                    contents.push(json!({ "role": "model", "parts": raw_parts }));
                } else {
                    let mut parts = Vec::new();
                    if let Some(text) = &msg.content {
                        if !text.is_empty() {
                            parts.push(json!({ "text": text }));
                        }
                    }
                    if let Some(tcs) = &msg.tool_calls {
                        for tc in tcs {
                            let args_val: Value =
                                serde_json::from_str(&tc.function.arguments).unwrap_or(json!({}));
                            let mut part_obj = json!({
                                "functionCall": {
                                    "name": tc.function.name,
                                    "args": args_val
                                }
                            });
                            if !tc.id.is_empty() && !tc.id.starts_with("gemini_") {
                                part_obj["functionCall"]["id"] = json!(tc.id);
                            }
                            if let Some(sig) = &tc.thought_signature {
                                part_obj["thoughtSignature"] = json!(sig);
                            }
                            parts.push(part_obj);
                        }
                    }
                    if !parts.is_empty() {
                        contents.push(json!({ "role": "model", "parts": parts }));
                    }
                }
            }
            "tool" => {
                let call_id = msg.tool_call_id.as_deref().unwrap_or("");
                let fn_name = active_calls
                    .iter()
                    .find(|tc| tc.id == call_id)
                    .map(|tc| tc.function.name.as_str())
                    .unwrap_or("tool");
                let result_str = msg.content.as_deref().unwrap_or("");
                let mut response_part = json!({
                    "functionResponse": {
                        "name": fn_name,
                        "response": { "result": result_str }
                    }
                });
                // Echo an id only when the API actually issued one. Synthetic ids stay local.
                if !call_id.is_empty() && !call_id.starts_with("gemini_") {
                    response_part["functionResponse"]["id"] = json!(call_id);
                }
                response_parts.push(response_part);
            }
            _ => {}
        }
    }

    if !response_parts.is_empty() {
        contents.push(json!({ "role": "user", "parts": response_parts }));
    }
    let mut body = json!({ "contents": contents });
    if !system_text.is_empty() {
        body["systemInstruction"] = json!({ "parts": [{ "text": system_text }] });
    }
    if !tools.is_empty() {
        let decls: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.function.name,
                    "description": t.function.description,
                    "parameters": t.function.parameters
                })
            })
            .collect();
        body["tools"] = json!([{ "functionDeclarations": decls }]);
    }
    body
}

pub fn parse_gemini_response(text: &str) -> anyhow::Result<AiResponse> {
    let parsed: GeminiResponse = serde_json::from_str(text)
        .map_err(|e| anyhow::anyhow!("Respuesta JSON inválida de Gemini: {e} — {text}"))?;
    let raw_parts = parsed
        .candidates
        .unwrap_or_default()
        .into_iter()
        .next()
        .and_then(|c| c.content)
        .and_then(|c| c.parts)
        .unwrap_or_default();

    let mut text_out = Vec::new();
    let mut tool_calls = Vec::new();
    for (idx, part) in raw_parts.iter().enumerate() {
        if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
            if !t.is_empty()
                && !part
                    .get("thought")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
            {
                text_out.push(t.to_string());
            }
        }
        if let Some(fc) = part.get("functionCall") {
            let name = fc
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let args_str = fc
                .get("args")
                .cloned()
                .unwrap_or_else(|| json!({}))
                .to_string();
            let sig = part
                .get("thoughtSignature")
                .or_else(|| part.get("thought_signature"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let id = fc
                .get("id")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("gemini_{}_{idx}", uuid::Uuid::new_v4()));
            tool_calls.push(ToolCall {
                id,
                kind: "function".to_string(),
                function: ToolFunctionCall {
                    name,
                    arguments: args_str,
                },
                thought_signature: sig,
            });
        }
    }

    let mut response = AiResponse::from_parts(
        if text_out.is_empty() {
            None
        } else {
            Some(text_out.join("\n"))
        },
        tool_calls,
        if raw_parts.is_empty() {
            None
        } else {
            Some(raw_parts)
        },
    );
    response.usage = TokenUsage::from_gemini(&parsed.usage_metadata);
    Ok(response)
}

/// Generic OpenAI-compatible HTTP client (works with OpenAI, Groq, OpenRouter, Ollama, LM Studio, etc.).
pub struct OpenAiCompatibleProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
    stt_model: String,
}

impl OpenAiCompatibleProvider {
    pub fn new(base_url: String, api_key: String, model: String, stt_model: String) -> Self {
        Self::new_with_timeout(base_url, api_key, model, stt_model, 30)
    }

    /// Dreaming can take much longer than a normal interactive chat on local CPUs.
    pub fn new_with_timeout(
        base_url: String,
        api_key: String,
        model: String,
        stt_model: String,
        timeout_secs: u64,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .build()
            .unwrap_or_default();
        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            model,
            stt_model,
        }
    }
}

#[derive(Deserialize)]
struct ApiChatResponse {
    choices: Vec<ApiChoice>,
    #[serde(default)]
    usage: Value,
}

#[derive(Deserialize)]
struct ApiChoice {
    message: ApiMessage,
}

#[derive(Deserialize)]
struct ApiMessage {
    content: Option<String>,
    tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Deserialize)]
struct ApiTranscribeResponse {
    text: String,
}

#[async_trait]
impl AiProvider for OpenAiCompatibleProvider {
    fn native_tool_support(&self) -> NativeToolSupport {
        let url = self.base_url.to_ascii_lowercase();
        if url.contains("api.openai.com")
            || url.contains("openrouter.ai")
            || url.contains("api.groq.com")
        {
            NativeToolSupport::Supported
        } else {
            // Ollama and llama.cpp speak this shape only when the loaded model/template
            // actually supports tools. `tool_choice` is not universal. Probe and say so.
            NativeToolSupport::Probe
        }
    }

    async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDef],
    ) -> anyhow::Result<AiResponse> {
        let url = format!("{}/chat/completions", self.base_url);
        let body = build_openai_chat_request(&self.model, messages, tools);

        let mut req = self.client.post(&url).json(&body);
        if !self.api_key.trim().is_empty() {
            req = req.bearer_auth(self.api_key.trim());
        }

        let resp = req.send().await?;
        let status = resp.status();
        let text = resp.text().await?;

        // Fallback if the configured model/endpoint does not support Tool Calling.
        if !status.is_success()
            && !tools.is_empty()
            && (status.as_u16() == 400 || status.as_u16() == 404 || status.as_u16() == 422)
        {
            let lower = text.to_lowercase();
            if lower.contains("tool") || lower.contains("function") {
                let fallback_body = json!({
                    "model": self.model,
                    "messages": ordered_tool_history(messages),
                });
                let mut retry_req = self.client.post(&url).json(&fallback_body);
                if !self.api_key.trim().is_empty() {
                    retry_req = retry_req.bearer_auth(self.api_key.trim());
                }
                let retry_resp = retry_req.send().await?;
                let retry_status = retry_resp.status();
                let retry_text = retry_resp.text().await?;
                if !retry_status.is_success() {
                    anyhow::bail!("Error de API ({retry_status}): {retry_text}");
                }
                let mut parsed = parse_openai_chat_response(&retry_text)?;
                // The endpoint refused native tools. Keep the plain text and do not
                // scrape it for fake function calls.
                parsed.tools_rejected = true;
                parsed.tool_calls.clear();
                return Ok(parsed);
            }
        }

        if !status.is_success() {
            anyhow::bail!("Error de API ({status}): {text}");
        }

        parse_openai_chat_response(&text)
    }

    async fn transcribe(&self, wav_bytes: Vec<u8>) -> anyhow::Result<String> {
        let url = format!("{}/audio/transcriptions", self.base_url);
        let part = multipart::Part::bytes(wav_bytes)
            .file_name("audio.wav")
            .mime_str("audio/wav")?;

        let form = multipart::Form::new()
            .text("model", self.stt_model.clone())
            .part("file", part);

        let mut req = self.client.post(&url).multipart(form);
        if !self.api_key.trim().is_empty() {
            req = req.bearer_auth(self.api_key.trim());
        }

        let resp = req.send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("Error STT ({status}): {text}");
        }

        let parsed: ApiTranscribeResponse = serde_json::from_str(&text)?;
        Ok(parsed.text.trim().to_string())
    }
}

/// Built-in local mock provider so the assistant works immediately without an API key.
pub struct MockProvider;

#[async_trait]
impl AiProvider for MockProvider {
    fn native_tool_support(&self) -> NativeToolSupport {
        NativeToolSupport::Supported
    }

    async fn chat(
        &self,
        messages: &[ChatMessage],
        _tools: &[ToolDef],
    ) -> anyhow::Result<AiResponse> {
        // If the last message is a tool result, summarize all tool results from the latest turn.
        if let Some(last) = messages.last() {
            if last.role == "tool" {
                let results: Vec<&str> = messages
                    .iter()
                    .rev()
                    .take_while(|m| m.role == "tool")
                    .filter_map(|m| m.content.as_deref())
                    .collect();
                let summary = results.into_iter().rev().collect::<Vec<_>>().join(" | ");
                return Ok(AiResponse::from_parts(
                    Some(format!("Listo (Modo Local): {summary}")),
                    vec![],
                    None,
                ));
            }
        }

        let user_text = messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .and_then(|m| m.content.as_deref())
            .unwrap_or("")
            .trim();
        let lower = user_text.to_lowercase();

        if lower.contains("simular error") {
            anyhow::bail!("Error simulado por MockProvider");
        }

        let mut calls = Vec::new();
        if lower.contains("tarea") || lower.contains("anota") || lower.contains("pendiente") {
            let title = user_text
                .split(':')
                .nth(1)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(user_text);
            calls.push(make_mock_call(
                "add_task",
                json!({ "title": title }),
                calls.len(),
            ));
        }
        if lower.contains("agua")
            || lower.contains("vaso")
            || lower.contains("hábito")
            || lower.contains("habito")
        {
            calls.push(make_mock_call(
                "log_habit",
                json!({ "name": "Vasos de agua", "delta": 1 }),
                calls.len(),
            ));
        }
        if lower.contains("pomodoro") || lower.contains("enfoque") || lower.contains("concentr") {
            if lower.contains("pausa") || lower.contains("deten") || lower.contains("para") {
                calls.push(make_mock_call("pause_pomodoro", json!({}), calls.len()));
            } else {
                calls.push(make_mock_call(
                    "start_pomodoro",
                    json!({ "minutes": 25 }),
                    calls.len(),
                ));
            }
        }
        if lower.contains("deshacer") || lower.contains("undo") {
            calls.push(make_mock_call(
                "undo_organize_desktop",
                json!({}),
                calls.len(),
            ));
        } else if lower.contains("ordena")
            || lower.contains("organiza")
            || lower.contains("escritorio")
        {
            if lower.contains("aplica") || lower.contains("confirma") || lower.contains("ejecuta") {
                calls.push(make_mock_call(
                    "apply_organize_desktop",
                    json!({}),
                    calls.len(),
                ));
            } else {
                calls.push(make_mock_call(
                    "preview_organize_desktop",
                    json!({}),
                    calls.len(),
                ));
            }
        }

        if lower.contains("volumen") || lower.contains("volume") {
            let percent = lower
                .split(|c: char| !c.is_ascii_digit())
                .find_map(|s| s.parse::<u64>().ok())
                .filter(|n| *n <= 100)
                .unwrap_or(20);
            calls.push(make_mock_call(
                "music_set_volume",
                json!({ "percent": percent }),
                calls.len(),
            ));
        }
        if lower.contains("música")
            || lower.contains("musica")
            || lower.contains("estación")
            || lower.contains("estacion")
            || lower.contains("playlist")
        {
            let query = user_text
                .split(':')
                .nth(1)
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let args = match query {
                Some(q) => json!({ "query": q }),
                None => json!({}),
            };
            calls.push(make_mock_call("music_play", args, calls.len()));
        }

        if !calls.is_empty() {
            return Ok(AiResponse::from_parts(None, calls, None));
        }

        Ok(AiResponse::from_parts(
            Some(
                "Estoy en **Modo Local** (sin API key configurada).\n\n\
                Podés probar comandos como:\n\
                - *\"Anota una tarea: comprar café\"*\n\
                - *\"Tomé un vaso de agua\"*\n\
                - *\"Inicia un pomodoro\"*\n\
                - *\"Poné música y bajá el volumen al 20%\"*\n\
                - *\"Ordena mi escritorio\"*\n\n\
                Cuando elijas tu API (Gemini, OpenAI, Ollama, etc.), configurala en la pestaña **⚙ Config API**."
                    .to_string(),
            ),
            vec![],
            None,
        ))
    }

    async fn transcribe(&self, _wav_bytes: Vec<u8>) -> anyhow::Result<String> {
        anyhow::bail!(
            "La transcripción de voz requiere configurar una API compatible con /audio/transcriptions (ej. OpenAI o Groq) en Config API."
        )
    }
}

/// Native Google Gemini API provider (`:generateContent` with `X-goog-api-key`).
pub struct GoogleGeminiProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl GoogleGeminiProvider {
    pub fn new(base_url: String, api_key: String, model: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .unwrap_or_default();
        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            model,
        }
    }
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Option<Vec<GeminiCandidate>>,
    #[serde(default, rename = "usageMetadata")]
    usage_metadata: Value,
}

#[derive(Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiContent>,
}

#[derive(Deserialize)]
struct GeminiContent {
    parts: Option<Vec<Value>>,
}

#[async_trait]
impl AiProvider for GoogleGeminiProvider {
    fn native_tool_support(&self) -> NativeToolSupport {
        NativeToolSupport::Supported
    }

    async fn chat(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDef],
    ) -> anyhow::Result<AiResponse> {
        let body = build_gemini_request(messages, tools);

        let mut last_status = reqwest::StatusCode::INTERNAL_SERVER_ERROR;
        let mut text = String::new();
        // Only the moving "latest" alias gets an automatic alternative.
        // Explicitly selected models are never silently replaced. 429 is a
        // quota/rate limit, not evidence that the model is overloaded.
        let overload_fallback = self.model == "gemini-flash-latest";
        let models: Vec<&str> = if overload_fallback {
            vec![self.model.as_str(), "gemini-3.5-flash-lite"]
        } else {
            vec![self.model.as_str()]
        };
        'models: for (model_index, model) in models.iter().enumerate() {
            let url = format!("{}/models/{}:generateContent", self.base_url, model);
            let attempts = if model_index == 0 { 3 } else { 2 };
            for attempt in 0..attempts {
                let resp = match self
                    .client
                    .post(&url)
                    .header("X-goog-api-key", self.api_key.trim())
                    .json(&body)
                    .send()
                    .await
                {
                    Ok(r) => r,
                    Err(_e) if attempt + 1 < attempts => {
                        tokio::time::sleep(std::time::Duration::from_secs(1 << attempt)).await;
                        continue;
                    }
                    Err(e) => {
                        return Err(anyhow::anyhow!("Error de red con Gemini: {e:#}"));
                    }
                };

                last_status = resp.status();
                text = resp
                    .text()
                    .await
                    .map_err(|e| anyhow::anyhow!("Error leyendo respuesta de Gemini: {e:#}"))?;
                if last_status.is_success() {
                    break 'models;
                }
                if (last_status.as_u16() == 503 || last_status.as_u16() == 429)
                    && attempt + 1 < attempts
                {
                    tokio::time::sleep(std::time::Duration::from_secs(1 << attempt)).await;
                    continue;
                }
                if last_status.as_u16() == 503 && model_index == 0 && overload_fallback {
                    eprintln!("Gemini Flash latest saturado (503); probando gemini-3.5-flash-lite");
                    break; // try the current Flash-Lite alternative with the same API key
                }
                anyhow::bail!("Error de Gemini ({last_status}) en {model}: {text}");
            }
        }
        if !last_status.is_success() {
            anyhow::bail!("Error de Gemini ({last_status}): {text}");
        }

        parse_gemini_response(&text)
    }

    async fn transcribe(&self, _wav_bytes: Vec<u8>) -> anyhow::Result<String> {
        anyhow::bail!(
            "El endpoint nativo de Gemini no usa /audio/transcriptions. Usá texto o configurá Groq/OpenAI para STT."
        )
    }
}

fn make_mock_call(name: &str, args: Value, idx: usize) -> ToolCall {
    ToolCall {
        id: format!("mock_{name}_{idx}"),
        kind: "function".to_string(),
        function: ToolFunctionCall {
            name: name.to_string(),
            arguments: args.to_string(),
        },
        thought_signature: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn spawn_mock_http_server(responses: Vec<(u16, String)>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for (status, body) in responses {
                if let Ok((mut stream, _)) = listener.accept() {
                    let mut buf = [0u8; 4096];
                    let _ = stream.read(&mut buf);
                    let status_text = if status == 200 { "OK" } else { "Bad Request" };
                    let resp = format!(
                        "HTTP/1.1 {status} {status_text}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(resp.as_bytes());
                }
            }
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn mock_provider_supports_multi_tool_errors_and_resumed_history() {
        let mock = MockProvider;
        let resp = mock
            .chat(
                &[ChatMessage::user(
                    "Anota una tarea: pan y tomé un vaso de agua",
                )],
                &[],
            )
            .await
            .unwrap();
        assert_eq!(resp.tool_calls.len(), 2);
        assert_eq!(resp.tool_calls[0].function.name, "add_task");
        assert_eq!(resp.tool_calls[1].function.name, "log_habit");

        let resumed = mock
            .chat(
                &[
                    ChatMessage::user("Anota una tarea: pan y tomé un vaso de agua"),
                    ChatMessage {
                        role: "assistant".to_string(),
                        content: None,
                        tool_calls: Some(resp.tool_calls.clone()),
                        tool_call_id: None,
                        raw_gemini_parts: None,
                    },
                    ChatMessage::tool_result(&resp.tool_calls[0].id, "Tarea agregada"),
                    ChatMessage::tool_result(&resp.tool_calls[1].id, "Agua 1/8"),
                ],
                &[],
            )
            .await
            .unwrap();
        assert!(resumed.tool_calls.is_empty());
        assert!(resumed
            .content
            .unwrap()
            .contains("Tarea agregada | Agua 1/8"));

        assert!(mock
            .chat(&[ChatMessage::user("por favor simular error")], &[])
            .await
            .is_err());
    }

    #[tokio::test]
    async fn openai_provider_tool_calls_and_unsupported_tools_fallback() {
        let base_url = spawn_mock_http_server(vec![
            (
                400,
                r#"{"error":{"message":"This model does not support tools"}}"#.to_string(),
            ),
            (
                200,
                r#"{"choices":[{"message":{"content":"Respuesta sin tools"}}]}"#.to_string(),
            ),
        ]);
        let provider = OpenAiCompatibleProvider::new(
            base_url,
            "test-key".to_string(),
            "test-model".to_string(),
            "whisper-1".to_string(),
        );
        let tools = vec![ToolDef {
            kind: "function",
            function: ToolFunctionDef {
                name: "list_tasks",
                description: "Lista tareas",
                parameters: json!({"type":"object","properties":{}}),
            },
        }];
        let resp = provider
            .chat(&[ChatMessage::user("hola")], &tools)
            .await
            .unwrap();
        assert_eq!(resp.content.as_deref(), Some("Respuesta sin tools"));
        assert!(resp.tools_rejected);
        assert!(resp.tool_calls.is_empty());
    }

    #[test]
    fn chat_parsers_read_optional_token_usage() {
        let openai = parse_openai_chat_response(
            r#"{"choices":[{"message":{"content":"ok"}}],"usage":{"prompt_tokens":10,"completion_tokens":20,"total_tokens":30}}"#,
        ).unwrap();
        let gemini = parse_gemini_response(
            r#"{"usageMetadata":{"promptTokenCount":10,"cachedContentTokenCount":3,"toolUsePromptTokenCount":5,"candidatesTokenCount":7,"thoughtsTokenCount":8,"totalTokenCount":30}}"#,
        ).unwrap();
        let unreported = parse_gemini_response("{}").unwrap();
        assert_eq!(
            openai.usage,
            Some(TokenUsage {
                input: 10,
                output: 20,
                total: 30
            })
        );
        assert_eq!(
            gemini.usage,
            Some(TokenUsage {
                input: 15,
                output: 15,
                total: 30
            })
        );
        assert_eq!(unreported.usage, None);
    }

    #[test]
    fn gemini_calls_without_provider_ids_have_distinct_local_ids() {
        let body = r#"{"candidates":[{"content":{"parts":[{"functionCall":{"name":"list_tasks","args":{}}}]}}]}"#;
        let first = parse_gemini_response(body).unwrap();
        let second = parse_gemini_response(body).unwrap();
        assert_ne!(first.tool_calls[0].id, second.tool_calls[0].id);
    }

    #[test]
    fn openai_and_gemini_fixtures_round_trip_parallel_tool_calls() {
        let tools = vec![ToolDef {
            kind: "function",
            function: ToolFunctionDef {
                name: "start_pomodoro",
                description: "Inicia un Pomodoro",
                parameters: json!({
                    "type": "object",
                    "properties": { "minutes": { "type": "integer" } },
                    "required": ["minutes"]
                }),
            },
        }];
        let call = ToolCall {
            id: "call_abc".to_string(),
            kind: "function".to_string(),
            function: ToolFunctionCall {
                name: "start_pomodoro".to_string(),
                arguments: r#"{"minutes":25}"#.to_string(),
            },
            thought_signature: None,
        };
        let history = vec![
            ChatMessage::user("arrancá un pomodoro"),
            ChatMessage {
                role: "assistant".to_string(),
                content: None,
                tool_calls: Some(vec![call.clone()]),
                tool_call_id: None,
                raw_gemini_parts: None,
            },
            ChatMessage::tool_result("call_abc", "Modo Enfoque iniciado: 25 minutos."),
        ];

        let openai = build_openai_chat_request("gpt-4o-mini", &history, &tools);
        assert_eq!(openai["tool_choice"], "auto");
        assert_eq!(openai["tools"][0]["type"], "function");
        assert_eq!(openai["tools"][0]["function"]["name"], "start_pomodoro");
        assert_eq!(openai["messages"][1]["tool_calls"][0]["id"], "call_abc");
        assert_eq!(openai["messages"][2]["role"], "tool");
        assert_eq!(openai["messages"][2]["tool_call_id"], "call_abc");

        let parsed = parse_openai_chat_response(
            r#"{"id":"chatcmpl_1","choices":[{"index":0,"finish_reason":"tool_calls","message":{"role":"assistant","content":null,"tool_calls":[{"id":"call_1","type":"function","function":{"name":"music_set_volume","arguments":"{\"percent\":20}"}},{"id":"call_2","type":"function","function":{"name":"add_task","arguments":"{\"title\":\"comprar café\"}"}}]}}]}"#,
        )
        .unwrap();
        assert!(!parsed.tools_rejected);
        assert_eq!(parsed.tool_calls.len(), 2);
        assert_eq!(parsed.tool_calls[0].function.name, "music_set_volume");
        assert_eq!(parsed.tool_calls[1].id, "call_2");
        assert!(parsed.tool_calls[1]
            .function
            .arguments
            .contains("comprar café"));

        let gemini_call = ToolCall {
            id: "fc_official".to_string(),
            kind: "function".to_string(),
            function: ToolFunctionCall {
                name: "start_pomodoro".to_string(),
                arguments: r#"{"minutes":25}"#.to_string(),
            },
            thought_signature: Some("sig-abc".to_string()),
        };
        let gemini_history = vec![
            ChatMessage::system("Sos Rusty"),
            ChatMessage::user("pomodoro"),
            ChatMessage {
                role: "assistant".to_string(),
                content: None,
                tool_calls: Some(vec![gemini_call]),
                tool_call_id: None,
                raw_gemini_parts: None,
            },
            ChatMessage::tool_result("fc_official", "iniciado"),
        ];
        let gemini = build_gemini_request(&gemini_history, &tools);
        assert_eq!(
            gemini["tools"][0]["functionDeclarations"][0]["name"],
            "start_pomodoro"
        );
        assert_eq!(gemini["systemInstruction"]["parts"][0]["text"], "Sos Rusty");
        assert_eq!(
            gemini["contents"][1]["parts"][0]["functionCall"]["name"],
            "start_pomodoro"
        );
        assert_eq!(
            gemini["contents"][1]["parts"][0]["thoughtSignature"],
            "sig-abc"
        );
        assert_eq!(
            gemini["contents"][2]["parts"][0]["functionResponse"]["name"],
            "start_pomodoro"
        );
        assert_eq!(
            gemini["contents"][2]["parts"][0]["functionResponse"]["id"],
            "fc_official"
        );
        assert_eq!(
            gemini["contents"][2]["parts"][0]["functionResponse"]["response"]["result"],
            "iniciado"
        );

        let gemini_parsed = parse_gemini_response(
            r#"{"candidates":[{"content":{"role":"model","parts":[{"functionCall":{"name":"music_play","args":{},"id":"fc_1"}},{"functionCall":{"name":"start_pomodoro","args":{"minutes":25},"id":"fc_2"},"thoughtSignature":"sig-2"}]}}]}"#,
        )
        .unwrap();
        assert_eq!(gemini_parsed.tool_calls.len(), 2);
        assert_eq!(gemini_parsed.tool_calls[0].id, "fc_1");
        assert_eq!(gemini_parsed.tool_calls[1].function.name, "start_pomodoro");
        assert_eq!(
            gemini_parsed.tool_calls[1].thought_signature.as_deref(),
            Some("sig-2")
        );
        assert!(gemini_parsed.raw_gemini_parts.is_some());

        assert_eq!(
            resolve_tool_dispatch(true, NativeToolSupport::Supported),
            ToolDispatch::Unavailable
        );
        assert_eq!(
            resolve_tool_dispatch(false, NativeToolSupport::Unsupported),
            ToolDispatch::Unavailable
        );
        assert_eq!(
            resolve_tool_dispatch(false, NativeToolSupport::Probe),
            ToolDispatch::Native
        );
        assert_eq!(
            OpenAiCompatibleProvider::new(
                "http://127.0.0.1:11434/v1".into(),
                String::new(),
                "llama3.2".into(),
                String::new(),
            )
            .native_tool_support(),
            NativeToolSupport::Probe
        );
        assert_eq!(
            GoogleGeminiProvider::new(
                "https://generativelanguage.googleapis.com/v1beta".into(),
                "k".into(),
                "gemini-flash-latest".into(),
            )
            .native_tool_support(),
            NativeToolSupport::Supported
        );
    }

    #[tokio::test]
    async fn gemini_provider_parses_function_call_and_preserves_parts() {
        let base_url = spawn_mock_http_server(vec![(
            200,
            r#"{"candidates":[{"content":{"parts":[{"functionCall":{"name":"list_tasks","args":{}},"thoughtSignature":"sig-123"}]}}]}"#
                .to_string(),
        )]);
        let provider = GoogleGeminiProvider::new(
            base_url,
            "test-key".to_string(),
            "gemini-flash-latest".to_string(),
        );
        let tools = vec![ToolDef {
            kind: "function",
            function: ToolFunctionDef {
                name: "list_tasks",
                description: "Lista tareas",
                parameters: json!({"type":"object","properties":{}}),
            },
        }];
        let resp = provider
            .chat(&[ChatMessage::user("listame las tareas")], &tools)
            .await
            .unwrap();
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].function.name, "list_tasks");
        assert_eq!(
            resp.tool_calls[0].thought_signature.as_deref(),
            Some("sig-123")
        );
        assert!(resp.raw_gemini_parts.is_some());
    }

    #[tokio::test]
    async fn gemini_overload_falls_back_to_flash_lite() {
        let overloaded = r#"{"error":{"status":"UNAVAILABLE"}}"#.to_string();
        let base_url = spawn_mock_http_server(vec![
            (503, overloaded.clone()),
            (503, overloaded.clone()),
            (503, overloaded),
            (
                200,
                r#"{"candidates":[{"content":{"parts":[{"text":"Volví"}]}}]}"#.to_string(),
            ),
        ]);
        let provider =
            GoogleGeminiProvider::new(base_url, "test-key".into(), "gemini-flash-latest".into());
        let response = provider
            .chat(&[ChatMessage::user("Hola")], &[])
            .await
            .unwrap();
        assert_eq!(response.content.as_deref(), Some("Volví"));
    }

    #[tokio::test]
    async fn gemini_other_client_errors_never_trigger_fallback() {
        let base_url = spawn_mock_http_server(vec![(400, "invalid model".into())]);
        let provider =
            GoogleGeminiProvider::new(base_url, "test-key".into(), "gemini-flash-latest".into());
        let err = provider
            .chat(&[ChatMessage::user("Hola")], &[])
            .await
            .unwrap_err();
        assert!(err.to_string().contains("400"));
    }
}
