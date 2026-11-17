use crate::ai::{ToolDef, ToolFunctionDef};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskLevel {
    Read,
    LowRiskWrite,
    FileMutation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationKind {
    Read,
    Write,
    FileSystem,
    Conversion,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ToolSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Value,
    pub risk: RiskLevel,
    pub operation: OperationKind,
    pub category: &'static str,
}

pub fn all_tool_specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            id: "add_task",
            name: "add_task",
            description: "Agrega una nueva tarea pendiente a la lista del usuario.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Descripción breve de la tarea" }
                },
                "required": ["title"]
            }),
            risk: RiskLevel::LowRiskWrite,
            operation: OperationKind::Write,
            category: "tasks",
        },
        ToolSpec {
            id: "complete_task",
            name: "complete_task",
            description: "Marca una tarea como completada buscando por texto.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Texto parcial de la tarea a completar" }
                },
                "required": ["query"]
            }),
            risk: RiskLevel::LowRiskWrite,
            operation: OperationKind::Write,
            category: "tasks",
        },
        ToolSpec {
            id: "list_tasks",
            name: "list_tasks",
            description: "Lista las tareas actuales del usuario.",
            parameters: json!({ "type": "object", "properties": {} }),
            risk: RiskLevel::Read,
            operation: OperationKind::Read,
            category: "tasks",
        },
        ToolSpec {
            id: "log_habit",
            name: "log_habit",
            description: "Incrementa o registra progreso en un hábito diario (ej. 'Vasos de agua').",
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Nombre del hábito" },
                    "delta": { "type": "integer", "description": "Cantidad a sumar (por defecto 1)" }
                },
                "required": ["name"]
            }),
            risk: RiskLevel::LowRiskWrite,
            operation: OperationKind::Write,
            category: "habits",
        },
        ToolSpec {
            id: "start_pomodoro",
            name: "start_pomodoro",
            description: "Inicia el temporizador de enfoque Pomodoro.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "minutes": { "type": "integer", "description": "Duración en minutos (opcional, por defecto 25)" }
                }
            }),
            risk: RiskLevel::LowRiskWrite,
            operation: OperationKind::Write,
            category: "pomodoro",
        },
        ToolSpec {
            id: "pause_pomodoro",
            name: "pause_pomodoro",
            description: "Pausa o reanuda el temporizador Pomodoro.",
            parameters: json!({ "type": "object", "properties": {} }),
            risk: RiskLevel::LowRiskWrite,
            operation: OperationKind::Write,
            category: "pomodoro",
        },
        ToolSpec {
            id: "preview_organize_desktop",
            name: "preview_organize_desktop",
            description: "Previsualiza cómo se organizarían los archivos sueltos del escritorio sin mover nada todavía.",
            parameters: json!({ "type": "object", "properties": {} }),
            risk: RiskLevel::Read,
            operation: OperationKind::Read,
            category: "organizer",
        },
        ToolSpec {
            id: "apply_organize_desktop",
            name: "apply_organize_desktop",
            description: "Aplica la organización de archivos del escritorio moviéndolos a carpetas por tipo (requiere confirmación).",
            parameters: json!({ "type": "object", "properties": {} }),
            risk: RiskLevel::FileMutation,
            operation: OperationKind::FileSystem,
            category: "organizer",
        },
        ToolSpec {
            id: "undo_organize_desktop",
            name: "undo_organize_desktop",
            description: "Deshace la última organización de archivos del escritorio (requiere confirmación).",
            parameters: json!({ "type": "object", "properties": {} }),
            risk: RiskLevel::FileMutation,
            operation: OperationKind::FileSystem,
            category: "organizer",
        },
        ToolSpec {
            id: "convert_to_markdown",
            name: "convert_to_markdown",
            description: "Convierte un archivo local (PDF, DOCX, XLSX, PPTX, HTML, CSV, XML, RSS, etc.) a Markdown usando MarkItDown local sin inferencia extra, con opción de compactar la salida y guardar el .md.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Ruta absoluta del archivo a convertir a Markdown" },
                    "save_file": { "type": "boolean", "description": "Si es true, guarda el .md resultante junto al archivo original (por defecto true)" },
                    "compact": {
                        "type": "string",
                        "enum": ["none", "moderate", "line", "extreme"],
                        "description": "Nivel de compactación: 'none', 'moderate' (recomendado), 'line' o 'extreme'."
                    }
                },
                "required": ["path"]
            }),
            risk: RiskLevel::LowRiskWrite,
            operation: OperationKind::Conversion,
            category: "markdown",
        },
        ToolSpec {
            id: "add_music_url",
            name: "add_music_url",
            description: "Guarda una URL de música, radio o stream en la biblioteca de música (SQLite).",
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL del stream o audio (http://, https://, rtsp://, file://)" },
                    "title": { "type": "string", "description": "Nombre opcional para la pista o radio" }
                },
                "required": ["url"]
            }),
            risk: RiskLevel::LowRiskWrite,
            operation: OperationKind::Write,
            category: "music",
        },
        ToolSpec {
            id: "get_weather",
            name: "get_weather",
            description: "Consulta el clima actual y temperatura de una ciudad usando Open-Meteo (gratuito).",
            parameters: json!({
                "type": "object",
                "properties": {
                    "city": { "type": "string", "description": "Nombre de la ciudad (ej: Buenos Aires, Córdoba, Madrid)" }
                },
                "required": ["city"]
            }),
            risk: RiskLevel::Read,
            operation: OperationKind::Read,
            category: "weather",
        },
        ToolSpec {
            id: "web_search",
            name: "web_search",
            description: "Buscar información actualizada en Internet y devolver resultados con título, URL, resumen y fuente.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Consulta de búsqueda" },
                    "max_results": { "type": "integer", "description": "Cantidad máxima de resultados (1..=10, por defecto 5)" }
                },
                "required": ["query"]
            }),
            risk: RiskLevel::Read,
            operation: OperationKind::Read,
            category: "web_search",
        },
        ToolSpec {
            id: "web_fetch",
            name: "web_fetch",
            description: "Lee y extrae el texto principal de una URL pública HTTP/HTTPS para resumirla o consultarla.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "URL HTTP o HTTPS pública a leer" }
                },
                "required": ["url"]
            }),
            risk: RiskLevel::Read,
            operation: OperationKind::Read,
            category: "web_fetch",
        },
        ToolSpec {
            id: "get_exchange_rate",
            name: "get_exchange_rate",
            description: "Consulta tipos de cambio oficiales de referencia (Frankfurter/BCE, no dólar blue/MEP).",
            parameters: json!({
                "type": "object",
                "properties": {
                    "base": { "type": "string", "description": "Código ISO de moneda origen (ej: USD)" },
                    "target": { "type": "string", "description": "Código ISO de moneda destino (ej: EUR)" },
                    "amount": { "type": "number", "description": "Importe a convertir (por defecto 1)" }
                },
                "required": ["base", "target"]
            }),
            risk: RiskLevel::Read,
            operation: OperationKind::Read,
            category: "exchange",
        },
    ]
}

pub fn find_spec(name: &str) -> Option<ToolSpec> {
    all_tool_specs().into_iter().find(|s| s.name == name)
}

pub fn specs_to_tool_defs(specs: &[ToolSpec]) -> Vec<ToolDef> {
    specs
        .iter()
        .map(|s| ToolDef {
            kind: "function",
            function: ToolFunctionDef {
                name: s.name,
                description: s.description,
                parameters: s.parameters.clone(),
            },
        })
        .collect()
}

/// Selects relevant tool definitions locally (zero LLM calls) to save prompt tokens.
/// Falls back to all tools when the user prompt is ambiguous or multi-domain.
pub fn select_tools_for_message(user_text: &str) -> Vec<ToolDef> {
    let lower = user_text.to_lowercase();
    let mut categories = Vec::new();

    if [
        "tarea",
        "pendiente",
        "anota",
        "recorda",
        "comple",
        "task",
        "lista",
    ]
    .iter()
    .any(|k| lower.contains(k))
    {
        categories.push("tasks");
    }
    if ["agua", "vaso", "habito", "hábito", "meta", "habit"]
        .iter()
        .any(|k| lower.contains(k))
    {
        categories.push("habits");
    }
    if [
        "pomodoro", "enfoque", "pausa", "concentr", "descanso", "timer",
    ]
    .iter()
    .any(|k| lower.contains(k))
    {
        categories.push("pomodoro");
    }
    if [
        "ordena",
        "organiza",
        "escritorio",
        "deshacer",
        "undo",
        "archivo",
    ]
    .iter()
    .any(|k| lower.contains(k))
    {
        categories.push("organizer");
    }
    if [
        "markdown",
        "markitdown",
        "pdf",
        "docx",
        "xlsx",
        "pptx",
        "convert",
        "compact",
        ".md",
        "/",
    ]
    .iter()
    .any(|k| lower.contains(k))
    {
        categories.push("markdown");
    }
    if [
        "musica", "música", "radio", "stream", "url", "cancion", "canción", "playlist",
    ]
    .iter()
    .any(|k| lower.contains(k))
    {
        categories.push("music");
    }
    if [
        "clima",
        "tiempo",
        "temperatura",
        "lluvia",
        "llueve",
        "pronostico",
        "pronóstico",
        "weather",
        "calor",
        "frio",
        "frío",
    ]
    .iter()
    .any(|k| lower.contains(k))
    {
        categories.push("weather");
    }
    if [
        "http://",
        "https://",
        "resum",
        "articulo",
        "artículo",
        "pagina",
        "página",
        "link",
    ]
    .iter()
    .any(|k| lower.contains(k))
    {
        categories.push("web_fetch");
    }
    if [
        "dolar", "dólar", "euro", "cotiza", "cambio", "usd", "eur", "brl", "moneda", "divisa",
    ]
    .iter()
    .any(|k| lower.contains(k))
    {
        categories.push("exchange");
    }
    if [
        "busca",
        "buscar",
        "internet",
        "noticias",
        "novedades",
        "ultima",
        "última",
        "actual",
        "semana",
        "reciente",
        "version",
        "versión",
    ]
    .iter()
    .any(|k| lower.contains(k))
    {
        categories.push("web_search");
    }

    let specs = all_tool_specs();
    if categories.is_empty() {
        return specs_to_tool_defs(&specs);
    }

    let filtered: Vec<ToolSpec> = specs
        .into_iter()
        .filter(|s| categories.contains(&s.category) || s.name == "list_tasks")
        .collect();
    specs_to_tool_defs(&filtered)
}
