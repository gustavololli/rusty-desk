use crate::ai::{ToolDef, ToolFunctionDef};
use crate::config::AppConfig;
use crate::converter;
use crate::organizer::FileOrganizer;
use crate::state::{AppData, AssistantMood, TaskItem};
use serde_json::{json, Value};
use std::path::Path;

/// Returns the OpenAI-compatible tool definitions exposed to the AI model.
pub fn tool_definitions() -> Vec<ToolDef> {
    vec![
        ToolDef {
            kind: "function",
            function: ToolFunctionDef {
                name: "add_task",
                description: "Agrega una nueva tarea pendiente a la lista del usuario.",
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "title": { "type": "string", "description": "Descripción breve de la tarea" }
                    },
                    "required": ["title"]
                }),
            },
        },
        ToolDef {
            kind: "function",
            function: ToolFunctionDef {
                name: "complete_task",
                description: "Marca una tarea como completada buscando por texto.",
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Texto parcial de la tarea a completar" }
                    },
                    "required": ["query"]
                }),
            },
        },
        ToolDef {
            kind: "function",
            function: ToolFunctionDef {
                name: "list_tasks",
                description: "Lista las tareas actuales del usuario.",
                parameters: json!({ "type": "object", "properties": {} }),
            },
        },
        ToolDef {
            kind: "function",
            function: ToolFunctionDef {
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
            },
        },
        ToolDef {
            kind: "function",
            function: ToolFunctionDef {
                name: "start_pomodoro",
                description: "Inicia el temporizador de enfoque Pomodoro.",
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "minutes": { "type": "integer", "description": "Duración en minutos (opcional, por defecto 25)" }
                    }
                }),
            },
        },
        ToolDef {
            kind: "function",
            function: ToolFunctionDef {
                name: "pause_pomodoro",
                description: "Pausa o reanuda el temporizador Pomodoro.",
                parameters: json!({ "type": "object", "properties": {} }),
            },
        },
        ToolDef {
            kind: "function",
            function: ToolFunctionDef {
                name: "preview_organize_desktop",
                description: "Previsualiza cómo se organizarían los archivos sueltos del escritorio sin mover nada todavía.",
                parameters: json!({ "type": "object", "properties": {} }),
            },
        },
        ToolDef {
            kind: "function",
            function: ToolFunctionDef {
                name: "apply_organize_desktop",
                description: "Aplica la organización de archivos del escritorio moviéndolos a carpetas por tipo.",
                parameters: json!({ "type": "object", "properties": {} }),
            },
        },
        ToolDef {
            kind: "function",
            function: ToolFunctionDef {
                name: "undo_organize_desktop",
                description: "Deshace la última organización de archivos del escritorio.",
                parameters: json!({ "type": "object", "properties": {} }),
            },
        },
        ToolDef {
            kind: "function",
            function: ToolFunctionDef {
                name: "convert_to_markdown",
                description: "Convierte un archivo local (PDF, DOCX, XLSX, PPTX, HTML, CSV, XML, RSS, etc.) a Markdown usando MarkItDown local sin inferencia extra, con opción de compactar la salida para ahorrar tokens y guardar el .md junto al original.",
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Ruta absoluta del archivo a convertir a Markdown" },
                        "save_file": { "type": "boolean", "description": "Si es true, guarda el .md resultante junto al archivo original (por defecto true)" },
                        "compact": {
                            "type": "string",
                            "enum": ["none", "moderate", "line", "extreme"],
                            "description": "Nivel de compactación de la salida: 'none' (normal), 'moderate' (recomendado: preserva Markdown y quita espacios/saltos extra), 'line' (línea por línea) o 'extreme' (una sola línea). Por defecto 'moderate'."
                        }
                    },
                    "required": ["path"]
                }),
            },
        },
    ]
}

/// Executes a tool call against `AppData` and `FileOrganizer`, returning a human-readable result
/// and an optional mood update for the assistant avatar.
pub fn execute_tool(
    name: &str,
    args_json: &str,
    data: &mut AppData,
    organizer: &mut FileOrganizer,
    config: &AppConfig,
) -> (String, Option<AssistantMood>) {
    let args: Value = serde_json::from_str(args_json).unwrap_or(Value::Null);

    match name {
        "add_task" => {
            let title = args["title"].as_str().unwrap_or("Nueva tarea").trim();
            data.tasks.push(TaskItem::new(title));
            let _ = data.save();
            (
                format!("Tarea agregada: \"{title}\"."),
                Some(AssistantMood::Happy),
            )
        }
        "complete_task" => {
            let query = args["query"].as_str().unwrap_or("").to_lowercase();
            if let Some(t) = data
                .tasks
                .iter_mut()
                .find(|t| !t.done && t.title.to_lowercase().contains(&query))
            {
                t.done = true;
                let title = t.title.clone();
                let _ = data.save();
                (
                    format!("Tarea completada: \"{title}\"."),
                    Some(AssistantMood::Happy),
                )
            } else {
                (
                    format!("No encontré una tarea pendiente que coincida con \"{query}\"."),
                    None,
                )
            }
        }
        "list_tasks" => {
            if data.tasks.is_empty() {
                return ("No tenés tareas registradas.".to_string(), None);
            }
            let lines: Vec<String> = data
                .tasks
                .iter()
                .map(|t| {
                    let mark = if t.done { "[x]" } else { "[ ]" };
                    format!("{mark} {}", t.title)
                })
                .collect();
            (lines.join("\n"), None)
        }
        "log_habit" => {
            let query = args["name"].as_str().unwrap_or("agua").to_lowercase();
            let delta = args["delta"].as_i64().unwrap_or(1).max(1) as u32;
            if let Some(h) = data
                .habits
                .iter_mut()
                .find(|h| h.name.to_lowercase().contains(&query))
            {
                h.ensure_today();
                h.current_count = h.current_count.saturating_add(delta);
                let msg = format!(
                    "{} {}: {}/{} hoy.",
                    h.icon, h.name, h.current_count, h.daily_goal
                );
                let _ = data.save();
                (msg, Some(AssistantMood::Happy))
            } else {
                (
                    format!("No encontré el hábito \"{query}\"."),
                    None,
                )
            }
        }
        "start_pomodoro" => {
            let mins = args["minutes"].as_u64().map(|m| m as u32);
            data.pomodoro.start_work(mins);
            let _ = data.save();
            (
                format!(
                    "Modo Enfoque iniciado: {} minutos. ¡A concentrarse!",
                    data.pomodoro.work_minutes
                ),
                Some(AssistantMood::Focused),
            )
        }
        "pause_pomodoro" => {
            data.pomodoro.toggle_pause();
            let _ = data.save();
            let status = if data.pomodoro.running {
                "reanudado"
            } else {
                "pausado"
            };
            (format!("Pomodoro {status}."), None)
        }
        "preview_organize_desktop" => {
            let dir = Path::new(&config.organize_target_dir);
            match organizer.preview(dir) {
                Ok(plans) if plans.is_empty() => (
                    format!("Tu carpeta '{}' ya está ordenada (no hay archivos sueltos).", dir.display()),
                    Some(AssistantMood::Happy),
                ),
                Ok(plans) => {
                    let summary = format!(
                        "Encontré {} archivo(s) suelto(s) en '{}'. Podés revisarlos y confirmarlos en la pestaña Organizador o decirme \"aplica la organización\".",
                        plans.len(),
                        dir.display()
                    );
                    (summary, None)
                }
                Err(e) => (format!("Error al previsualizar: {e}"), None),
            }
        }
        "apply_organize_desktop" => {
            let dir = Path::new(&config.organize_target_dir);
            match organizer.apply(dir) {
                Ok(n) => (
                    format!("¡Listo! Se organizaron {n} archivo(s) en '{}'. Si querés revertirlo, decime \"deshacer\".", dir.display()),
                    Some(AssistantMood::Happy),
                ),
                Err(e) => (format!("Error al organizar: {e}"), None),
            }
        }
        "undo_organize_desktop" => match organizer.undo_last() {
            Ok(n) => (
                format!("Se restauraron {n} archivo(s) a su ubicación original."),
                Some(AssistantMood::Happy),
            ),
            Err(e) => (format!("No se pudo deshacer: {e}"), None),
        },
        "convert_to_markdown" => {
            let path_str = args["path"].as_str().unwrap_or("").trim();
            if path_str.is_empty() {
                return ("Falta la ruta del archivo a convertir.".to_string(), None);
            }
            let save_file = args["save_file"].as_bool().unwrap_or(true);
            let compact_mode = converter::CompactionMode::from_str_opt(args["compact"].as_str());
            let source_path = Path::new(path_str);
            match converter::convert_file_to_markdown(source_path) {
                Ok(raw_markdown) => {
                    let final_markdown = converter::compact_for_llm(&raw_markdown, compact_mode);
                    let (orig_len, comp_len, saved_pct) =
                        converter::compaction_stats(&raw_markdown, &final_markdown);
                    let compact_note = if compact_mode != converter::CompactionMode::None {
                        format!(
                            "Compactación ({}): {orig_len} -> {comp_len} caracteres (-{saved_pct:.1}%).\n",
                            compact_mode.label()
                        )
                    } else {
                        String::new()
                    };

                    let saved_note = if save_file {
                        match converter::save_markdown_beside(source_path, &final_markdown) {
                            Ok(out_path) => format!("Archivo guardado en: {}\n\n", out_path.display()),
                            Err(e) => format!("No se pudo guardar el .md ({e})\n\n"),
                        }
                    } else {
                        String::new()
                    };
                    const MAX_TOOL_CHARS: usize = 8000;
                    let preview = if final_markdown.chars().count() > MAX_TOOL_CHARS {
                        let truncated: String = final_markdown.chars().take(MAX_TOOL_CHARS).collect();
                        format!(
                            "{truncated}\n\n[... contenido truncado para el chat; el archivo completo tiene {} caracteres ...]",
                            final_markdown.chars().count()
                        )
                    } else {
                        final_markdown
                    };
                    (
                        format!("{compact_note}{saved_note}Contenido Markdown extraído:\n\n{preview}"),
                        Some(AssistantMood::Happy),
                    )
                }
                Err(e) => (format!("Error en MarkItDown: {e}"), None),
            }
        }
        other => (format!("Herramienta desconocida: {other}"), None),
    }
}
