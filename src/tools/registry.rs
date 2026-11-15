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

    if ["tarea", "pendiente", "anota", "recorda", "comple", "task", "lista"]
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
    if ["pomodoro", "enfoque", "pausa", "concentr", "descanso", "timer"]
        .iter()
        .any(|k| lower.contains(k))
    {
        categories.push("pomodoro");
    }
    if ["ordena", "organiza", "escritorio", "deshacer", "undo", "archivo"]
        .iter()
        .any(|k| lower.contains(k))
    {
        categories.push("organizer");
    }
    if ["markdown", "markitdown", "pdf", "docx", "xlsx", "pptx", "convert", "compact", ".md", "/"]
        .iter()
        .any(|k| lower.contains(k))
    {
        categories.push("markdown");
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
