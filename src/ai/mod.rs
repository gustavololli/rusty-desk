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

#[derive(Debug, Clone)]
pub struct AiResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub raw_gemini_parts: Option<Vec<Value>>,
}

#[async_trait]
pub trait AiProvider: Send + Sync {
    async fn chat(&self, messages: &[ChatMessage], tools: &[ToolDef]) -> anyhow::Result<AiResponse>;
    async fn transcribe(&self, wav_bytes: Vec<u8>) -> anyhow::Result<String>;
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
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
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
    async fn chat(&self, messages: &[ChatMessage], tools: &[ToolDef]) -> anyhow::Result<AiResponse> {
        let url = format!("{}/chat/completions", self.base_url);
        let mut body = json!({
            "model": self.model,
            "messages": messages,
        });

        if !tools.is_empty() {
            body["tools"] = serde_json::to_value(tools)?;
            body["tool_choice"] = json!("auto");
        }

        let mut req = self.client.post(&url).json(&body);
        if !self.api_key.trim().is_empty() {
            req = req.bearer_auth(self.api_key.trim());
        }

        let resp = req.send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("Error de API ({status}): {text}");
        }

        let parsed: ApiChatResponse = serde_json::from_str(&text)
            .map_err(|e| anyhow::anyhow!("Respuesta JSON inválida: {e} — {text}"))?;

        let first = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("La API no devolvió choices"))?;

        Ok(AiResponse {
            content: first.message.content,
            tool_calls: first.message.tool_calls.unwrap_or_default(),
            raw_gemini_parts: None,
        })
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
    async fn chat(&self, messages: &[ChatMessage], _tools: &[ToolDef]) -> anyhow::Result<AiResponse> {
        // If the last message is a tool result, summarize it nicely.
        if let Some(last) = messages.last() {
            if last.role == "tool" {
                let result_text = last.content.as_deref().unwrap_or("Hecho.");
                return Ok(AiResponse {
                    content: Some(format!("Listo (Modo Local): {result_text}")),
                    tool_calls: vec![],
                    raw_gemini_parts: None,
                });
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

        // Simple Spanish/English intent matching -> emits real tool calls
        if lower.contains("tarea") || lower.contains("anota") || lower.contains("pendiente") {
            let title = user_text
                .split(':')
                .nth(1)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(user_text);
            return Ok(mock_tool("add_task", json!({ "title": title })));
        }

        if lower.contains("agua") || lower.contains("vaso") || lower.contains("hábito") || lower.contains("habito") {
            return Ok(mock_tool(
                "log_habit",
                json!({ "name": "Vasos de agua", "delta": 1 }),
            ));
        }

        if lower.contains("pomodoro") || lower.contains("enfoque") || lower.contains("concentr") {
            if lower.contains("pausa") || lower.contains("deten") || lower.contains("para") {
                return Ok(mock_tool("pause_pomodoro", json!({})));
            }
            return Ok(mock_tool("start_pomodoro", json!({ "minutes": 25 })));
        }

        if lower.contains("deshacer") || lower.contains("undo") {
            return Ok(mock_tool("undo_organize_desktop", json!({})));
        }

        if lower.contains("ordena") || lower.contains("organiza") || lower.contains("escritorio") {
            if lower.contains("aplica") || lower.contains("confirma") || lower.contains("ejecuta") {
                return Ok(mock_tool("apply_organize_desktop", json!({})));
            }
            return Ok(mock_tool("preview_organize_desktop", json!({})));
        }

        Ok(AiResponse {
            content: Some(
                "Estoy en **Modo Local** (sin API key configurada).\n\n\
                Podés probar comandos como:\n\
                - *\"Anota una tarea: comprar café\"*\n\
                - *\"Tomé un vaso de agua\"*\n\
                - *\"Inicia un pomodoro\"*\n\
                - *\"Ordena mi escritorio\"*\n\n\
                Cuando elijas tu API (OpenAI, Groq, Ollama, etc.), configurala en la pestaña **⚙ Config API**."
                    .to_string(),
            ),
            tool_calls: vec![],
            raw_gemini_parts: None,
        })
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
    async fn chat(&self, messages: &[ChatMessage], tools: &[ToolDef]) -> anyhow::Result<AiResponse> {
        let url = format!("{}/models/{}:generateContent", self.base_url, self.model);

        let system_text: String = messages
            .iter()
            .filter(|m| m.role == "system")
            .filter_map(|m| m.content.as_deref())
            .collect::<Vec<_>>()
            .join("\n");

        let mut contents = Vec::new();
        for msg in messages.iter().filter(|m| m.role != "system") {
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
                    let fn_name = messages
                        .iter()
                        .filter_map(|m| m.tool_calls.as_ref())
                        .flatten()
                        .find(|tc| tc.id == call_id)
                        .map(|tc| tc.function.name.as_str())
                        .unwrap_or("tool");
                    let result_str = msg.content.as_deref().unwrap_or("");
                    contents.push(json!({
                        "role": "user",
                        "parts": [{
                            "functionResponse": {
                                "name": fn_name,
                                "response": { "result": result_str }
                            }
                        }]
                    }));
                }
                _ => {}
            }
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

        let mut last_status = reqwest::StatusCode::INTERNAL_SERVER_ERROR;
        let mut text = String::new();
        for attempt in 0..3 {
            let resp = match self
                .client
                .post(&url)
                .header("X-goog-api-key", self.api_key.trim())
                .json(&body)
                .send()
                .await
            {
                Ok(r) => r,
                Err(_e) if attempt < 2 => {
                    tokio::time::sleep(std::time::Duration::from_secs(attempt + 1)).await;
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
                break;
            }
            if (last_status.as_u16() == 503 || last_status.as_u16() == 429) && attempt < 2 {
                tokio::time::sleep(std::time::Duration::from_secs(attempt + 1)).await;
                continue;
            }
            anyhow::bail!("Error de Gemini ({last_status}): {text}");
        }
        if !last_status.is_success() {
            anyhow::bail!("Error de Gemini ({last_status}): {text}");
        }

        let parsed: GeminiResponse = serde_json::from_str(&text)
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
                if !t.is_empty() && !part.get("thought").and_then(|v| v.as_bool()).unwrap_or(false) {
                    text_out.push(t.to_string());
                }
            }
            if let Some(fc) = part.get("functionCall") {
                let name = fc.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let args_str = fc.get("args").cloned().unwrap_or_else(|| json!({})).to_string();
                let sig = part
                    .get("thoughtSignature")
                    .or_else(|| part.get("thought_signature"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                tool_calls.push(ToolCall {
                    id: format!("gemini_{name}_{idx}"),
                    kind: "function".to_string(),
                    function: ToolFunctionCall {
                        name,
                        arguments: args_str,
                    },
                    thought_signature: sig,
                });
            }
        }

        Ok(AiResponse {
            content: if text_out.is_empty() {
                None
            } else {
                Some(text_out.join("\n"))
            },
            tool_calls,
            raw_gemini_parts: if raw_parts.is_empty() {
                None
            } else {
                Some(raw_parts)
            },
        })
    }

    async fn transcribe(&self, _wav_bytes: Vec<u8>) -> anyhow::Result<String> {
        anyhow::bail!(
            "El endpoint nativo de Gemini no usa /audio/transcriptions. Usá texto o configurá Groq/OpenAI para STT."
        )
    }
}

fn mock_tool(name: &str, args: Value) -> AiResponse {
    AiResponse {
        content: None,
        tool_calls: vec![ToolCall {
            id: format!("mock_{name}"),
            kind: "function".to_string(),
            function: ToolFunctionCall {
                name: name.to_string(),
                arguments: args.to_string(),
            },
            thought_signature: None,
        }],
        raw_gemini_parts: None,
    }
}
