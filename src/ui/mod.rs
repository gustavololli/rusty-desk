pub mod avatar;
pub mod icons8bit;

use crate::ai::{
    AiProvider, ChatMessage, GoogleGeminiProvider, MockProvider, OpenAiCompatibleProvider,
};
use crate::config::AppConfig;
use crate::converter::{self, CompactionMode};
use crate::organizer::FileOrganizer;
use crate::persistence::{ConversationSummary, MessageKind};
use crate::sfx::{self, SfxEvent};
use crate::state::{
    format_local_datetime_from_utc, AppData, AssistantMood, HabitItem, PomodoroPhase, TaskItem,
    UiChatEntry,
};
use crate::tools::{
    parse_local_command, select_tools_for_message, ExecutionSource, LocalCommand,
    PendingConfirmation, ToolEngine, ToolRequest, ToolStatus,
};
use crate::ui::avatar::draw_avatar;
use crate::ui::icons8bit::{paint_pixel_icon, pixel_button, PixelIcon};
use crate::voice::{speak_local_async, ActiveRecording};
use eframe::egui::{self, Color32, Pos2, RichText, Rounding, Stroke, Vec2};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveTab {
    Chat,
    Tasks,
    Habits,
    Focus,
    Organizer,
    Markdown,
    Compact,
    Settings,
}

enum BgEvent {
    AiReply {
        assistant_text: String,
        tool_calls: Vec<crate::ai::ToolCall>,
        raw_gemini_parts: Option<Vec<serde_json::Value>>,
        history_snapshot: Vec<ChatMessage>,
    },
    AiError(String),
    Transcribed(String),
    VoiceError(String),
    MarkdownConverted {
        source: PathBuf,
        markdown: String,
        saved_path: Option<PathBuf>,
    },
    MarkdownError(String),
    FilesPicked {
        for_chat: bool,
        paths: Vec<PathBuf>,
    },
    FilePickError(String),
}

pub struct RustyDeskApp {
    rt: tokio::runtime::Runtime,
    config: AppConfig,
    data: AppData,
    organizer: FileOrganizer,

    compact_mode: bool,
    always_on_top: bool,
    active_tab: ActiveTab,
    mood: AssistantMood,

    chat_ui: Vec<UiChatEntry>,
    chat_history: Vec<ChatMessage>,
    chat_input: String,
    is_thinking: bool,
    current_chat_id: String,
    show_chat_history: bool,
    history_page: usize,
    pending_confirmation: Option<PendingConfirmation>,
    system_prompt: String,
    welcome_text: String,

    new_task_input: String,
    new_habit_name: String,
    new_habit_icon: String,
    new_habit_goal: u32,
    status_banner: Option<String>,

    active_recording: Option<ActiveRecording>,
    last_pomodoro_tick: Instant,

    md_is_converting: bool,
    md_auto_save: bool,
    md_compact_mode: CompactionMode,
    md_source_path: Option<PathBuf>,
    md_saved_path: Option<PathBuf>,
    md_output: String,
    md_error: Option<String>,

    compact_input: String,
    compact_mode_tab: CompactionMode,

    bg_tx: Sender<BgEvent>,
    bg_rx: Receiver<BgEvent>,
    global_cursor: Arc<Mutex<Option<(i16, i16)>>>,
}

impl RustyDeskApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_theme(&cc.egui_ctx);

        let config = AppConfig::load();
        let mut data = AppData::load();
        let initial_banner = data
            .startup_error
            .take()
            .map(|e| format!("⚠️ {e}"))
            .or_else(|| data.migration_notice.take());
        let (bg_tx, bg_rx) = mpsc::channel();
        let global_cursor = spawn_global_cursor_tracker();

        let system_prompt = format!(
            "Sos {}, un asistente de escritorio amigable y conciso inspirado en Troko-Desk. \
            Ayudás al usuario con sus tareas, hábitos, temporizador Pomodoro, organización de archivos y conversión de documentos. \
            Cuando el usuario te envíe o adjunte la ruta de un archivo en el chat, ofrecé convertirlo a Markdown usando la herramienta `convert_to_markdown` (aclarando que podés hacerlo en modo normal o compactado para ahorrar tokens), e invocala cuando el usuario acepte o te lo pida. \
            Respondé siempre en español salvo que el usuario hable en otro idioma.",
            config.assistant_name
        );

        let welcome = format!(
            "¡Hola! Soy {}. Puedo anotar tareas, registrar tus hábitos, iniciar el modo enfoque Pomodoro y organizar tu escritorio.",
            config.assistant_name
        );

        sfx::play(SfxEvent::Startup, config.sfx_enabled);

        Self {
            rt: tokio::runtime::Runtime::new().expect("No se pudo iniciar Tokio"),
            config,
            data,
            organizer: FileOrganizer::default(),
            compact_mode: false,
            always_on_top: true,
            active_tab: ActiveTab::Chat,
            mood: AssistantMood::Idle,
            chat_ui: vec![UiChatEntry::assistant(&welcome)],
            chat_history: vec![ChatMessage::system(&system_prompt)],
            chat_input: String::new(),
            is_thinking: false,
            current_chat_id: Uuid::new_v4().to_string(),
            show_chat_history: false,
            history_page: 0,
            pending_confirmation: None,
            system_prompt,
            welcome_text: welcome,
            new_task_input: String::new(),
            new_habit_name: String::new(),
            new_habit_icon: "✨".to_string(),
            new_habit_goal: 1,
            status_banner: initial_banner,
            active_recording: None,
            last_pomodoro_tick: Instant::now(),
            md_is_converting: false,
            md_auto_save: true,
            md_compact_mode: CompactionMode::Moderate,
            md_source_path: None,
            md_saved_path: None,
            md_output: String::new(),
            md_error: None,
            compact_input: String::new(),
            compact_mode_tab: CompactionMode::Moderate,
            bg_tx,
            bg_rx,
            global_cursor,
        }
    }

    fn mood_accent(&self) -> Color32 {
        match self.mood {
            AssistantMood::Idle => Color32::from_rgb(56, 189, 248),
            AssistantMood::Listening => Color32::from_rgb(244, 63, 94),
            AssistantMood::Thinking => Color32::from_rgb(168, 85, 247),
            AssistantMood::Happy => Color32::from_rgb(52, 211, 153),
            AssistantMood::Focused => Color32::from_rgb(99, 102, 241),
        }
    }

    fn build_provider(&self) -> Arc<dyn AiProvider> {
        if self.config.should_use_mock() {
            Arc::new(MockProvider)
        } else if self.config.base_url.contains("generativelanguage.googleapis.com") {
            let clean_base = self
                .config
                .base_url
                .trim_end_matches('/')
                .trim_end_matches("/openai")
                .to_string();
            Arc::new(GoogleGeminiProvider::new(
                clean_base,
                self.config.api_key.clone(),
                self.config.model.clone(),
            ))
        } else {
            Arc::new(OpenAiCompatibleProvider::new(
                self.config.base_url.clone(),
                self.config.api_key.clone(),
                self.config.model.clone(),
                self.config.stt_model.clone(),
            ))
        }
    }

    fn report_persistence_result<T>(&mut self, res: anyhow::Result<T>) {
        if let Err(e) = res {
            self.status_banner = Some(format!("⚠️ Error de persistencia SQLite: {e}"));
        }
    }

    fn send_user_message(&mut self, text: String, ctx: &egui::Context) {
        let trimmed = text.trim().to_string();
        if trimmed.is_empty() || self.is_thinking {
            return;
        }

        let user_msg = ChatMessage::user(&trimmed);
        self.chat_ui.push(UiChatEntry::user(&trimmed));
        self.chat_history.push(user_msg.clone());
        let persist_res = self.data.record_chat_message(
            &self.current_chat_id,
            Some(&trimmed),
            &user_msg,
            MessageKind::Text,
        );
        self.report_persistence_result(persist_res);
        sfx::play(SfxEvent::MessageSent, self.config.sfx_enabled);

        // Intercept explicit `/` commands locally (0 LLM tokens!)
        if let Some(local_cmd) = parse_local_command(&trimmed) {
            match local_cmd {
                LocalCommand::Help(help_text) => {
                    let reply = ChatMessage::assistant_text(&help_text);
                    self.chat_ui.push(UiChatEntry::assistant(&help_text));
                    self.chat_history.push(reply.clone());
                    let r = self.data.record_chat_message(
                        &self.current_chat_id,
                        None,
                        &reply,
                        MessageKind::Text,
                    );
                    self.report_persistence_result(r);
                }
                LocalCommand::Invalid(err_text) => {
                    let msg = format!("⚠️ {err_text}");
                    let reply = ChatMessage::assistant_text(&msg);
                    self.chat_ui.push(UiChatEntry::assistant(&msg));
                    self.chat_history.push(reply.clone());
                    let r = self.data.record_chat_message(
                        &self.current_chat_id,
                        None,
                        &reply,
                        MessageKind::Text,
                    );
                    self.report_persistence_result(r);
                }
                LocalCommand::Tool { name, args } => {
                    let out = ToolEngine::execute(
                        ToolRequest {
                            tool_name: name.to_string(),
                            args,
                            source: ExecutionSource::LocalCommand,
                            conversation_id: Some(self.current_chat_id.clone()),
                            tool_call_id: None,
                            approved_confirmation_token: None,
                        },
                        &mut self.data,
                        &mut self.organizer,
                        &self.config,
                    );
                    if let Some(m) = out.mood {
                        self.mood = m;
                    }
                    match out.status {
                        ToolStatus::NeedsConfirmation(pending) => {
                            let msg = format!("Confirmación requerida: {}", pending.summary);
                            self.pending_confirmation = Some(pending);
                            let reply = ChatMessage::assistant_text(&msg);
                            self.chat_ui.push(UiChatEntry::assistant(&msg));
                            self.chat_history.push(reply);
                        }
                        _ => {
                            let reply_text = if out.status == ToolStatus::Error {
                                format!("⚠️ {}", out.ui_message)
                            } else {
                                out.ui_message
                            };
                            let reply = ChatMessage::assistant_text(&reply_text);
                            self.chat_ui.push(UiChatEntry::assistant(&reply_text));
                            self.chat_history.push(reply.clone());
                            let r = self.data.record_chat_message(
                                &self.current_chat_id,
                                None,
                                &reply,
                                MessageKind::Text,
                            );
                            self.report_persistence_result(r);
                        }
                    }
                    sfx::play(SfxEvent::MessageReceived, self.config.sfx_enabled);
                }
            }
            return;
        }

        self.is_thinking = true;
        self.mood = AssistantMood::Thinking;
        self.spawn_ai_turn(ctx.clone(), Some(trimmed));
    }

    fn spawn_ai_turn(&self, ctx: egui::Context, latest_user_text: Option<String>) {
        let provider = self.build_provider();
        let history = self.chat_history.clone();
        let tools = match latest_user_text.as_deref() {
            Some(txt) => select_tools_for_message(txt),
            None => crate::tools::tool_definitions(),
        };
        let tx = self.bg_tx.clone();

        self.rt.spawn(async move {
            match provider.chat(&history, &tools).await {
                Ok(resp) => {
                    let _ = tx.send(BgEvent::AiReply {
                        assistant_text: resp.content.unwrap_or_default(),
                        tool_calls: resp.tool_calls,
                        raw_gemini_parts: resp.raw_gemini_parts,
                        history_snapshot: history,
                    });
                }
                Err(e) => {
                    let _ = tx.send(BgEvent::AiError(e.to_string()));
                }
            }
            ctx.request_repaint();
        });
    }

    fn toggle_voice_recording(&mut self, ctx: &egui::Context) {
        if let Some(rec) = self.active_recording.take() {
            match rec.stop_to_wav() {
                Ok(wav) => {
                    self.mood = AssistantMood::Thinking;
                    self.status_banner = Some("Transcribiendo audio...".to_string());
                    let provider = self.build_provider();
                    let tx = self.bg_tx.clone();
                    let ctx_clone = ctx.clone();

                    self.rt.spawn(async move {
                        match provider.transcribe(wav).await {
                            Ok(text) => {
                                let _ = tx.send(BgEvent::Transcribed(text));
                            }
                            Err(e) => {
                                let _ = tx.send(BgEvent::VoiceError(e.to_string()));
                            }
                        }
                        ctx_clone.request_repaint();
                    });
                }
                Err(e) => {
                    self.mood = AssistantMood::Idle;
                    self.status_banner = Some(format!("⚠️ {e}"));
                }
            }
        } else {
            match ActiveRecording::start() {
                Ok(rec) => {
                    self.active_recording = Some(rec);
                    self.mood = AssistantMood::Listening;
                    self.status_banner =
                        Some("Escuchando... volvé a tocar el micrófono para enviar.".to_string());
                }
                Err(e) => {
                    self.status_banner = Some(format!("Micrófono no disponible: {e}"));
                }
            }
        }
    }

    fn process_bg_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.bg_rx.try_recv() {
            match event {
                BgEvent::AiReply {
                    assistant_text,
                    tool_calls,
                    raw_gemini_parts,
                    mut history_snapshot,
                } => {
                    if !tool_calls.is_empty() {
                        let assistant_tool_msg = ChatMessage {
                            role: "assistant".to_string(),
                            content: if assistant_text.is_empty() {
                                None
                            } else {
                                Some(assistant_text.clone())
                            },
                            tool_calls: Some(tool_calls.clone()),
                            tool_call_id: None,
                            raw_gemini_parts,
                        };
                        let persist_tc = self.data.record_chat_message(
                            &self.current_chat_id,
                            None,
                            &assistant_tool_msg,
                            MessageKind::ToolCall,
                        );
                        self.report_persistence_result(persist_tc);
                        history_snapshot.push(assistant_tool_msg);

                        let mut new_mood = None;
                        let mut waiting_for_confirmation = false;
                        for tc in &tool_calls {
                            let args_val: serde_json::Value =
                                serde_json::from_str(&tc.function.arguments)
                                    .unwrap_or(serde_json::Value::Null);
                            let out = ToolEngine::execute(
                                ToolRequest {
                                    tool_name: tc.function.name.clone(),
                                    args: args_val,
                                    source: ExecutionSource::Ai,
                                    conversation_id: Some(self.current_chat_id.clone()),
                                    tool_call_id: Some(tc.id.clone()),
                                    approved_confirmation_token: None,
                                },
                                &mut self.data,
                                &mut self.organizer,
                                &self.config,
                            );
                            if out.mood.is_some() {
                                new_mood = out.mood;
                            }

                            match out.status {
                                ToolStatus::NeedsConfirmation(pending) => {
                                    waiting_for_confirmation = true;
                                    self.pending_confirmation = Some(pending);
                                    let tool_msg = ChatMessage::tool_result(&tc.id, out.llm_message);
                                    let r = self.data.record_chat_message(
                                        &self.current_chat_id,
                                        None,
                                        &tool_msg,
                                        MessageKind::ToolResult,
                                    );
                                    self.report_persistence_result(r);
                                    history_snapshot.push(tool_msg);
                                }
                                ToolStatus::Error => {
                                    let tool_msg = ChatMessage::tool_result(&tc.id, out.llm_message);
                                    let r = self.data.record_chat_message(
                                        &self.current_chat_id,
                                        None,
                                        &tool_msg,
                                        MessageKind::ToolError,
                                    );
                                    self.report_persistence_result(r);
                                    history_snapshot.push(tool_msg);
                                }
                                _ => {
                                    let tool_msg = ChatMessage::tool_result(&tc.id, out.llm_message);
                                    let r = self.data.record_chat_message(
                                        &self.current_chat_id,
                                        None,
                                        &tool_msg,
                                        MessageKind::ToolResult,
                                    );
                                    self.report_persistence_result(r);
                                    history_snapshot.push(tool_msg);
                                }
                            }
                        }

                        if let Some(m) = new_mood {
                            self.mood = m;
                        }
                        self.chat_history = history_snapshot;
                        if waiting_for_confirmation {
                            self.is_thinking = false;
                            self.mood = AssistantMood::Idle;
                        } else {
                            self.spawn_ai_turn(ctx.clone(), None);
                        }
                    } else {
                        self.is_thinking = false;
                        if self.mood == AssistantMood::Thinking {
                            self.mood = if self.data.pomodoro.running {
                                AssistantMood::Focused
                            } else {
                                AssistantMood::Idle
                            };
                        }
                        let final_text = if assistant_text.trim().is_empty() {
                            "Listo.".to_string()
                        } else {
                            assistant_text
                        };
                        let reply_msg = ChatMessage::assistant_text(&final_text);
                        self.chat_history.push(reply_msg.clone());
                        self.chat_ui.push(UiChatEntry::assistant(&final_text));
                        let r = self.data.record_chat_message(
                            &self.current_chat_id,
                            None,
                            &reply_msg,
                            MessageKind::Text,
                        );
                        self.report_persistence_result(r);
                        sfx::play(SfxEvent::MessageReceived, self.config.sfx_enabled);
                        if self.config.tts_enabled {
                            speak_local_async(final_text);
                        }
                    }
                }
                BgEvent::AiError(err) => {
                    self.is_thinking = false;
                    self.mood = AssistantMood::Idle;
                    // Roll back poisoned tool turns so subsequent messages keep working
                    while self
                        .chat_history
                        .last()
                        .is_some_and(|m| m.role == "tool" || m.tool_calls.is_some())
                    {
                        self.chat_history.pop();
                    }
                    sfx::play(SfxEvent::Error, self.config.sfx_enabled);
                    self.chat_ui.push(UiChatEntry::assistant(format!("⚠️ {err}")));
                }
                BgEvent::Transcribed(text) => {
                    self.status_banner = None;
                    if text.is_empty() {
                        self.mood = AssistantMood::Idle;
                    } else {
                        self.send_user_message(text, ctx);
                    }
                }
                BgEvent::VoiceError(err) => {
                    self.mood = AssistantMood::Idle;
                    self.status_banner = Some(format!("⚠️ {err}"));
                }
                BgEvent::MarkdownConverted {
                    source,
                    markdown,
                    saved_path,
                } => {
                    self.md_is_converting = false;
                    self.md_source_path = Some(source);
                    self.md_saved_path = saved_path.clone();
                    self.md_output = markdown;
                    self.md_error = None;
                    self.mood = AssistantMood::Happy;
                    sfx::play(SfxEvent::OrganizeDone, self.config.sfx_enabled);
                    if let Some(p) = saved_path {
                        self.status_banner = Some(format!("Markdown guardado en {}", p.display()));
                    } else {
                        self.status_banner = Some("Archivo convertido a Markdown.".to_string());
                    }
                }
                BgEvent::MarkdownError(err) => {
                    self.md_is_converting = false;
                    self.md_error = Some(err.clone());
                    self.mood = AssistantMood::Idle;
                    sfx::play(SfxEvent::Error, self.config.sfx_enabled);
                }
                BgEvent::FilesPicked { for_chat, paths } => {
                    if paths.is_empty() {
                        continue;
                    }
                    if for_chat {
                        self.notify_chat_files(&paths, ctx);
                    } else if let Some(first) = paths.into_iter().next() {
                        self.start_markdown_conversion(first, ctx);
                    }
                }
                BgEvent::FilePickError(err) => {
                    self.status_banner = Some(format!("⚠️ {err}"));
                }
            }
        }
    }

    fn tick_pomodoro(&mut self, ctx: &egui::Context) {
        if self.data.pomodoro.running {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
            if self.last_pomodoro_tick.elapsed().as_secs() >= 1 {
                self.last_pomodoro_tick = Instant::now();
                if let Some(next_phase) = self.data.pomodoro.tick_one_second() {
                    let _ = self.data.save();
                    let msg = match next_phase {
                        PomodoroPhase::Break => {
                            self.mood = AssistantMood::Happy;
                            sfx::play(SfxEvent::PomodoroDone, self.config.sfx_enabled);
                            "¡Sesión Pomodoro completada! Hora de descansar."
                        }
                        PomodoroPhase::Idle => {
                            self.mood = AssistantMood::Idle;
                            sfx::play(SfxEvent::Click, self.config.sfx_enabled);
                            "Descanso terminado. ¿Arrancamos otro bloque de enfoque?"
                        }
                        PomodoroPhase::Work => {
                            sfx::play(SfxEvent::PomodoroStart, self.config.sfx_enabled);
                            "Enfoque iniciado."
                        }
                    };
                    self.chat_ui.push(UiChatEntry::assistant(msg));
                    if self.config.tts_enabled {
                        speak_local_async(msg.to_string());
                    }
                }
            }
        }
    }

    fn start_markdown_conversion(&mut self, path: PathBuf, ctx: &egui::Context) {
        self.md_is_converting = true;
        self.md_error = None;
        self.md_source_path = Some(path.clone());
        self.md_saved_path = None;
        self.mood = AssistantMood::Thinking;

        let auto_save = self.md_auto_save;
        let tx = self.bg_tx.clone();
        let ctx_clone = ctx.clone();

        std::thread::spawn(move || {
            match converter::convert_file_to_markdown(&path) {
                Ok(markdown) => {
                    let saved_path = if auto_save {
                        converter::save_markdown_beside(&path, &markdown).ok()
                    } else {
                        None
                    };
                    let _ = tx.send(BgEvent::MarkdownConverted {
                        source: path,
                        markdown,
                        saved_path,
                    });
                }
                Err(err) => {
                    let _ = tx.send(BgEvent::MarkdownError(err));
                }
            }
            ctx_clone.request_repaint();
        });
    }

    fn spawn_file_picker(&self, for_chat: bool, ctx: &egui::Context) {
        let tx = self.bg_tx.clone();
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            match converter::pick_files_dialog() {
                Ok(paths) => {
                    let _ = tx.send(BgEvent::FilesPicked { for_chat, paths });
                }
                Err(err) => {
                    let _ = tx.send(BgEvent::FilePickError(err));
                }
            }
            ctx_clone.request_repaint();
        });
    }

    fn notify_chat_files(&mut self, paths: &[PathBuf], _ctx: &egui::Context) {
        if paths.is_empty() {
            return;
        }
        let (user_msg, assistant_offer) = if paths.len() == 1 {
            let path = &paths[0];
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("archivo");
            (
                format!("Adjunto archivo: {}", path.display()),
                format!(
                    "Recibí **{}** (`{}`). ¿Querés que lo convierta a Markdown con MarkItDown (normal o compactado para ahorrar tokens)?",
                    name,
                    path.display()
                ),
            )
        } else {
            let list = paths
                .iter()
                .map(|p| format!("- {}", p.display()))
                .collect::<Vec<_>>()
                .join("\n");
            (
                format!("Adjunto archivos:\n{list}"),
                "Recibí esos archivos. ¿Querés que convierta alguno a Markdown con MarkItDown (normal o compactado)?"
                    .to_string(),
            )
        };

        let user_msg_obj = ChatMessage::user(&user_msg);
        let offer_msg_obj = ChatMessage::assistant_text(&assistant_offer);
        self.chat_ui.push(UiChatEntry::user(&user_msg));
        self.chat_history.push(user_msg_obj.clone());
        self.chat_ui.push(UiChatEntry::assistant(&assistant_offer));
        self.chat_history.push(offer_msg_obj.clone());
        let r1 = self.data.record_chat_message(
            &self.current_chat_id,
            Some(&user_msg),
            &user_msg_obj,
            MessageKind::Text,
        );
        self.report_persistence_result(r1);
        let r2 = self.data.record_chat_message(
            &self.current_chat_id,
            None,
            &offer_msg_obj,
            MessageKind::Text,
        );
        self.report_persistence_result(r2);
        sfx::play(SfxEvent::MessageReceived, self.config.sfx_enabled);
    }

    fn start_new_chat(&mut self) {
        self.current_chat_id = Uuid::new_v4().to_string();
        self.chat_ui = vec![UiChatEntry::assistant(&self.welcome_text)];
        self.chat_history = vec![ChatMessage::system(&self.system_prompt)];
        self.show_chat_history = false;
        self.pending_confirmation = None;
        self.is_thinking = false;
        self.mood = AssistantMood::Idle;
    }

    fn load_chat_session(&mut self, session: ConversationSummary) {
        match self
            .data
            .load_conversation_history(&session.id, &self.system_prompt, 24)
        {
            Ok((ui_entries, tech_history)) => {
                self.current_chat_id = session.id;
                self.chat_ui = if ui_entries.is_empty() {
                    vec![UiChatEntry::assistant(&self.welcome_text)]
                } else {
                    ui_entries
                };
                self.chat_history = tech_history;
                self.show_chat_history = false;
                self.pending_confirmation = None;
                self.is_thinking = false;
                self.mood = AssistantMood::Idle;
            }
            Err(e) => {
                self.status_banner = Some(format!("⚠️ Error cargando conversación: {e}"));
            }
        }
    }

    fn approve_pending_confirmation(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.pending_confirmation.take() else {
            return;
        };
        let out = ToolEngine::execute(
            ToolRequest {
                tool_name: pending.tool_name,
                args: pending.args,
                source: ExecutionSource::Ai,
                conversation_id: pending.conversation_id,
                tool_call_id: None, // New execution after explicit confirmation
                approved_confirmation_token: Some(pending.token),
            },
            &mut self.data,
            &mut self.organizer,
            &self.config,
        );
        if let Some(m) = out.mood {
            self.mood = m;
        }
        let msg_text = if out.status == ToolStatus::Error {
            format!("⚠️ {}", out.ui_message)
        } else {
            out.ui_message
        };
        let reply = ChatMessage::assistant_text(&msg_text);
        self.chat_ui.push(UiChatEntry::assistant(&msg_text));
        self.chat_history.push(reply.clone());
        let r = self.data.record_chat_message(
            &self.current_chat_id,
            None,
            &reply,
            MessageKind::Text,
        );
        self.report_persistence_result(r);
        let _ = ctx;
    }

    fn cancel_pending_confirmation(&mut self) {
        let Some(pending) = self.pending_confirmation.take() else {
            return;
        };
        let msg = format!("Operación cancelada por el usuario: {}.", pending.summary);
        let reply = ChatMessage::assistant_text(&msg);
        self.chat_ui.push(UiChatEntry::assistant(&msg));
        self.chat_history.push(reply.clone());
        let r = self.data.record_chat_message(
            &self.current_chat_id,
            None,
            &reply,
            MessageKind::Text,
        );
        self.report_persistence_result(r);
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if dropped.is_empty() {
            return;
        }
        let paths: Vec<PathBuf> = dropped.into_iter().filter_map(|f| f.path).collect();
        if paths.is_empty() {
            return;
        }

        if !self.compact_mode && self.active_tab == ActiveTab::Chat {
            self.notify_chat_files(&paths, ctx);
        } else {
            if !self.compact_mode {
                self.active_tab = ActiveTab::Markdown;
            }
            if let Some(first) = paths.into_iter().next() {
                self.start_markdown_conversion(first, ctx);
            }
        }
    }

    fn toggle_compact_mode(&mut self, ctx: &egui::Context) {
        self.compact_mode = !self.compact_mode;
        let size = if self.compact_mode {
            Vec2::new(360.0, 66.0)
        } else {
            Vec2::new(450.0, 650.0)
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
    }

    fn handle_drag(&self, ui: &egui::Ui, ctx: &egui::Context, rect: egui::Rect) {
        let resp = ui.interact(rect, ui.id().with("island_drag"), egui::Sense::drag());
        if resp.drag_started() {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
    }
}

impl eframe::App for RustyDeskApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_bg_events(ctx);
        self.handle_dropped_files(ctx);
        self.tick_pomodoro(ctx);

        ctx.request_repaint_after(std::time::Duration::from_millis(33));
        let time = ctx.input(|i| i.time);
        let pointer_pos = self.resolve_look_target(ctx);
        let accent = self.mood_accent();
        let border_color = accent;
        let glass_bg = Color32::from_rgba_unmultiplied(11, 15, 25, 248);

        let outer_frame = egui::Frame::none().fill(Color32::TRANSPARENT);

        egui::CentralPanel::default().frame(outer_frame).show(ctx, |ui| {
            let rounding = if self.compact_mode { 4.0_f32 } else { 4.0_f32 };
            let island_frame = egui::Frame::none()
                .fill(glass_bg)
                .rounding(Rounding::same(rounding))
                .stroke(Stroke::new(2.0_f32, border_color))
                .inner_margin(egui::Margin::symmetric(14.0, 10.0));

            island_frame.show(ui, |ui| {
                ui.set_min_size(ui.available_size());

                if self.compact_mode {
                    self.render_compact_island(ui, ctx, time, pointer_pos, accent);
                    return;
                }

                // Expanded Dynamic Island Header (drag on left avatar/title area only)
                ui.horizontal(|ui| {
                    let drag_area = ui.horizontal(|ui| {
                        draw_avatar(ui, 54.0, self.mood, time, pointer_pos);
                        ui.add_space(4.0);
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(&self.config.assistant_name)
                                        .strong()
                                        .size(16.0)
                                        .color(Color32::WHITE),
                                );
                                let badge = if self.config.should_use_mock() {
                                    "Local"
                                } else {
                                    &self.config.model
                                };
                                egui::Frame::none()
                                    .fill(Color32::from_rgba_unmultiplied(
                                        accent.r(),
                                        accent.g(),
                                        accent.b(),
                                        32,
                                    ))
                                    .rounding(2.0)
                                    .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                    .show(ui, |ui| {
                                        ui.label(RichText::new(badge).size(11.0).color(accent));
                                    });
                            });

                            let status_text = if self.data.pomodoro.running {
                                format!("Enfoque: {}", self.data.pomodoro.formatted_time())
                            } else if self.is_thinking {
                                "Pensando...".to_string()
                            } else {
                                let pending = self.data.tasks.iter().filter(|t| !t.done).count();
                                format!("{pending} pendientes • Arrastrar aqui")
                            };
                            ui.label(RichText::new(status_text).size(11.5).color(Color32::from_gray(165)));
                        });
                    });
                    self.handle_drag(ui, ctx, drag_area.response.rect);

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if pixel_button(
                            ui,
                            PixelIcon::Close,
                            "",
                            Color32::from_rgb(153, 27, 27),
                            Color32::WHITE,
                            "Cerrar",
                        )
                        .clicked()
                        {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if pixel_button(
                            ui,
                            PixelIcon::Island,
                            "Isla",
                            Color32::from_rgb(30, 41, 59),
                            Color32::WHITE,
                            "Colapsar a Isla Flotante",
                        )
                        .clicked()
                        {
                            self.toggle_compact_mode(ctx);
                        }
                        let (pin_lbl, pin_bg) = if self.always_on_top {
                            ("Fijo", Color32::from_rgb(30, 64, 175))
                        } else {
                            ("Fijar", Color32::from_rgb(30, 41, 59))
                        };
                        if pixel_button(
                            ui,
                            PixelIcon::Pin,
                            pin_lbl,
                            pin_bg,
                            Color32::WHITE,
                            "Mantener siempre visible",
                        )
                        .clicked()
                        {
                            self.always_on_top = !self.always_on_top;
                            let level = if self.always_on_top {
                                egui::WindowLevel::AlwaysOnTop
                            } else {
                                egui::WindowLevel::Normal
                            };
                            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
                        }
                    });
                });

                ui.add_space(8.0);

                // Segmented Pill Navigation Bar
                egui::Frame::none()
                    .fill(Color32::from_rgb(24, 31, 48))
                    .rounding(12.0)
                    .inner_margin(4.0)
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            nav_pill(ui, &mut self.active_tab, ActiveTab::Chat, PixelIcon::Chat, "Chat", accent);
                            nav_pill(ui, &mut self.active_tab, ActiveTab::Tasks, PixelIcon::Tasks, "Tareas", accent);
                            nav_pill(ui, &mut self.active_tab, ActiveTab::Habits, PixelIcon::Habits, "Habitos", accent);
                            nav_pill(ui, &mut self.active_tab, ActiveTab::Focus, PixelIcon::Focus, "Enfoque", accent);
                            nav_pill(ui, &mut self.active_tab, ActiveTab::Organizer, PixelIcon::Organizer, "Ordenar", accent);
                            nav_pill(ui, &mut self.active_tab, ActiveTab::Markdown, PixelIcon::Markdown, "MarkItDown", accent);
                            nav_pill(ui, &mut self.active_tab, ActiveTab::Compact, PixelIcon::Compact, "Compactar", accent);
                            nav_pill(ui, &mut self.active_tab, ActiveTab::Settings, PixelIcon::Settings, "Config", accent);
                        });
                    });

                ui.add_space(8.0);

                if let Some(banner) = &self.status_banner {
                    egui::Frame::none()
                        .fill(Color32::from_rgba_unmultiplied(250, 204, 21, 25))
                        .rounding(8.0)
                        .inner_margin(6.0)
                        .show(ui, |ui| {
                            ui.colored_label(Color32::from_rgb(250, 204, 21), banner);
                        });
                    ui.add_space(6.0);
                }

                match self.active_tab {
                    ActiveTab::Chat => self.render_chat_tab(ui, ctx, accent),
                    ActiveTab::Tasks => self.render_tasks_tab(ui),
                    ActiveTab::Habits => self.render_habits_tab(ui, accent),
                    ActiveTab::Focus => self.render_focus_tab(ui, accent),
                    ActiveTab::Organizer => self.render_organizer_tab(ui, accent),
                    ActiveTab::Markdown => self.render_markdown_tab(ui, ctx, accent),
                    ActiveTab::Compact => self.render_compact_tab(ui, ctx, accent),
                    ActiveTab::Settings => self.render_settings_tab(ui),
                }
            });
        });
    }
}

impl RustyDeskApp {
    fn render_compact_island(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        time: f64,
        pointer_pos: Option<Pos2>,
        accent: Color32,
    ) {
        ui.horizontal_centered(|ui| {
            let drag_area = ui.horizontal_centered(|ui| {
                draw_avatar(ui, 44.0, self.mood, time, pointer_pos);
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(&self.config.assistant_name)
                            .strong()
                            .size(13.5)
                            .color(Color32::WHITE),
                    );
                    let sub = if self.data.pomodoro.running {
                        format!("Pomodoro {}", self.data.pomodoro.formatted_time())
                    } else if self.is_thinking {
                        "Pensando...".to_string()
                    } else {
                        let pending = self.data.tasks.iter().filter(|t| !t.done).count();
                        format!("{pending} pendientes")
                    };
                    ui.label(RichText::new(sub).size(11.0).color(accent));
                });
            });
            self.handle_drag(ui, ctx, drag_area.response.rect);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if pixel_button(
                    ui,
                    PixelIcon::Close,
                    "",
                    Color32::from_rgb(153, 27, 27),
                    Color32::WHITE,
                    "Cerrar",
                )
                .clicked()
                {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if pixel_button(
                    ui,
                    PixelIcon::Island,
                    "Abrir",
                    Color32::from_rgb(30, 41, 59),
                    Color32::WHITE,
                    "Expandir panel",
                )
                .clicked()
                {
                    self.toggle_compact_mode(ctx);
                }
                let (mic_lbl, mic_bg) = if self.active_recording.is_some() {
                    ("Parar", Color32::from_rgb(190, 18, 60))
                } else {
                    ("Voz", Color32::from_rgb(30, 41, 59))
                };
                if pixel_button(
                    ui,
                    PixelIcon::Mic,
                    mic_lbl,
                    mic_bg,
                    Color32::WHITE,
                    "Hablar por micrófono",
                )
                .clicked()
                {
                    self.toggle_voice_recording(ctx);
                }
            });
        });
    }

    fn render_chat_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, accent: Color32) {
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            egui::Frame::none()
                .fill(Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 30))
                .stroke(Stroke::new(1.5_f32, accent))
                .rounding(8.0)
                .inner_margin(8.0)
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("Soltá el archivo acá para enviárselo a Rusty (MarkItDown)")
                            .color(accent)
                            .strong(),
                    );
                });
            ui.add_space(6.0);
        }

        ui.horizontal_wrapped(|ui| {
            if pixel_button(
                ui,
                PixelIcon::NewChat,
                "Nuevo",
                Color32::from_rgb(30, 64, 175),
                Color32::WHITE,
                "Guardar chat actual y empezar uno nuevo",
            )
            .clicked()
            {
                self.start_new_chat();
            }

            let total_chats = self.data.count_conversations().unwrap_or(0);
            let hist_label = format!("Historial ({total_chats})");
            let hist_fill = if self.show_chat_history {
                accent
            } else {
                Color32::from_rgb(30, 41, 59)
            };
            let hist_text = if self.show_chat_history {
                Color32::from_rgb(15, 20, 32)
            } else {
                Color32::WHITE
            };
            if pixel_button(
                ui,
                PixelIcon::History,
                &hist_label,
                hist_fill,
                hist_text,
                "Ver historial de chats guardados",
            )
            .clicked()
            {
                self.show_chat_history = !self.show_chat_history;
            }

            for (label, cmd) in [
                ("+1 Agua", "/agua"),
                ("Pomodoro", "/pomodoro 25"),
                ("Ordenar", "/ordenar preview"),
            ] {
                let btn = egui::Button::new(RichText::new(label).size(11.5))
                    .fill(Color32::from_rgb(30, 41, 59))
                    .rounding(4.0);
                if ui.add(btn).clicked() {
                    self.send_user_message(cmd.to_string(), ctx);
                }
            }
        });
        ui.add_space(6.0);

        if let Some(pending) = self.pending_confirmation.clone() {
            egui::Frame::none()
                .fill(Color32::from_rgba_unmultiplied(250, 204, 21, 22))
                .stroke(Stroke::new(1.5_f32, Color32::from_rgb(250, 204, 21)))
                .rounding(8.0)
                .inner_margin(8.0)
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("Confirmación requerida: {}", pending.summary))
                            .strong()
                            .size(12.0)
                            .color(Color32::from_rgb(250, 204, 21)),
                    );
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if pixel_button(
                            ui,
                            PixelIcon::Check,
                            "Confirmar",
                            accent,
                            Color32::from_rgb(15, 20, 32),
                            "Autorizar esta operación",
                        )
                        .clicked()
                        {
                            self.approve_pending_confirmation(ctx);
                        }
                        if pixel_button(
                            ui,
                            PixelIcon::Close,
                            "Cancelar",
                            Color32::from_rgb(153, 27, 27),
                            Color32::WHITE,
                            "Rechazar esta operación",
                        )
                        .clicked()
                        {
                            self.cancel_pending_confirmation();
                        }
                    });
                });
            ui.add_space(6.0);
        }

        if self.show_chat_history {
            const PAGE_SIZE: usize = 15;
            let total = self.data.count_conversations().unwrap_or(0);
            let max_page = total.saturating_sub(1) / PAGE_SIZE;
            if self.history_page > max_page {
                self.history_page = max_page;
            }
            let sessions = self
                .data
                .list_conversations_page(PAGE_SIZE, self.history_page * PAGE_SIZE)
                .unwrap_or_default();

            egui::Frame::none()
                .fill(Color32::from_rgb(18, 25, 40))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(51, 65, 85)))
                .rounding(6.0)
                .inner_margin(8.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Chats guardados").strong().size(12.0));
                        if total > PAGE_SIZE {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if self.history_page < max_page && ui.small_button(">").clicked() {
                                    self.history_page += 1;
                                }
                                ui.label(
                                    RichText::new(format!("Pág {}/{}", self.history_page + 1, max_page + 1))
                                        .size(11.0)
                                        .color(Color32::GRAY),
                                );
                                if self.history_page > 0 && ui.small_button("<").clicked() {
                                    self.history_page -= 1;
                                }
                            });
                        }
                    });
                    ui.add_space(4.0);
                    if sessions.is_empty() {
                        ui.label(
                            RichText::new("Todavía no hay chats guardados.")
                                .size(11.5)
                                .color(Color32::GRAY),
                        );
                    } else {
                        egui::ScrollArea::vertical()
                            .id_salt("chat_history_scroll")
                            .max_height(150.0)
                            .show(ui, |ui| {
                                for s in sessions {
                                    let is_current = s.id == self.current_chat_id;
                                    let local_dt = format_local_datetime_from_utc(&s.updated_at);
                                    ui.horizontal(|ui| {
                                        let label = format!("{}  ({})", s.title, local_dt);
                                        let btn = egui::Button::new(
                                            RichText::new(label).size(11.5).color(if is_current {
                                                accent
                                            } else {
                                                Color32::WHITE
                                            }),
                                        )
                                        .fill(Color32::from_rgb(26, 35, 52))
                                        .rounding(4.0);
                                        if ui.add(btn).clicked() {
                                            self.load_chat_session(s.clone());
                                        }
                                        if pixel_button(
                                            ui,
                                            PixelIcon::Clear,
                                            "",
                                            Color32::from_rgb(127, 29, 29),
                                            Color32::WHITE,
                                            "Eliminar chat del historial",
                                        )
                                        .clicked()
                                        {
                                            let r = self.data.delete_conversation(&s.id);
                                            self.report_persistence_result(r);
                                        }
                                    });
                                    ui.add_space(3.0);
                                }
                            });
                    }
                });
            ui.add_space(6.0);
        }

        let available_h = (ui.available_height() - 50.0).max(120.0);
        egui::ScrollArea::vertical()
            .max_height(available_h)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for entry in &self.chat_ui {
                    let (bg, border, align) = if entry.is_user {
                        (
                            Color32::from_rgb(30, 58, 138),
                            Color32::from_rgba_unmultiplied(96, 165, 250, 90),
                            egui::Align::RIGHT,
                        )
                    } else {
                        (
                            Color32::from_rgb(26, 35, 52),
                            Color32::from_rgba_unmultiplied(148, 163, 184, 45),
                            egui::Align::LEFT,
                        )
                    };
                    ui.with_layout(egui::Layout::top_down(align), |ui| {
                        egui::Frame::none()
                            .fill(bg)
                            .stroke(Stroke::new(1.0_f32, border))
                            .rounding(12.0)
                            .inner_margin(9.0)
                            .show(ui, |ui| {
                                ui.set_max_width(330.0);
                                ui.label(&entry.text);
                                ui.label(
                                    RichText::new(&entry.timestamp)
                                        .size(10.0)
                                        .color(Color32::from_gray(150)),
                                );
                            });
                    });
                    ui.add_space(6.0);
                }
            });

        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            egui::Frame::none()
                .fill(Color32::from_rgb(24, 31, 48))
                .rounding(14.0)
                .inner_margin(5.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (mic_text, mic_bg) = if self.active_recording.is_some() {
                            ("Parar", Color32::from_rgb(190, 18, 60))
                        } else {
                            ("Voz", Color32::from_rgb(30, 41, 59))
                        };
                        if pixel_button(ui, PixelIcon::Mic, mic_text, mic_bg, Color32::WHITE, "Grabar voz").clicked() {
                            self.toggle_voice_recording(ctx);
                        }
                        if pixel_button(
                            ui,
                            PixelIcon::Attach,
                            "",
                            Color32::from_rgb(30, 41, 59),
                            Color32::WHITE,
                            "Adjuntar archivo para MarkItDown",
                        )
                        .clicked()
                        {
                            self.spawn_file_picker(true, ctx);
                        }

                        let resp = ui.add_sized(
                            [ui.available_width() - 72.0, 28.0],
                            egui::TextEdit::singleline(&mut self.chat_input)
                                .frame(false)
                                .hint_text("Escribí una orden o pregunta..."),
                        );

                        let enter_pressed =
                            resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

                        let (send_icon, send_label, send_fill, send_fg) = if self.is_thinking {
                            (
                                PixelIcon::Close,
                                "Cancelar",
                                Color32::from_rgb(153, 27, 27),
                                Color32::WHITE,
                            )
                        } else {
                            (
                                PixelIcon::Send,
                                "Enviar",
                                accent,
                                Color32::from_rgb(15, 20, 32),
                            )
                        };

                        if pixel_button(ui, send_icon, send_label, send_fill, send_fg, "").clicked()
                        {
                            if self.is_thinking {
                                self.is_thinking = false;
                                self.mood = AssistantMood::Idle;
                                self.chat_ui.push(UiChatEntry::assistant("Solicitud cancelada."));
                            } else if !self.chat_input.trim().is_empty() {
                                let msg = std::mem::take(&mut self.chat_input);
                                self.send_user_message(msg, ctx);
                            }
                        } else if enter_pressed && !self.is_thinking && !self.chat_input.trim().is_empty() {
                            let msg = std::mem::take(&mut self.chat_input);
                            self.send_user_message(msg, ctx);
                        }
                    });
                });
        });
    }

    fn render_tasks_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let resp = ui.add_sized(
                [ui.available_width() - 82.0, 28.0],
                egui::TextEdit::singleline(&mut self.new_task_input).hint_text("Nueva tarea..."),
            );
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (pixel_button(
                ui,
                PixelIcon::NewChat,
                "Agregar",
                Color32::from_rgb(30, 64, 175),
                Color32::WHITE,
                "Agregar nueva tarea",
            )
            .clicked()
                || enter)
                && !self.new_task_input.trim().is_empty() {
                let title = std::mem::take(&mut self.new_task_input);
                let out = ToolEngine::execute(
                    ToolRequest {
                        tool_name: "add_task".to_string(),
                        args: serde_json::json!({ "title": title.trim() }),
                        source: ExecutionSource::Ui,
                        conversation_id: None,
                        tool_call_id: None,
                        approved_confirmation_token: None,
                    },
                    &mut self.data,
                    &mut self.organizer,
                    &self.config,
                );
                if out.status == ToolStatus::Error {
                    self.status_banner = Some(format!("⚠️ {}", out.ui_message));
                } else {
                    sfx::play(SfxEvent::TaskAdded, self.config.sfx_enabled);
                }
            }
        });
        ui.add_space(6.0);

        let mut remove_idx = None;
        let mut toggled_task: Option<TaskItem> = None;

        egui::ScrollArea::vertical().show(ui, |ui| {
            if self.data.tasks.is_empty() {
                ui.label(RichText::new("No hay tareas todavía.").color(Color32::GRAY));
            }
            for (idx, task) in self.data.tasks.iter_mut().enumerate() {
                egui::Frame::none()
                    .fill(Color32::from_rgb(24, 31, 48))
                    .rounding(10.0)
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if ui.checkbox(&mut task.done, "").changed() {
                                if task.done {
                                    sfx::play(SfxEvent::TaskCompleted, self.config.sfx_enabled);
                                } else {
                                    sfx::play(SfxEvent::Click, self.config.sfx_enabled);
                                }
                                toggled_task = Some(task.clone());
                            }
                            let mut text = RichText::new(&task.title);
                            if task.done {
                                text = text.strikethrough().color(Color32::GRAY);
                            }
                            ui.label(text);
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if pixel_button(
                                    ui,
                                    PixelIcon::Clear,
                                    "Borrar",
                                    Color32::from_rgb(127, 29, 29),
                                    Color32::WHITE,
                                    "Eliminar tarea",
                                )
                                .clicked()
                                {
                                    remove_idx = Some(idx);
                                }
                            });
                        });
                    });
                ui.add_space(4.0);
            }
        });

        if let Some(t) = toggled_task {
            let r = self.data.upsert_task(&t);
            self.report_persistence_result(r);
        }
        if let Some(i) = remove_idx {
            let r = self.data.delete_task_at(i);
            self.report_persistence_result(r);
        }
    }

    fn render_habits_tab(&mut self, ui: &mut egui::Ui, accent: Color32) {
        let mut delta_action: Option<(usize, i32)> = None;
        for (idx, h) in self.data.habits.iter_mut().enumerate() {
            h.ensure_today();
            egui::Frame::none()
                .fill(Color32::from_rgb(24, 31, 48))
                .rounding(12.0)
                .inner_margin(10.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{} {}", h.icon, h.name)).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("+1").clicked() {
                                delta_action = Some((idx, 1));
                            }
                            if ui.small_button("-1").clicked() && h.current_count > 0 {
                                delta_action = Some((idx, -1));
                            }
                            ui.label(RichText::new(format!("{}/{}", h.current_count, h.daily_goal)).color(accent));
                        });
                    });
                    ui.add_space(4.0);
                    let progress = (h.current_count as f32 / h.daily_goal as f32).clamp(0.0, 1.0);
                    ui.add(egui::ProgressBar::new(progress).fill(accent).show_percentage());
                });
            ui.add_space(6.0);
        }

        if let Some((idx, delta)) = delta_action {
            if delta > 0 {
                let name = self.data.habits[idx].name.clone();
                let out = ToolEngine::execute(
                    ToolRequest {
                        tool_name: "log_habit".to_string(),
                        args: serde_json::json!({ "name": name, "delta": delta }),
                        source: ExecutionSource::Ui,
                        conversation_id: None,
                        tool_call_id: None,
                        approved_confirmation_token: None,
                    },
                    &mut self.data,
                    &mut self.organizer,
                    &self.config,
                );
                if out.status == ToolStatus::Error {
                    self.status_banner = Some(format!("⚠️ {}", out.ui_message));
                } else {
                    sfx::play(SfxEvent::HabitLogged, self.config.sfx_enabled);
                }
            } else {
                let r = self.data.log_habit_delta(idx, delta);
                self.report_persistence_result(r);
            }
        }

        ui.separator();
        ui.label(RichText::new("Nuevo hábito").strong());
        ui.horizontal(|ui| {
            ui.add_sized([36.0, 24.0], egui::TextEdit::singleline(&mut self.new_habit_icon));
            ui.add_sized(
                [180.0, 24.0],
                egui::TextEdit::singleline(&mut self.new_habit_name).hint_text("Ej. Leer páginas"),
            );
            ui.add(egui::DragValue::new(&mut self.new_habit_goal).range(1..=100));
            if pixel_button(
                ui,
                PixelIcon::NewChat,
                "Crear",
                Color32::from_rgb(30, 64, 175),
                Color32::WHITE,
                "Crear nuevo hábito",
            )
            .clicked()
                && !self.new_habit_name.trim().is_empty() {
                let item = HabitItem::new(
                    self.new_habit_name.trim(),
                    self.new_habit_icon.trim(),
                    self.new_habit_goal,
                );
                let r = self.data.upsert_habit(&item);
                if r.is_ok() {
                    self.data.habits.push(item);
                    self.new_habit_name.clear();
                }
                self.report_persistence_result(r);
            }
        });
    }

    fn render_focus_tab(&mut self, ui: &mut egui::Ui, accent: Color32) {
        ui.vertical_centered(|ui| {
            ui.add_space(10.0);
            let phase_lbl = match self.data.pomodoro.phase {
                PomodoroPhase::Idle => "Listo para enfocar",
                PomodoroPhase::Work => "Enfoque activo",
                PomodoroPhase::Break => "Descanso",
            };
            ui.label(RichText::new(phase_lbl).size(15.0).color(Color32::LIGHT_GRAY));
            ui.add_space(10.0);

            // Circular progress ring
            let total_secs = match self.data.pomodoro.phase {
                PomodoroPhase::Break => (self.data.pomodoro.break_minutes * 60).max(1),
                _ => (self.data.pomodoro.work_minutes * 60).max(1),
            } as f32;
            let progress = (self.data.pomodoro.remaining_secs as f32 / total_secs).clamp(0.0, 1.0);
            draw_pomodoro_ring(
                ui,
                160.0,
                progress,
                &self.data.pomodoro.formatted_time(),
                accent,
            );

            ui.add_space(14.0);
            ui.horizontal(|ui| {
                let (start_icon, start_lbl) = if self.data.pomodoro.running {
                    (PixelIcon::Pause, "Pausar")
                } else {
                    (PixelIcon::Play, "Iniciar")
                };
                if pixel_button(
                    ui,
                    start_icon,
                    start_lbl,
                    accent,
                    Color32::from_rgb(15, 20, 32),
                    "Iniciar o pausar Pomodoro",
                )
                .clicked()
                {
                    let out = ToolEngine::execute(
                        ToolRequest {
                            tool_name: "pause_pomodoro".to_string(),
                            args: serde_json::json!({}),
                            source: ExecutionSource::Ui,
                            conversation_id: None,
                            tool_call_id: None,
                            approved_confirmation_token: None,
                        },
                        &mut self.data,
                        &mut self.organizer,
                        &self.config,
                    );
                    if out.status == ToolStatus::Error {
                        self.status_banner = Some(format!("⚠️ {}", out.ui_message));
                    } else if self.data.pomodoro.running {
                        self.mood = AssistantMood::Focused;
                        sfx::play(SfxEvent::PomodoroStart, self.config.sfx_enabled);
                    } else {
                        self.mood = AssistantMood::Idle;
                        sfx::play(SfxEvent::Click, self.config.sfx_enabled);
                    }
                }
                if pixel_button(
                    ui,
                    PixelIcon::Reset,
                    "Reiniciar",
                    Color32::from_rgb(51, 65, 85),
                    Color32::WHITE,
                    "Reiniciar temporizador",
                )
                .clicked()
                {
                    self.data.pomodoro.reset();
                    self.mood = AssistantMood::Idle;
                    let _ = self.data.save();
                }
            });
            ui.add_space(12.0);
            ui.label(format!(
                "Sesiones completadas hoy: {}",
                self.data.pomodoro.completed_sessions
            ));
        });

        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Trabajo (min):");
            if ui
                .add(egui::DragValue::new(&mut self.data.pomodoro.work_minutes).range(1..=120))
                .changed()
                && self.data.pomodoro.phase == PomodoroPhase::Idle
            {
                self.data.pomodoro.remaining_secs = self.data.pomodoro.work_minutes * 60;
                let _ = self.data.save();
            }
            ui.label("Descanso (min):");
            if ui
                .add(egui::DragValue::new(&mut self.data.pomodoro.break_minutes).range(1..=60))
                .changed()
            {
                let _ = self.data.save();
            }
        });
    }

    fn render_organizer_tab(&mut self, ui: &mut egui::Ui, accent: Color32) {
        ui.label("Carpeta a organizar:");
        ui.text_edit_singleline(&mut self.config.organize_target_dir);
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            if pixel_button(
                ui,
                PixelIcon::Eye,
                "Previsualizar",
                Color32::from_rgb(30, 41, 59),
                Color32::WHITE,
                "Ver qué archivos se moverían",
            )
            .clicked()
            {
                let out = ToolEngine::execute(
                    ToolRequest {
                        tool_name: "preview_organize_desktop".to_string(),
                        args: serde_json::json!({}),
                        source: ExecutionSource::Ui,
                        conversation_id: None,
                        tool_call_id: None,
                        approved_confirmation_token: None,
                    },
                    &mut self.data,
                    &mut self.organizer,
                    &self.config,
                );
                self.status_banner = Some(if out.status == ToolStatus::Error {
                    format!("⚠️ {}", out.ui_message)
                } else {
                    format!("Vista previa: {} archivo(s) para organizar.", self.organizer.last_preview.len())
                });
            }

            if pixel_button(
                ui,
                PixelIcon::Check,
                "Aplicar",
                accent,
                Color32::from_rgb(15, 20, 32),
                "Mover archivos a sus carpetas",
            )
            .clicked()
            {
                let out = ToolEngine::execute(
                    ToolRequest {
                        tool_name: "apply_organize_desktop".to_string(),
                        args: serde_json::json!({}),
                        source: ExecutionSource::Ui,
                        conversation_id: None,
                        tool_call_id: None,
                        approved_confirmation_token: None,
                    },
                    &mut self.data,
                    &mut self.organizer,
                    &self.config,
                );
                if out.status == ToolStatus::Error {
                    sfx::play(SfxEvent::Error, self.config.sfx_enabled);
                    self.status_banner = Some(format!("⚠️ {}", out.ui_message));
                } else {
                    self.mood = AssistantMood::Happy;
                    sfx::play(SfxEvent::OrganizeDone, self.config.sfx_enabled);
                    self.status_banner = Some(out.ui_message);
                }
            }

            if !self.organizer.undo_stack.is_empty()
                && pixel_button(
                    ui,
                    PixelIcon::Undo,
                    "Deshacer",
                    Color32::from_rgb(51, 65, 85),
                    Color32::WHITE,
                    "Revertir última organización",
                )
                .clicked()
            {
                let out = ToolEngine::execute(
                    ToolRequest {
                        tool_name: "undo_organize_desktop".to_string(),
                        args: serde_json::json!({}),
                        source: ExecutionSource::Ui,
                        conversation_id: None,
                        tool_call_id: None,
                        approved_confirmation_token: None,
                    },
                    &mut self.data,
                    &mut self.organizer,
                    &self.config,
                );
                self.status_banner = Some(if out.status == ToolStatus::Error {
                    format!("⚠️ {}", out.ui_message)
                } else {
                    out.ui_message
                });
            }
        });

        ui.add_space(6.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            if self.organizer.last_preview.is_empty() {
                ui.label(
                    RichText::new("Tocá 'Previsualizar' para ver qué archivos se moverían.")
                        .color(Color32::GRAY),
                );
            } else {
                for plan in &self.organizer.last_preview {
                    egui::Frame::none()
                        .fill(Color32::from_rgb(24, 31, 48))
                        .rounding(8.0)
                        .inner_margin(6.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("[{}]", plan.category)).color(accent));
                                ui.label(&plan.file_name);
                            });
                        });
                    ui.add_space(3.0);
                }
            }
        });
    }

    fn render_markdown_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, accent: Color32) {
        let is_hovering_file = ctx.input(|i| !i.raw.hovered_files.is_empty());
        let drop_bg = if is_hovering_file {
            Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 35)
        } else {
            Color32::from_rgb(18, 25, 40)
        };
        let drop_stroke = if is_hovering_file {
            Stroke::new(2.0_f32, accent)
        } else {
            Stroke::new(1.2_f32, Color32::from_rgba_unmultiplied(148, 163, 184, 70))
        };

        egui::Frame::none()
            .fill(drop_bg)
            .stroke(drop_stroke)
            .rounding(12.0)
            .inner_margin(14.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    let title = if is_hovering_file {
                        "Soltá el archivo acá para convertirlo"
                    } else {
                        "Arrastrá y soltá un archivo acá"
                    };
                    ui.label(RichText::new(title).strong().size(14.0).color(if is_hovering_file {
                        accent
                    } else {
                        Color32::WHITE
                    }));
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("PDF, DOCX, XLSX, PPTX, HTML, CSV, XML, RSS — 100% local sin LLM")
                            .size(11.5)
                            .color(Color32::from_gray(165)),
                    );
                    ui.add_space(10.0);

                    if !self.md_is_converting
                        && pixel_button(
                            ui,
                            PixelIcon::FolderOpen,
                            "Elegir archivo...",
                            accent,
                            Color32::from_rgb(15, 20, 32),
                            "Abrir selector de archivos",
                        )
                        .clicked()
                    {
                        self.spawn_file_picker(false, ctx);
                    }
                });
            });

        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(
                &mut self.md_auto_save,
                "Guardar .md automáticamente junto al archivo",
            );
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Salida:");
            for mode in [
                CompactionMode::None,
                CompactionMode::Moderate,
                CompactionMode::LineByLine,
                CompactionMode::Extreme,
            ] {
                ui.selectable_value(&mut self.md_compact_mode, mode, mode.label());
            }
        });

        if self.md_is_converting {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Convirtiendo a Markdown localmente...");
            });
        }

        if let Some(err) = &self.md_error {
            ui.add_space(6.0);
            ui.colored_label(Color32::from_rgb(248, 113, 113), format!("⚠️ {err}"));
        }

        if !self.md_output.is_empty() {
            let preview_text = converter::compact_for_llm(&self.md_output, self.md_compact_mode);
            let compact_mode_for_copy = if self.md_compact_mode == CompactionMode::None {
                CompactionMode::Moderate
            } else {
                self.md_compact_mode
            };
            let compacted_for_copy =
                converter::compact_for_llm(&self.md_output, compact_mode_for_copy);
            let (orig_len, comp_len, saved_pct) =
                converter::compaction_stats(&self.md_output, &compacted_for_copy);

            ui.add_space(6.0);
            if let Some(src) = &self.md_source_path {
                ui.label(
                    RichText::new(format!("Origen: {}", src.display()))
                        .size(11.0)
                        .color(Color32::from_gray(170)),
                );
            }
            if let Some(saved) = &self.md_saved_path {
                ui.label(
                    RichText::new(format!("Guardado: {}", saved.display()))
                        .size(11.0)
                        .color(accent),
                );
            }
            ui.label(
                RichText::new(format!(
                    "Original: {orig_len} chars • Compactado ({}): {comp_len} chars (-{saved_pct:.1}%)",
                    compact_mode_for_copy.label()
                ))
                .size(11.0)
                .color(Color32::from_gray(170)),
            );

            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if pixel_button(
                    ui,
                    PixelIcon::Copy,
                    "Copiar normal",
                    Color32::from_rgb(30, 41, 59),
                    Color32::WHITE,
                    "Copiar Markdown sin compactar",
                )
                .clicked()
                {
                    ctx.copy_text(self.md_output.clone());
                    self.status_banner = Some("Markdown normal copiado al portapapeles.".to_string());
                }
                if pixel_button(
                    ui,
                    PixelIcon::Compact,
                    "Copiar compactado",
                    accent,
                    Color32::from_rgb(15, 20, 32),
                    "Copiar versión compactada para Gemini/LLM",
                )
                .clicked()
                {
                    ctx.copy_text(compacted_for_copy.clone());
                    self.status_banner = Some(format!(
                        "Markdown compactado ({}) copiado (-{saved_pct:.1}%).",
                        compact_mode_for_copy.label()
                    ));
                }
                if pixel_button(
                    ui,
                    PixelIcon::Save,
                    "Guardar .md",
                    Color32::from_rgb(30, 64, 175),
                    Color32::WHITE,
                    "Guardar archivo .md junto al original",
                )
                .clicked()
                {
                    if let Some(src) = &self.md_source_path {
                        match converter::save_markdown_beside(src, &preview_text) {
                            Ok(p) => {
                                self.md_saved_path = Some(p.clone());
                                self.status_banner = Some(format!("Guardado en {}", p.display()));
                            }
                            Err(e) => self.md_error = Some(e),
                        }
                    }
                }
                if pixel_button(
                    ui,
                    PixelIcon::Clear,
                    "Limpiar",
                    Color32::from_rgb(51, 65, 85),
                    Color32::WHITE,
                    "Limpiar vista previa",
                )
                .clicked()
                {
                    self.md_output.clear();
                    self.md_source_path = None;
                    self.md_saved_path = None;
                    self.md_error = None;
                }
            });

            ui.add_space(6.0);
            egui::ScrollArea::vertical()
                .id_salt("md_preview_scroll")
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut preview_text.as_str())
                            .desired_width(f32::INFINITY)
                            .font(egui::TextStyle::Monospace),
                    );
                });
        }
    }

    fn render_compact_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, accent: Color32) {
        ui.label(
            RichText::new("Compactador de texto / Markdown para LLM")
                .strong()
                .size(13.5),
        );
        ui.label(
            RichText::new("Pegá texto o Markdown para reducir espacios y saltos innecesarios sin perder estructura.")
                .size(11.5)
                .color(Color32::from_gray(165)),
        );
        ui.add_space(6.0);

        ui.horizontal_wrapped(|ui| {
            ui.label("Modo:");
            for mode in [
                CompactionMode::Moderate,
                CompactionMode::LineByLine,
                CompactionMode::Extreme,
            ] {
                ui.selectable_value(&mut self.compact_mode_tab, mode, mode.label());
            }
        });

        ui.add_space(6.0);
        ui.label(RichText::new("Texto original:").size(11.5).color(Color32::from_gray(180)));
        egui::ScrollArea::vertical()
            .id_salt("compact_input_scroll")
            .max_height(150.0)
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.compact_input)
                        .desired_width(f32::INFINITY)
                        .desired_rows(6)
                        .hint_text("Pegá acá tu texto o Markdown (Ctrl+V)..."),
                );
            });

        let compacted = converter::compact_for_llm(&self.compact_input, self.compact_mode_tab);
        let (orig_len, comp_len, saved_pct) =
            converter::compaction_stats(&self.compact_input, &compacted);

        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if !compacted.is_empty() {
                if pixel_button(
                    ui,
                    PixelIcon::Copy,
                    "Copiar compactado",
                    accent,
                    Color32::from_rgb(15, 20, 32),
                    "Copiar texto compactado al portapapeles",
                )
                .clicked()
                {
                    ctx.copy_text(compacted.clone());
                    self.status_banner = Some(format!(
                        "Texto compactado copiado (-{saved_pct:.1}% caracteres)."
                    ));
                }
            }
            if !self.compact_input.is_empty()
                && pixel_button(
                    ui,
                    PixelIcon::Clear,
                    "Limpiar",
                    Color32::from_rgb(51, 65, 85),
                    Color32::WHITE,
                    "Vaciar texto",
                )
                .clicked()
            {
                self.compact_input.clear();
            }
        });

        ui.add_space(4.0);
        ui.label(
            RichText::new(format!(
                "Original: {orig_len} chars • Compactado: {comp_len} chars • Ahorro: {saved_pct:.1}%"
            ))
            .size(11.5)
            .color(accent),
        );

        ui.add_space(6.0);
        ui.label(RichText::new("Resultado compactado:").size(11.5).color(Color32::from_gray(180)));
        egui::ScrollArea::vertical()
            .id_salt("compact_output_scroll")
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut compacted.as_str())
                        .desired_width(f32::INFINITY)
                        .font(egui::TextStyle::Monospace),
                );
            });
    }

    fn render_settings_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Presets rápidos de API").strong());
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("Google Gemini").clicked() {
                self.config.base_url =
                    "https://generativelanguage.googleapis.com/v1beta".to_string();
                self.config.model = "gemini-flash-latest".to_string();
                self.config.force_mock = false;
            }
            if ui.small_button("OpenRouter").clicked() {
                self.config.base_url = "https://openrouter.ai/api/v1".to_string();
                self.config.model = "openai/gpt-4o-mini".to_string();
                self.config.force_mock = false;
            }
            if ui.small_button("OpenAI").clicked() {
                self.config.base_url = "https://api.openai.com/v1".to_string();
                self.config.model = "gpt-4o-mini".to_string();
                self.config.stt_model = "whisper-1".to_string();
                self.config.force_mock = false;
            }
            if ui.small_button("Groq").clicked() {
                self.config.base_url = "https://api.groq.com/openai/v1".to_string();
                self.config.model = "llama-3.3-70b-versatile".to_string();
                self.config.stt_model = "whisper-large-v3".to_string();
                self.config.force_mock = false;
            }
            if ui.small_button("Ollama (Local)").clicked() {
                self.config.base_url = "http://localhost:11434/v1".to_string();
                self.config.model = "llama3.2".to_string();
                self.config.api_key.clear();
                self.config.force_mock = false;
            }
            if ui.small_button("Mock Local").clicked() {
                self.config.force_mock = true;
            }
        });

        ui.separator();
        egui::Grid::new("api_cfg_grid")
            .num_columns(2)
            .spacing([10.0, 8.0])
            .show(ui, |ui| {
                ui.label("Nombre:");
                ui.text_edit_singleline(&mut self.config.assistant_name);
                ui.end_row();

                ui.label("Base URL:");
                ui.text_edit_singleline(&mut self.config.base_url);
                ui.end_row();

                ui.label("API Key:");
                ui.add(egui::TextEdit::singleline(&mut self.config.api_key).password(true));
                ui.end_row();

                ui.label("Modelo Chat:");
                ui.text_edit_singleline(&mut self.config.model);
                ui.end_row();

                ui.label("Modelo Voz (STT):");
                ui.text_edit_singleline(&mut self.config.stt_model);
                ui.end_row();
            });

        ui.add_space(6.0);
        ui.checkbox(&mut self.config.force_mock, "Forzar Modo Mock Local (sin red)");
        ui.checkbox(
            &mut self.config.sfx_enabled,
            "Efectos de sonido tacticos (8-bit / C&C)",
        );
        ui.checkbox(
            &mut self.config.tts_enabled,
            "Leer respuestas en voz alta (requiere espeak-ng / spd-say)",
        );
        ui.label(
            RichText::new(format!(
                "Sonidos personalizados (.wav): {}",
                sfx::custom_sounds_dir().display()
            ))
            .size(10.5)
            .color(Color32::GRAY),
        );

        ui.add_space(10.0);
        if pixel_button(
            ui,
            PixelIcon::Save,
            "Guardar configuracion",
            Color32::from_rgb(30, 64, 175),
            Color32::WHITE,
            "Guardar cambios en config.json",
        )
        .clicked()
        {
            match self.config.save() {
                Ok(_) => self.status_banner = Some("Configuración guardada.".to_string()),
                Err(e) => self.status_banner = Some(format!("Error al guardar: {e}")),
            }
        }
    }
}

fn nav_pill(
    ui: &mut egui::Ui,
    active: &mut ActiveTab,
    tab: ActiveTab,
    icon: PixelIcon,
    label: &str,
    accent: Color32,
) {
    let selected = *active == tab;
    let fill = if selected {
        accent
    } else {
        Color32::from_rgb(30, 41, 59)
    };
    let text_color = if selected {
        Color32::from_rgb(11, 15, 25)
    } else {
        Color32::from_gray(225)
    };

    let icon_size = 11.0;
    let font_id = egui::FontId::proportional(11.5);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font_id, text_color);
    let pad_x = 8.0;
    let gap = 5.0;
    let width = pad_x * 2.0 + icon_size + gap + galley.size().x;
    let height = 28.0;

    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let stroke_color = if selected {
            accent
        } else {
            Color32::from_rgb(51, 65, 85)
        };
        ui.painter().rect_filled(rect, 3.0, fill);
        ui.painter()
            .rect_stroke(rect, 3.0, Stroke::new(1.5_f32, stroke_color));

        let icon_rect = egui::Rect::from_min_size(
            Pos2::new(rect.min.x + pad_x, rect.center().y - icon_size * 0.5),
            Vec2::splat(icon_size),
        );
        paint_pixel_icon(ui.painter(), icon_rect, icon, text_color);

        let text_pos = Pos2::new(
            icon_rect.max.x + gap,
            rect.center().y - galley.size().y * 0.5,
        );
        ui.painter().galley(text_pos, galley, text_color);
    }

    if resp.clicked() {
        *active = tab;
    }
}

fn draw_pomodoro_ring(ui: &mut egui::Ui, size: f32, progress: f32, time_text: &str, accent: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let center = rect.center();
    let radius = size * 0.42;

    let steps = 32;
    let active_steps = ((steps as f32) * progress).round() as usize;
    let block = 7.0_f32;
    for i in 0..steps {
        let frac = i as f32 / steps as f32;
        let angle = -std::f32::consts::FRAC_PI_2 + frac * std::f32::consts::TAU;
        let p = Pos2::new(center.x + angle.cos() * radius, center.y + angle.sin() * radius);
        let c = if i < active_steps {
            accent
        } else {
            Color32::from_rgb(30, 41, 59)
        };
        painter.rect_filled(
            egui::Rect::from_center_size(p, Vec2::splat(block)),
            Rounding::ZERO,
            c,
        );
    }

    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        time_text,
        egui::FontId::monospace(28.0),
        Color32::WHITE,
    );
}

fn configure_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.window_rounding = 2.0.into();
    visuals.widgets.inactive.rounding = 2.0.into();
    visuals.widgets.hovered.rounding = 2.0.into();
    visuals.widgets.active.rounding = 2.0.into();
    visuals.panel_fill = Color32::TRANSPARENT;
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.button_padding = Vec2::new(10.0, 6.0);
    style.spacing.item_spacing = Vec2::new(6.0, 6.0);
    for font_id in style.text_styles.values_mut() {
        font_id.family = egui::FontFamily::Monospace;
    }
    ctx.set_style(style);
}

impl RustyDeskApp {
    fn resolve_look_target(&self, ctx: &egui::Context) -> Option<Pos2> {
        if let Some(inside) = ctx.input(|i| i.pointer.hover_pos()) {
            return Some(inside);
        }
        if let Ok(guard) = self.global_cursor.lock() {
            if let Some((gx, gy)) = *guard {
                if let Some(outer) = ctx.input(|i| i.viewport().outer_rect) {
                    return Some(Pos2::new(gx as f32 - outer.min.x, gy as f32 - outer.min.y));
                }
                return Some(Pos2::new(gx as f32 * 0.25, gy as f32 * 0.25));
            }
        }
        ctx.input(|i| i.pointer.latest_pos())
    }
}

fn spawn_global_cursor_tracker() -> Arc<Mutex<Option<(i16, i16)>>> {
    let pos = Arc::new(Mutex::new(None));
    let pos_clone = Arc::clone(&pos);
    std::thread::spawn(move || {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::ConnectionExt;

        let Ok((conn, screen_num)) = x11rb::connect(None) else {
            return;
        };
        let root = conn.setup().roots[screen_num].root;
        loop {
            if let Ok(cookie) = conn.query_pointer(root) {
                if let Ok(reply) = cookie.reply() {
                    if let Ok(mut guard) = pos_clone.lock() {
                        *guard = Some((reply.root_x, reply.root_y));
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(33));
        }
    });
    pos
}
