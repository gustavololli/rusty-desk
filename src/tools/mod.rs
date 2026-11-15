pub mod commands;
pub mod registry;

use crate::ai::ToolDef;
use crate::config::AppConfig;
use crate::converter;
use crate::organizer::FileOrganizer;
use crate::persistence::NewToolExecution;
use crate::state::{AppData, AssistantMood, TaskItem};
use serde_json::Value;
use std::path::Path;
use uuid::Uuid;

pub use commands::{parse_local_command, LocalCommand};
pub use registry::{find_spec, select_tools_for_message, RiskLevel, ToolSpec};

/// Returns all OpenAI-compatible tool definitions from the centralized Tool Registry.
pub fn tool_definitions() -> Vec<ToolDef> {
    registry::specs_to_tool_defs(&registry::all_tool_specs())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionSource {
    Ui,
    LocalCommand,
    Ai,
}

impl ExecutionSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ui => "ui",
            Self::LocalCommand => "local_command",
            Self::Ai => "ai",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolRequest {
    pub tool_name: String,
    pub args: Value,
    pub source: ExecutionSource,
    pub conversation_id: Option<String>,
    pub tool_call_id: Option<String>,
    pub approved_confirmation_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingConfirmation {
    pub token: String,
    pub tool_name: String,
    pub args: Value,
    pub summary: String,
    pub conversation_id: Option<String>,
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolStatus {
    Success,
    Error,
    NeedsConfirmation(PendingConfirmation),
    AlreadyExecuted,
}

#[derive(Debug, Clone)]
pub struct ToolOutput {
    pub status: ToolStatus,
    /// Full message intended for the user / UI.
    pub ui_message: String,
    /// Compact representation intended for LLM context to save tokens.
    pub llm_message: String,
    pub mood: Option<AssistantMood>,
}

/// Binds a confirmation token strictly to the tool name and its canonical JSON arguments.
pub fn confirmation_token(tool_name: &str, args: &Value) -> String {
    let canonical = serde_json::to_string(args).unwrap_or_else(|_| "null".to_string());
    format!("{tool_name}::{canonical}")
}

pub struct ToolEngine;

impl ToolEngine {
    pub fn execute(
        req: ToolRequest,
        data: &mut AppData,
        organizer: &mut FileOrganizer,
        config: &AppConfig,
    ) -> ToolOutput {
        // 1. Deduplication guard for resumed/retried AI tool calls
        if let (Some(conv_id), Some(call_id)) = (&req.conversation_id, &req.tool_call_id) {
            if let Ok(true) = data.has_tool_execution(conv_id, call_id) {
                let msg = format!(
                    "La herramienta '{}' (id: {}) ya fue ejecutada previamente; no se repite.",
                    req.tool_name, call_id
                );
                return ToolOutput {
                    status: ToolStatus::AlreadyExecuted,
                    ui_message: msg.clone(),
                    llm_message: msg,
                    mood: None,
                };
            }
        }

        // 2. Lookup in Tool Registry
        let Some(spec) = find_spec(&req.tool_name) else {
            let err_msg = format!("Herramienta desconocida: {}", req.tool_name);
            Self::audit(data, &req, "error", None, None, Some(&err_msg));
            return ToolOutput {
                status: ToolStatus::Error,
                ui_message: err_msg.clone(),
                llm_message: err_msg,
                mood: None,
            };
        };

        // 3. Validate arguments
        if let Err(validation_err) = validate_args(&spec, &req.args) {
            Self::audit(data, &req, "error", None, None, Some(&validation_err));
            return ToolOutput {
                status: ToolStatus::Error,
                ui_message: validation_err.clone(),
                llm_message: validation_err,
                mood: None,
            };
        }

        // 4. Authorization policy (cannot be bypassed by AI)
        if spec.risk == RiskLevel::FileMutation && req.source != ExecutionSource::Ui {
            let expected_token = confirmation_token(&req.tool_name, &req.args);
            let approved = req
                .approved_confirmation_token
                .as_deref()
                .is_some_and(|t| t == expected_token);

            if !approved {
                let summary = match req.tool_name.as_str() {
                    "apply_organize_desktop" => format!(
                        "Confirmar organización de archivos en '{}'",
                        config.organize_target_dir
                    ),
                    "undo_organize_desktop" => {
                        "Confirmar deshacer la última organización de archivos".to_string()
                    }
                    _ => format!("Confirmar ejecución de '{}'", req.tool_name),
                };
                let pending = PendingConfirmation {
                    token: expected_token,
                    tool_name: req.tool_name.clone(),
                    args: req.args.clone(),
                    summary: summary.clone(),
                    conversation_id: req.conversation_id.clone(),
                    tool_call_id: req.tool_call_id.clone(),
                };
                Self::audit(
                    data,
                    &req,
                    "pending_confirmation",
                    Some(&summary),
                    None,
                    None,
                );
                let llm_msg = format!(
                    "Acción '{}' pendiente de confirmación del usuario: {}.",
                    req.tool_name, summary
                );
                return ToolOutput {
                    status: ToolStatus::NeedsConfirmation(pending),
                    ui_message: summary,
                    llm_message: llm_msg,
                    mood: None,
                };
            }
        }

        // 5. Execute business logic
        match Self::execute_inner(&req.tool_name, &req.args, data, organizer, config) {
            Ok((ui_msg, llm_msg, mood)) => {
                Self::audit(
                    data,
                    &req,
                    "success",
                    Some(&llm_msg),
                    Some(&ui_msg),
                    None,
                );
                ToolOutput {
                    status: ToolStatus::Success,
                    ui_message: ui_msg,
                    llm_message: llm_msg,
                    mood,
                }
            }
            Err(err_msg) => {
                Self::audit(data, &req, "error", None, None, Some(&err_msg));
                ToolOutput {
                    status: ToolStatus::Error,
                    ui_message: err_msg.clone(),
                    llm_message: err_msg,
                    mood: None,
                }
            }
        }
    }

    fn audit(
        data: &AppData,
        req: &ToolRequest,
        status: &str,
        summary: Option<&str>,
        full: Option<&str>,
        error: Option<&str>,
    ) {
        let _ = data.record_tool_execution(&NewToolExecution {
            id: Uuid::new_v4().to_string(),
            conversation_id: req.conversation_id.clone(),
            message_id: None,
            tool_call_id: req.tool_call_id.clone(),
            tool_name: req.tool_name.clone(),
            source: req.source.as_str().to_string(),
            arguments_json: serde_json::to_string(&req.args).unwrap_or_else(|_| "{}".to_string()),
            status: status.to_string(),
            result_summary: summary.map(str::to_string),
            result_full: full.map(str::to_string),
            error_message: error.map(str::to_string),
        });
    }

    fn execute_inner(
        name: &str,
        args: &Value,
        data: &mut AppData,
        organizer: &mut FileOrganizer,
        config: &AppConfig,
    ) -> Result<(String, String, Option<AssistantMood>), String> {
        match name {
            "add_task" => {
                let title = args["title"].as_str().unwrap_or("").trim();
                let item = TaskItem::new(title);
                data.upsert_task(&item)
                    .map_err(|e| format!("Error guardando tarea en SQLite: {e}"))?;
                data.tasks.push(item);
                let msg = format!("Tarea agregada: \"{title}\".");
                Ok((msg.clone(), msg, Some(AssistantMood::Happy)))
            }
            "complete_task" => {
                let query = args["query"].as_str().unwrap_or("").trim().to_lowercase();
                if let Some(pos) = data
                    .tasks
                    .iter()
                    .position(|t| !t.done && t.title.to_lowercase().contains(&query))
                {
                    data.tasks[pos].done = true;
                    let item = data.tasks[pos].clone();
                    if let Err(e) = data.upsert_task(&item) {
                        data.tasks[pos].done = false;
                        return Err(format!("Error actualizando tarea en SQLite: {e}"));
                    }
                    let msg = format!("Tarea completada: \"{}\".", item.title);
                    Ok((msg.clone(), msg, Some(AssistantMood::Happy)))
                } else {
                    Err(format!(
                        "No encontré una tarea pendiente que coincida con \"{query}\"."
                    ))
                }
            }
            "list_tasks" => {
                if data.tasks.is_empty() {
                    let msg = "No tenés tareas registradas.".to_string();
                    return Ok((msg.clone(), msg, None));
                }
                let lines: Vec<String> = data
                    .tasks
                    .iter()
                    .map(|t| {
                        let mark = if t.done { "[x]" } else { "[ ]" };
                        format!("{mark} {}", t.title)
                    })
                    .collect();
                let full = lines.join("\n");
                let compact = if lines.len() > 25 {
                    format!(
                        "{}\n[... {} tareas más en la pestaña Tareas ...]",
                        lines[..25].join("\n"),
                        lines.len() - 25
                    )
                } else {
                    full.clone()
                };
                Ok((full, compact, None))
            }
            "log_habit" => {
                let query = args["name"].as_str().unwrap_or("agua").trim().to_lowercase();
                let delta = args["delta"].as_i64().unwrap_or(1).max(1) as i32;
                if let Some(idx) = data
                    .habits
                    .iter()
                    .position(|h| h.name.to_lowercase().contains(&query))
                {
                    data.log_habit_delta(idx, delta)
                        .map_err(|e| format!("Error registrando hábito en SQLite: {e}"))?;
                    let h = &data.habits[idx];
                    let msg = format!("{} {}: {}/{} hoy.", h.icon, h.name, h.current_count, h.daily_goal);
                    Ok((msg.clone(), msg, Some(AssistantMood::Happy)))
                } else {
                    Err(format!("No encontré el hábito \"{query}\"."))
                }
            }
            "start_pomodoro" => {
                let mins = args["minutes"].as_u64().map(|m| m as u32);
                data.pomodoro.start_work(mins);
                data.save_pomodoro()
                    .map_err(|e| format!("Error guardando Pomodoro en SQLite: {e}"))?;
                let msg = format!(
                    "Modo Enfoque iniciado: {} minutos. ¡A concentrarse!",
                    data.pomodoro.work_minutes
                );
                Ok((msg.clone(), msg, Some(AssistantMood::Focused)))
            }
            "pause_pomodoro" => {
                data.pomodoro.toggle_pause();
                data.save_pomodoro()
                    .map_err(|e| format!("Error guardando Pomodoro en SQLite: {e}"))?;
                let status = if data.pomodoro.running {
                    "reanudado"
                } else {
                    "pausado"
                };
                let msg = format!("Pomodoro {status}.");
                Ok((msg.clone(), msg, None))
            }
            "preview_organize_desktop" => {
                let dir = Path::new(&config.organize_target_dir);
                let plans = organizer
                    .preview(dir)
                    .map_err(|e| format!("Error al previsualizar: {e}"))?;
                if plans.is_empty() {
                    let msg = format!(
                        "Tu carpeta '{}' ya está ordenada (no hay archivos sueltos).",
                        dir.display()
                    );
                    Ok((msg.clone(), msg, Some(AssistantMood::Happy)))
                } else {
                    let full_list: Vec<String> = plans
                        .iter()
                        .take(30)
                        .map(|p| format!("- [{}] {}", p.category, p.file_name))
                        .collect();
                    let ui_msg = format!(
                        "Encontré {} archivo(s) suelto(s) en '{}':\n{}",
                        plans.len(),
                        dir.display(),
                        full_list.join("\n")
                    );
                    let llm_msg = format!(
                        "Encontré {} archivo(s) suelto(s) en '{}'. El usuario puede revisarlos en Ordenar o confirmar para aplicar.",
                        plans.len(),
                        dir.display()
                    );
                    Ok((ui_msg, llm_msg, None))
                }
            }
            "apply_organize_desktop" => {
                let dir = Path::new(&config.organize_target_dir);
                let n = organizer
                    .apply(dir)
                    .map_err(|e| format!("Error al organizar: {e}"))?;
                let msg = format!(
                    "¡Listo! Se organizaron {n} archivo(s) en '{}'.",
                    dir.display()
                );
                Ok((msg.clone(), msg, Some(AssistantMood::Happy)))
            }
            "undo_organize_desktop" => {
                let n = organizer
                    .undo_last()
                    .map_err(|e| format!("No se pudo deshacer: {e}"))?;
                let msg = format!("Se restauraron {n} archivo(s) a su ubicación original.");
                Ok((msg.clone(), msg, Some(AssistantMood::Happy)))
            }
            "convert_to_markdown" => {
                let path_str = args["path"].as_str().unwrap_or("").trim();
                let save_file = args["save_file"].as_bool().unwrap_or(true);
                let compact_mode = converter::CompactionMode::from_str_opt(args["compact"].as_str());
                let source_path = Path::new(path_str);

                let raw_markdown = converter::convert_file_to_markdown(source_path)?;
                let final_markdown = converter::compact_for_llm(&raw_markdown, compact_mode);
                let (orig_len, comp_len, saved_pct) =
                    converter::compaction_stats(&raw_markdown, &final_markdown);

                let compact_note = if compact_mode != converter::CompactionMode::None {
                    format!(
                        "Compactación ({}): {orig_len} -> {comp_len} chars (-{saved_pct:.1}%).\n",
                        compact_mode.label()
                    )
                } else {
                    String::new()
                };

                let saved_path_str = if save_file {
                    let out_path = converter::save_markdown_beside(source_path, &final_markdown)?;
                    format!("Archivo guardado en: {}\n", out_path.display())
                } else {
                    String::new()
                };

                let ui_msg = format!("{compact_note}{saved_path_str}\n{final_markdown}");
                const MAX_LLM_CHARS: usize = 6000;
                let llm_preview = if final_markdown.chars().count() > MAX_LLM_CHARS {
                    let truncated: String = final_markdown.chars().take(MAX_LLM_CHARS).collect();
                    format!(
                        "{truncated}\n\n[... vista acotada a {MAX_LLM_CHARS} de {comp_len} chars; el documento completo está disponible localmente ...]"
                    )
                } else {
                    final_markdown
                };
                let llm_msg = format!("{compact_note}{saved_path_str}Contenido:\n\n{llm_preview}");
                Ok((ui_msg, llm_msg, Some(AssistantMood::Happy)))
            }
            other => Err(format!("Herramienta desconocida: {other}")),
        }
    }
}

fn validate_args(spec: &ToolSpec, args: &Value) -> Result<(), String> {
    match spec.name {
        "add_task" => {
            if args["title"].as_str().unwrap_or("").trim().is_empty() {
                return Err("Argumento inválido: 'title' no puede estar vacío.".to_string());
            }
        }
        "complete_task" => {
            if args["query"].as_str().unwrap_or("").trim().is_empty() {
                return Err("Argumento inválido: 'query' no puede estar vacío.".to_string());
            }
        }
        "log_habit" => {
            if args["name"].as_str().unwrap_or("").trim().is_empty() {
                return Err("Argumento inválido: 'name' no puede estar vacío.".to_string());
            }
            if let Some(d) = args.get("delta").and_then(|v| v.as_i64()) {
                if d <= 0 || d > 1000 {
                    return Err("Argumento inválido: 'delta' debe estar entre 1 y 1000.".to_string());
                }
            }
        }
        "start_pomodoro" => {
            if let Some(m) = args.get("minutes").and_then(|v| v.as_i64()) {
                if m < 1 || m > 180 {
                    return Err("Argumento inválido: 'minutes' debe estar entre 1 y 180.".to_string());
                }
            }
        }
        "convert_to_markdown" => {
            if args["path"].as_str().unwrap_or("").trim().is_empty() {
                return Err("Argumento inválido: 'path' no puede estar vacío.".to_string());
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::Database;
    use serde_json::json;

    fn test_data() -> AppData {
        let db = Database::open_in_memory().unwrap();
        AppData::from_database(db).unwrap()
    }

    #[test]
    fn engine_validates_args_unknown_tools_and_audits() {
        let mut data = test_data();
        let mut org = FileOrganizer::default();
        let cfg = AppConfig::default();

        // Unknown tool
        let out_unknown = ToolEngine::execute(
            ToolRequest {
                tool_name: "hack_system".to_string(),
                args: json!({}),
                source: ExecutionSource::Ai,
                conversation_id: None,
                tool_call_id: None,
                approved_confirmation_token: None,
            },
            &mut data,
            &mut org,
            &cfg,
        );
        assert_eq!(out_unknown.status, ToolStatus::Error);

        // Invalid empty title
        let out_invalid = ToolEngine::execute(
            ToolRequest {
                tool_name: "add_task".to_string(),
                args: json!({ "title": "   " }),
                source: ExecutionSource::Ai,
                conversation_id: None,
                tool_call_id: None,
                approved_confirmation_token: None,
            },
            &mut data,
            &mut org,
            &cfg,
        );
        assert_eq!(out_invalid.status, ToolStatus::Error);
    }

    #[test]
    fn engine_enforces_confirmation_and_deduplicates_tool_calls() {
        let mut data = test_data();
        let mut org = FileOrganizer::default();
        let mut cfg = AppConfig::default();
        let temp_dir = std::env::temp_dir().join(format!("rusty_desk_engine_test_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        std::fs::write(temp_dir.join("nota.txt"), "hola").unwrap();
        cfg.organize_target_dir = temp_dir.to_string_lossy().to_string();

        let conv_id = "conv-confirm-test";
        data.db
            .as_ref()
            .unwrap()
            .ensure_conversation(conv_id, "Test")
            .unwrap();

        // 1. AI tries to apply_organize_desktop -> MUST ask confirmation
        let out1 = ToolEngine::execute(
            ToolRequest {
                tool_name: "apply_organize_desktop".to_string(),
                args: json!({}),
                source: ExecutionSource::Ai,
                conversation_id: Some(conv_id.to_string()),
                tool_call_id: Some("call-1".to_string()),
                approved_confirmation_token: None,
            },
            &mut data,
            &mut org,
            &cfg,
        );
        let pending = match out1.status {
            ToolStatus::NeedsConfirmation(p) => p,
            other => panic!("Expected NeedsConfirmation, got {other:?}"),
        };

        // Wrong token for a different operation must NOT authorize it
        let out_wrong = ToolEngine::execute(
            ToolRequest {
                tool_name: "apply_organize_desktop".to_string(),
                args: json!({}),
                source: ExecutionSource::Ai,
                conversation_id: Some(conv_id.to_string()),
                tool_call_id: Some("call-2".to_string()),
                approved_confirmation_token: Some("undo_organize_desktop::{}".to_string()),
            },
            &mut data,
            &mut org,
            &cfg,
        );
        assert!(matches!(out_wrong.status, ToolStatus::NeedsConfirmation(_)));

        // 2. Approved with exact bound token -> succeeds
        let out_ok = ToolEngine::execute(
            ToolRequest {
                tool_name: "apply_organize_desktop".to_string(),
                args: json!({}),
                source: ExecutionSource::Ai,
                conversation_id: Some(conv_id.to_string()),
                tool_call_id: Some("call-3".to_string()),
                approved_confirmation_token: Some(pending.token),
            },
            &mut data,
            &mut org,
            &cfg,
        );
        assert_eq!(out_ok.status, ToolStatus::Success);
        assert!(temp_dir.join("Documentos").join("nota.txt").exists());

        // 3. Re-running same conversation_id + tool_call_id -> deduplicated
        let out_dup = ToolEngine::execute(
            ToolRequest {
                tool_name: "apply_organize_desktop".to_string(),
                args: json!({}),
                source: ExecutionSource::Ai,
                conversation_id: Some(conv_id.to_string()),
                tool_call_id: Some("call-3".to_string()),
                approved_confirmation_token: None,
            },
            &mut data,
            &mut org,
            &cfg,
        );
        assert_eq!(out_dup.status, ToolStatus::AlreadyExecuted);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn local_command_parser_and_tool_selection_work_without_llm() {
        assert_eq!(
            parse_local_command("/pomodoro 15"),
            Some(LocalCommand::Tool {
                name: "start_pomodoro",
                args: json!({ "minutes": 15 })
            })
        );
        assert_eq!(
            parse_local_command("/tarea Comprar leche"),
            Some(LocalCommand::Tool {
                name: "add_task",
                args: json!({ "title": "Comprar leche" })
            })
        );
        assert!(parse_local_command("hola rusty").is_none());

        let selected = select_tools_for_message("inicia un pomodoro de 25 minutos");
        assert!(selected.iter().any(|t| t.function.name == "start_pomodoro"));
        assert!(selected.len() < tool_definitions().len());
    }
}
