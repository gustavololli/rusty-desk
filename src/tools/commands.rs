use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq)]
pub enum LocalCommand {
    Tool { name: &'static str, args: Value },
    Help(String),
    Invalid(String),
}

const HELP_TEXT: &str = "Comandos locales (sin gastar tokens de IA):
- `/tarea <título>` — Agrega una tarea
- `/completar <texto>` — Marca una tarea como completada
- `/tareas` — Lista tus tareas
- `/agua [cantidad]` — Suma vasos de agua (por defecto 1)
- `/habito <nombre> [cantidad]` — Registra progreso en un hábito
- `/pomodoro [minutos]` — Inicia Pomodoro (por defecto 25m)
- `/pausa` — Pausa o reanuda Pomodoro
- `/ordenar [preview|aplicar|deshacer]` — Organiza el escritorio
- `/md <ruta>` — Convierte un archivo a Markdown local
- `/ayuda` — Muestra esta ayuda";

/// Parses explicit `/` slash commands so deterministic operations bypass the LLM completely.
pub fn parse_local_command(input: &str) -> Option<LocalCommand> {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
        return None;
    }

    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let cmd = parts.next().unwrap_or("").to_lowercase();
    let rest = parts.next().unwrap_or("").trim();

    Some(match cmd.as_str() {
        "/ayuda" | "/help" => LocalCommand::Help(HELP_TEXT.to_string()),
        "/tarea" | "/task" => {
            if rest.is_empty() {
                LocalCommand::Invalid("Uso: `/tarea <descripción de la tarea>`".to_string())
            } else {
                LocalCommand::Tool {
                    name: "add_task",
                    args: json!({ "title": rest }),
                }
            }
        }
        "/completar" | "/done" => {
            if rest.is_empty() {
                LocalCommand::Invalid("Uso: `/completar <texto de la tarea>`".to_string())
            } else {
                LocalCommand::Tool {
                    name: "complete_task",
                    args: json!({ "query": rest }),
                }
            }
        }
        "/tareas" | "/tasks" => LocalCommand::Tool {
            name: "list_tasks",
            args: json!({}),
        },
        "/agua" => {
            let delta = rest.parse::<i64>().unwrap_or(1).max(1);
            LocalCommand::Tool {
                name: "log_habit",
                args: json!({ "name": "Vasos de agua", "delta": delta }),
            }
        }
        "/habito" | "/habit" => {
            if rest.is_empty() {
                LocalCommand::Invalid("Uso: `/habito <nombre> [cantidad]`".to_string())
            } else {
                let tokens: Vec<&str> = rest.split_whitespace().collect();
                let (name, delta) = if tokens.len() >= 2 {
                    if let Ok(n) = tokens.last().unwrap().parse::<i64>() {
                        (tokens[..tokens.len() - 1].join(" "), n.max(1))
                    } else {
                        (rest.to_string(), 1)
                    }
                } else {
                    (rest.to_string(), 1)
                };
                LocalCommand::Tool {
                    name: "log_habit",
                    args: json!({ "name": name, "delta": delta }),
                }
            }
        }
        "/pomodoro" => {
            let args = if rest.is_empty() {
                json!({})
            } else if let Ok(mins) = rest.parse::<u64>() {
                json!({ "minutes": mins })
            } else {
                return Some(LocalCommand::Invalid(
                    "Uso: `/pomodoro [minutos]` (ej: `/pomodoro 25`)".to_string(),
                ));
            };
            LocalCommand::Tool {
                name: "start_pomodoro",
                args,
            }
        }
        "/pausa" | "/pause" => LocalCommand::Tool {
            name: "pause_pomodoro",
            args: json!({}),
        },
        "/ordenar" => match rest.to_lowercase().as_str() {
            "" | "preview" | "ver" => LocalCommand::Tool {
                name: "preview_organize_desktop",
                args: json!({}),
            },
            "aplicar" | "apply" => LocalCommand::Tool {
                name: "apply_organize_desktop",
                args: json!({}),
            },
            "deshacer" | "undo" => LocalCommand::Tool {
                name: "undo_organize_desktop",
                args: json!({}),
            },
            _ => LocalCommand::Invalid("Uso: `/ordenar [preview|aplicar|deshacer]`".to_string()),
        },
        "/md" | "/markitdown" => {
            if rest.is_empty() {
                LocalCommand::Invalid("Uso: `/md <ruta-al-archivo>`".to_string())
            } else {
                LocalCommand::Tool {
                    name: "convert_to_markdown",
                    args: json!({ "path": rest, "save_file": true, "compact": "moderate" }),
                }
            }
        }
        _ => LocalCommand::Invalid(format!(
            "Comando desconocido `{cmd}`. Escribí `/ayuda` para ver los comandos locales."
        )),
    })
}
