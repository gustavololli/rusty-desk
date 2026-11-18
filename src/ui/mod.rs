pub mod avatar;
pub mod icons8bit;
pub mod island;
pub mod pet;
pub mod retro;
pub mod sprites;

use crate::ai::{
    AiProvider, ChatMessage, GoogleGeminiProvider, MockProvider, OpenAiCompatibleProvider,
};
use crate::config::{
    clamp_pet_position, clamp_window_position, AppConfig, IslandPosition, PetIntensity, PetSize,
    SearchProviderId,
};
use crate::converter::{self, CompactionMode};
use crate::free_tools::{
    format_search_response_for_user_and_llm, FreeToolsEngine, ProviderHealthStatus,
    TurnExternalLimits,
};
use crate::music::{self, MusicPlayer};
use crate::organizer::FileOrganizer;
use crate::persistence::{ConversationSummary, MessageKind, NewMusicTrack, PersistedTrack};
use crate::platform::{
    apply_platform_always_on_top, detect_current_monitor, detect_session_kind,
    query_x11_virtual_bounds, spawn_global_cursor_tracker, MonitorRect, SessionKind,
};
use crate::sfx::{self, SfxEvent};
use crate::state::{
    format_local_datetime_from_utc, AppData, AssistantMood, FocusStartResult, HabitItem,
    PomodoroPhase, PomodoroPrompt, PomodoroTickOutcome, TaskItem, UiChatEntry,
};
use crate::tools::{
    parse_local_command, select_tools_for_message, ExecutionSource, LocalCommand,
    PendingConfirmation, ToolEngine, ToolRequest, ToolStatus,
};
use crate::ui::avatar::draw_rusty;
use crate::ui::icons8bit::{paint_pixel_icon, pixel_button, PixelIcon};
use crate::ui::island::{IslandInput, IslandStateMachine, IslandVisibility};
use crate::ui::pet::{
    AnimationController, AnimationFrame, PetBehavior, PetEvent, PetRenderer, WindowMode,
};
use crate::voice::{speak_local_async, ActiveRecording};
use eframe::egui::{self, Color32, Pos2, RichText, Rounding, Stroke, Vec2};
use retro::{console_frame, paint_pixel_bevel, BUTTON, BUTTON_HOVER, IVORY, NIGHT};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveTab {
    Chat,
    Tasks,
    Habits,
    Focus,
    Music,
    Organizer,
    Markdown,
    Compact,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SettingsSubTab {
    #[default]
    Ai,
    Position,
    Pet,
    ExternalTools,
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
    M3uPicked(Option<PathBuf>),
    M3uPickError(String),
    WeatherResult(Result<String, String>),
    ExternalCommandResult {
        #[allow(dead_code)]
        tool_name: String,
        result: Result<String, String>,
    },
    ExternalAiToolsDone {
        results: Vec<(String, String, bool)>,
        history_snapshot: Vec<ChatMessage>,
        new_mood: Option<AssistantMood>,
        waiting_for_confirmation: bool,
    },
    ProviderTestResult(String),
}

pub struct RustyDeskApp {
    rt: tokio::runtime::Runtime,
    config: AppConfig,
    data: AppData,
    organizer: FileOrganizer,

    window_mode: WindowMode,
    always_on_top: bool,
    active_tab: ActiveTab,
    mood: AssistantMood,
    pet_controller: AnimationController,
    pet_behavior: PetBehavior,
    pet_drag_origin: Option<Pos2>,
    pet_dragging: bool,
    last_frame_time: f64,
    screen_bounds: Option<(f32, f32, f32, f32)>,

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
    editing_task_id: Option<String>,
    editing_task_title: String,
    pending_focus_switch_task_id: Option<String>,
    confirm_clear_completed_tasks: bool,
    confirm_skip_break_continue: bool,
    new_habit_name: String,
    new_habit_icon: String,
    new_habit_goal: u32,
    status_banner: Option<String>,

    active_recording: Option<ActiveRecording>,

    md_is_converting: bool,
    md_auto_save: bool,
    md_compact_mode: CompactionMode,
    md_source_path: Option<PathBuf>,
    md_saved_path: Option<PathBuf>,
    md_output: String,
    md_error: Option<String>,

    compact_input: String,
    compact_mode_tab: CompactionMode,

    music_player: MusicPlayer,
    music_m3u_path_input: String,
    music_url_input: String,
    music_url_title_input: String,
    music_selected_playlist_id: Option<String>,
    music_tracks: Vec<PersistedTrack>,
    music_current_track_idx: Option<usize>,
    music_status: Option<String>,
    island_sm: IslandStateMachine,
    island_dragging: bool,
    current_monitor: MonitorRect,
    session_kind: SessionKind,
    settings_sub_tab: SettingsSubTab,
    free_tools: Arc<FreeToolsEngine>,
    turn_limits: TurnExternalLimits,
    provider_test_status: Option<String>,

    bg_tx: Sender<BgEvent>,
    bg_rx: Receiver<BgEvent>,
    global_cursor: Arc<Mutex<Option<(i16, i16)>>>,
}

impl RustyDeskApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        retro::apply_theme(&cc.egui_ctx);

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
        let always_on_top = config.always_on_top;
        apply_platform_always_on_top(always_on_top);
        if !always_on_top {
            cc.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                    egui::WindowLevel::Normal,
                ));
        }

        let initial_playlist_id = data.playlists.first().map(|p| p.id.clone());
        let initial_tracks = initial_playlist_id
            .as_deref()
            .and_then(|id| data.load_playlist_tracks(id).ok())
            .unwrap_or_default();

        let mut music_player = MusicPlayer::default();
        music_player.set_volume(config.audio_volume);
        music_player.set_muted(config.audio_muted);

        let screen_bounds = query_x11_virtual_bounds();
        let current_monitor = detect_current_monitor(&cc.egui_ctx, screen_bounds);
        let session_kind = detect_session_kind();

        let mut config = config;
        if !config.island_remember_position {
            config.island_custom_position = None;
        } else if let Some(pos) = config.island_custom_position {
            let clamped =
                clamp_window_position(pos, [360.0, 66.0], Some(current_monitor.as_tuple()));
            config.island_custom_position = Some(clamped);
        }

        let initial_mode = if config.window_mode == WindowMode::Pet && !config.pet.enabled {
            WindowMode::Island
        } else {
            config.window_mode
        };

        let initial_mood = if data.pomodoro.running && data.pomodoro.phase == PomodoroPhase::Work {
            AssistantMood::Focused
        } else if data.pomodoro.pending_prompt == Some(PomodoroPrompt::WorkFinished) {
            AssistantMood::Happy
        } else {
            AssistantMood::Idle
        };

        let free_tools = FreeToolsEngine::open_default().unwrap_or_else(|_| {
            let mem = crate::persistence::Database::open_in_memory()
                .expect("No se pudo abrir DB en memoria");
            FreeToolsEngine::with_database(mem)
        });
        let mut app = Self {
            rt: tokio::runtime::Runtime::new().expect("No se pudo iniciar Tokio"),
            config,
            data,
            organizer: FileOrganizer::default(),
            window_mode: initial_mode,
            always_on_top,
            active_tab: ActiveTab::Chat,
            mood: initial_mood,
            pet_controller: AnimationController::default(),
            pet_behavior: PetBehavior::new(0.0),
            pet_drag_origin: None,
            pet_dragging: false,
            last_frame_time: 0.0,
            screen_bounds,
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
            editing_task_id: None,
            editing_task_title: String::new(),
            pending_focus_switch_task_id: None,
            confirm_clear_completed_tasks: false,
            confirm_skip_break_continue: false,
            new_habit_name: String::new(),
            new_habit_icon: "✨".to_string(),
            new_habit_goal: 1,
            status_banner: initial_banner,
            active_recording: None,
            md_is_converting: false,
            md_auto_save: true,
            md_compact_mode: CompactionMode::Moderate,
            md_source_path: None,
            md_saved_path: None,
            md_output: String::new(),
            md_error: None,
            compact_input: String::new(),
            compact_mode_tab: CompactionMode::Moderate,
            music_player,
            music_m3u_path_input: String::new(),
            music_url_input: String::new(),
            music_url_title_input: String::new(),
            music_selected_playlist_id: initial_playlist_id,
            music_tracks: initial_tracks,
            music_current_track_idx: None,
            music_status: None,
            island_sm: IslandStateMachine::default(),
            island_dragging: false,
            current_monitor,
            session_kind,
            settings_sub_tab: SettingsSubTab::default(),
            free_tools,
            turn_limits: TurnExternalLimits::default(),
            provider_test_status: None,
            bg_tx,
            bg_rx,
            global_cursor,
        };
        if initial_mode != WindowMode::Expanded {
            app.set_window_mode(&cc.egui_ctx, initial_mode);
        }
        app
    }

    fn mood_accent(&self) -> Color32 {
        match self.mood {
            AssistantMood::Idle => retro::CYAN,
            AssistantMood::Listening => retro::ERROR,
            AssistantMood::Thinking => retro::GOLD,
            AssistantMood::Happy => retro::EMERALD,
            AssistantMood::Focused => retro::GOLD_LIGHT,
        }
    }

    fn build_provider(&self) -> Arc<dyn AiProvider> {
        if self.config.should_use_mock() {
            Arc::new(MockProvider)
        } else if self
            .config
            .base_url
            .contains("generativelanguage.googleapis.com")
        {
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

        self.turn_limits = TurnExternalLimits::default();

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
                LocalCommand::PlayMusic(query) => {
                    let msg = self.play_saved_music_by_query(query.as_deref());
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
                LocalCommand::StopMusic => {
                    self.music_player.stop();
                    let msg = "⏹ Reproducción detenida.".to_string();
                    self.music_status = Some("Reproducción detenida.".to_string());
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
                LocalCommand::Weather(city) => {
                    let target_city = city
                        .as_deref()
                        .map(str::trim)
                        .filter(|c| !c.is_empty())
                        .unwrap_or("Buenos Aires")
                        .to_string();
                    let tx = self.bg_tx.clone();
                    let ctx_clone = ctx.clone();
                    let free_tools = Arc::clone(&self.free_tools);
                    let ext_cfg = self.config.external_tools.clone();
                    self.is_thinking = true;
                    self.mood = AssistantMood::Thinking;
                    self.rt.spawn(async move {
                        let res = free_tools
                            .execute_weather(&ext_cfg, &target_city)
                            .await
                            .map_err(|e| e.to_string());
                        let _ = tx.send(BgEvent::WeatherResult(res));
                        ctx_clone.request_repaint();
                    });
                }
                LocalCommand::External { name, args } => {
                    if name == "web_search" {
                        self.chat_ui
                            .push(UiChatEntry::assistant("🔎 Buscando en Internet..."));
                    }
                    let tx = self.bg_tx.clone();
                    let ctx_clone = ctx.clone();
                    let free_tools = Arc::clone(&self.free_tools);
                    let ext_cfg = self.config.external_tools.clone();
                    let tool_name = name.to_string();
                    self.is_thinking = true;
                    self.mood = AssistantMood::Thinking;
                    self.rt.spawn(async move {
                        let result =
                            execute_external_tool_async(&free_tools, &ext_cfg, &tool_name, &args)
                                .await;
                        let _ = tx.send(BgEvent::ExternalCommandResult { tool_name, result });
                        ctx_clone.request_repaint();
                    });
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
                    self.emit_pet_event_for_tool(name, &out.status);
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
        self.emit_pet_event(PetEvent::AiThinking);
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
                    self.emit_pet_event(PetEvent::VoiceListeningStop);
                    self.emit_pet_event(PetEvent::AiThinking);
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
                    self.emit_pet_event(PetEvent::VoiceListeningStart);
                    self.status_banner =
                        Some("Escuchando... volvé a tocar el micrófono para enviar.".to_string());
                }
                Err(e) => {
                    self.emit_pet_event(PetEvent::ErrorOccurred);
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
                        let mut external_batch: Vec<(String, String, serde_json::Value)> =
                            Vec::new();
                        for tc in &tool_calls {
                            let args_val: serde_json::Value =
                                serde_json::from_str(&tc.function.arguments)
                                    .unwrap_or(serde_json::Value::Null);
                            if is_external_tool_name(&tc.function.name) {
                                if let Err(limit_err) =
                                    self.turn_limits.try_acquire(&tc.function.name)
                                {
                                    let tool_msg =
                                        ChatMessage::tool_result(&tc.id, limit_err.to_string());
                                    let r = self.data.record_chat_message(
                                        &self.current_chat_id,
                                        None,
                                        &tool_msg,
                                        MessageKind::ToolError,
                                    );
                                    self.report_persistence_result(r);
                                    history_snapshot.push(tool_msg);
                                } else {
                                    external_batch.push((
                                        tc.id.clone(),
                                        tc.function.name.clone(),
                                        args_val,
                                    ));
                                }
                                continue;
                            }
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
                            self.emit_pet_event_for_tool(&tc.function.name, &out.status);

                            match out.status {
                                ToolStatus::NeedsConfirmation(pending) => {
                                    waiting_for_confirmation = true;
                                    self.pending_confirmation = Some(pending);
                                    let tool_msg =
                                        ChatMessage::tool_result(&tc.id, out.llm_message);
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
                                    let tool_msg =
                                        ChatMessage::tool_result(&tc.id, out.llm_message);
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
                                    let tool_msg =
                                        ChatMessage::tool_result(&tc.id, out.llm_message);
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

                        if !external_batch.is_empty() {
                            if external_batch
                                .iter()
                                .any(|(_, name, _)| name == "web_search")
                            {
                                self.chat_ui
                                    .push(UiChatEntry::assistant("🔎 Buscando en Internet..."));
                            }
                            let free_tools = Arc::clone(&self.free_tools);
                            let ext_cfg = self.config.external_tools.clone();
                            let tx = self.bg_tx.clone();
                            let ctx_clone = ctx.clone();
                            self.rt.spawn(async move {
                                let mut results = Vec::new();
                                for (tc_id, name, args) in external_batch {
                                    match execute_external_tool_async(
                                        &free_tools,
                                        &ext_cfg,
                                        &name,
                                        &args,
                                    )
                                    .await
                                    {
                                        Ok(text) => results.push((tc_id, text, false)),
                                        Err(err) => results.push((tc_id, err, true)),
                                    }
                                }
                                let _ = tx.send(BgEvent::ExternalAiToolsDone {
                                    results,
                                    history_snapshot,
                                    new_mood,
                                    waiting_for_confirmation,
                                });
                                ctx_clone.request_repaint();
                            });
                            continue;
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
                        self.emit_pet_event(PetEvent::AiReplyReady);
                        if self.config.tts_enabled {
                            speak_local_async(final_text);
                        }
                    }
                }
                BgEvent::AiError(err) => {
                    self.is_thinking = false;
                    self.mood = AssistantMood::Idle;
                    self.emit_pet_event(PetEvent::AiError);
                    // Roll back poisoned tool turns so subsequent messages keep working
                    while self
                        .chat_history
                        .last()
                        .is_some_and(|m| m.role == "tool" || m.tool_calls.is_some())
                    {
                        self.chat_history.pop();
                    }
                    sfx::play(SfxEvent::Error, self.config.sfx_enabled);
                    self.chat_ui
                        .push(UiChatEntry::assistant(format!("⚠️ {err}")));
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
                    self.emit_pet_event(PetEvent::ErrorOccurred);
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
                    self.emit_pet_event(PetEvent::MarkdownConverted);
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
                    self.emit_pet_event(PetEvent::ErrorOccurred);
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
                BgEvent::M3uPicked(maybe_path) => {
                    if let Some(path) = maybe_path {
                        self.import_m3u_from_path(&path);
                    }
                }
                BgEvent::M3uPickError(err) => {
                    self.music_status = Some(format!("⚠️ {err}"));
                }
                BgEvent::WeatherResult(res) => {
                    self.is_thinking = false;
                    let msg = match res {
                        Ok(text) => {
                            self.mood = AssistantMood::Happy;
                            text
                        }
                        Err(err) => {
                            self.mood = AssistantMood::Idle;
                            format!("⚠️ {err}")
                        }
                    };
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
                    sfx::play(SfxEvent::MessageReceived, self.config.sfx_enabled);
                }
                BgEvent::ExternalCommandResult {
                    tool_name: _,
                    result,
                } => {
                    self.is_thinking = false;
                    let msg = match result {
                        Ok(text) => {
                            self.mood = AssistantMood::Happy;
                            text
                        }
                        Err(err) => {
                            self.mood = AssistantMood::Idle;
                            format!("⚠️ {err}")
                        }
                    };
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
                    sfx::play(SfxEvent::MessageReceived, self.config.sfx_enabled);
                }
                BgEvent::ExternalAiToolsDone {
                    results,
                    mut history_snapshot,
                    new_mood,
                    waiting_for_confirmation,
                } => {
                    for (tc_id, text, is_err) in results {
                        let kind = if is_err {
                            MessageKind::ToolError
                        } else {
                            MessageKind::ToolResult
                        };
                        let tool_msg = ChatMessage::tool_result(&tc_id, text);
                        let r = self.data.record_chat_message(
                            &self.current_chat_id,
                            None,
                            &tool_msg,
                            kind,
                        );
                        self.report_persistence_result(r);
                        history_snapshot.push(tool_msg);
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
                }
                BgEvent::ProviderTestResult(msg) => {
                    self.provider_test_status = Some(msg);
                }
            }
        }
    }

    fn tick_pomodoro(&mut self, ctx: &egui::Context) {
        if self.data.pomodoro.running {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
            if let Some(outcome) = self.data.sync_pomodoro_elapsed() {
                let msg = match outcome {
                    PomodoroTickOutcome::WorkFinished(_) => {
                        self.mood = AssistantMood::Happy;
                        self.emit_pet_event(PetEvent::PomodoroFinished);
                        sfx::play(SfxEvent::PomodoroDone, self.config.sfx_enabled);
                        if let Some(title) = self.data.active_task_display_title() {
                            format!("¡Terminaste tu bloque de concentración en «{title}»!")
                        } else {
                            "¡Sesión Pomodoro completada! Hora de descansar.".to_string()
                        }
                    }
                    PomodoroTickOutcome::BreakFinished => {
                        self.mood = AssistantMood::Idle;
                        self.emit_pet_event(PetEvent::PomodoroPaused);
                        sfx::play(SfxEvent::Click, self.config.sfx_enabled);
                        "Descanso terminado. ¿Seguimos?".to_string()
                    }
                };
                self.chat_ui.push(UiChatEntry::assistant(&msg));
                if self.config.tts_enabled {
                    speak_local_async(msg);
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
        let tool_name = pending.tool_name.clone();
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
        self.emit_pet_event_for_tool(&tool_name, &out.status);
        let msg_text = if out.status == ToolStatus::Error {
            format!("⚠️ {}", out.ui_message)
        } else {
            out.ui_message
        };
        let reply = ChatMessage::assistant_text(&msg_text);
        self.chat_ui.push(UiChatEntry::assistant(&msg_text));
        self.chat_history.push(reply.clone());
        let r =
            self.data
                .record_chat_message(&self.current_chat_id, None, &reply, MessageKind::Text);
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
        let r =
            self.data
                .record_chat_message(&self.current_chat_id, None, &reply, MessageKind::Text);
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

        if let Some(first_m3u) = paths.iter().find(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("m3u") || e.eq_ignore_ascii_case("m3u8"))
        }) {
            self.import_m3u_from_path(first_m3u);
            if self.window_mode == WindowMode::Expanded {
                self.active_tab = ActiveTab::Music;
            }
            return;
        }

        if self.window_mode == WindowMode::Expanded && self.active_tab == ActiveTab::Chat {
            self.notify_chat_files(&paths, ctx);
        } else {
            if self.window_mode == WindowMode::Expanded {
                self.active_tab = ActiveTab::Markdown;
            }
            if let Some(first) = paths.into_iter().next() {
                self.start_markdown_conversion(first, ctx);
            }
        }
    }

    fn emit_pet_event(&mut self, event: PetEvent) {
        self.pet_behavior.record_activity(self.last_frame_time);
        self.pet_controller
            .on_event(event, &self.config.pet, self.mood);
    }

    fn emit_pet_event_for_tool(&mut self, tool_name: &str, status: &ToolStatus) {
        if *status == ToolStatus::Error {
            self.emit_pet_event(PetEvent::ErrorOccurred);
            return;
        }
        if *status != ToolStatus::Success {
            return;
        }
        match tool_name {
            "complete_task" => self.emit_pet_event(PetEvent::TaskCompleted),
            "log_habit" => self.emit_pet_event(PetEvent::HabitCompleted),
            "start_pomodoro" => self.emit_pet_event(PetEvent::PomodoroStarted),
            "pause_pomodoro" => {
                if self.data.pomodoro.running {
                    self.emit_pet_event(PetEvent::PomodoroStarted);
                } else {
                    self.emit_pet_event(PetEvent::PomodoroPaused);
                }
            }
            "convert_to_markdown" => self.emit_pet_event(PetEvent::MarkdownConverted),
            _ => {}
        }
    }

    fn toggle_pomodoro_action(&mut self) {
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
        if let Some(m) = out.mood {
            self.mood = m;
        }
        self.emit_pet_event_for_tool("pause_pomodoro", &out.status);
    }

    fn island_expanded_size(&self, music_active: bool) -> [f32; 2] {
        if music_active {
            [360.0, 118.0]
        } else {
            [360.0, 66.0]
        }
    }

    fn set_window_mode(&mut self, ctx: &egui::Context, target: WindowMode) {
        if self.window_mode == WindowMode::Pet && target != WindowMode::Pet {
            if let Some(outer) = ctx.input(|i| i.viewport().outer_rect) {
                self.config.pet.pet_pos = Some([outer.min.x, outer.min.y]);
                let _ = self.config.save();
            }
            self.pet_drag_origin = None;
            self.pet_dragging = false;
        }
        if self.window_mode == WindowMode::Island && target != WindowMode::Island {
            if self.config.island_position == IslandPosition::Custom
                && self.config.island_remember_position
            {
                if let Some(outer) = ctx.input(|i| i.viewport().outer_rect) {
                    self.config.island_custom_position = Some([outer.min.x, outer.min.y]);
                }
            }
            self.island_dragging = false;
        }

        let target = if target == WindowMode::Pet && !self.config.pet.enabled {
            WindowMode::Island
        } else {
            target
        };

        self.window_mode = target;
        if self.config.window_mode != target {
            self.config.window_mode = target;
            let _ = self.config.save();
        }
        let music_active = self.music_player.is_active();
        let (min_size, size) = match target {
            WindowMode::Expanded => (Vec2::new(320.0, 360.0), Vec2::new(450.0, 650.0)),
            WindowMode::Island => {
                self.island_sm.reset_visible();
                let s = self.island_expanded_size(music_active);
                (Vec2::new(10.0, 10.0), Vec2::from(s))
            }
            WindowMode::Pet => {
                let s = self.config.pet.size.pixels();
                (Vec2::new(64.0, 64.0), Vec2::splat(s))
            }
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(min_size));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));

        let want_on_top = match target {
            WindowMode::Pet => self.config.pet.always_on_top,
            _ => self.always_on_top,
        };
        let level = if want_on_top {
            egui::WindowLevel::AlwaysOnTop
        } else {
            egui::WindowLevel::Normal
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
        apply_platform_always_on_top(want_on_top);

        match target {
            WindowMode::Pet => {
                if let Some(pos) = self.config.pet.pet_pos {
                    let clamped = clamp_pet_position(
                        pos,
                        self.config.pet.size.pixels(),
                        Some(self.current_monitor.as_tuple()),
                    );
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(Pos2::new(
                        clamped[0], clamped[1],
                    )));
                }
            }
            WindowMode::Island => {
                if self.session_kind != SessionKind::Wayland {
                    if let Some(pos) = crate::config::compute_island_position(
                        self.config.island_position,
                        [size.x, size.y],
                        Some(self.current_monitor.as_tuple()),
                        self.config.island_margin,
                        self.config.island_custom_position,
                    ) {
                        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(Pos2::new(
                            pos[0], pos[1],
                        )));
                        let _ = self
                            .island_sm
                            .should_apply_geometry([size.x, size.y], Some(pos));
                    }
                }
            }
            WindowMode::Expanded => {
                if let Some(outer) = ctx.input(|i| i.viewport().outer_rect) {
                    let pos = [outer.min.x, outer.min.y];
                    let clamped = clamp_window_position(
                        pos,
                        [size.x, size.y],
                        Some(self.current_monitor.as_tuple()),
                    );
                    if clamped != pos {
                        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(Pos2::new(
                            clamped[0], clamped[1],
                        )));
                    }
                }
            }
        }

        sfx::play(SfxEvent::Click, self.config.sfx_enabled);
        self.emit_pet_event(PetEvent::ModeChanged(target));
    }

    fn handle_drag(
        &mut self,
        ui: &egui::Ui,
        ctx: &egui::Context,
        rect: egui::Rect,
    ) -> egui::Response {
        let resp = ui.interact(
            rect,
            ui.id().with("island_drag"),
            egui::Sense::click_and_drag(),
        );
        if resp.drag_started() {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        self.island_dragging = resp.dragged();
        if resp.drag_stopped() && self.window_mode == WindowMode::Island {
            if let Some(outer) = ctx.input(|i| i.viewport().outer_rect) {
                let (snapped_edge, snapped_pos) = crate::ui::island::snap_island_to_edge(
                    [outer.min.x, outer.min.y],
                    [outer.width(), outer.height()],
                    self.current_monitor,
                    self.config.island_margin,
                );
                if snapped_edge == IslandPosition::Custom {
                    self.config.island_position = IslandPosition::Custom;
                    self.config.island_custom_position = Some(snapped_pos);
                } else {
                    self.config.island_position = IslandPosition::Custom;
                    self.config.island_custom_position = Some(snapped_pos);
                    if self.session_kind != SessionKind::Wayland {
                        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(Pos2::new(
                            snapped_pos[0],
                            snapped_pos[1],
                        )));
                    }
                }
                if self.config.island_remember_position {
                    let _ = self.config.save();
                }
                self.island_sm.invalidate_geometry();
            }
        }
        resp
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
        if self.music_player.poll_finished() {
            self.play_next_music_track();
        }

        let time = ctx.input(|i| i.time);
        let dt = if self.last_frame_time > 0.0 {
            (time - self.last_frame_time) as f32
        } else {
            0.033
        };
        self.last_frame_time = time;

        self.current_monitor = detect_current_monitor(ctx, self.screen_bounds);

        let music_active = self.music_player.is_active();
        let music_playing = self.music_player.is_playing();
        let ai_busy = self.is_thinking || self.md_is_converting;
        let is_busy = ai_busy || self.active_recording.is_some() || music_playing;
        self.pet_controller.set_music_playing(music_playing);
        if let Some(auto_event) = self.pet_behavior.tick(time, is_busy, &self.config.pet) {
            self.pet_controller
                .on_event(auto_event, &self.config.pet, self.mood);
        }
        let pet_frame = self.pet_controller.tick(dt, self.mood, &self.config.pet);

        let pointer_pos = self.resolve_look_target(ctx);
        let accent = self.mood_accent();
        let reduce = self.config.pet.reduce_animations;
        let follow = self.config.pet.follow_cursor;

        let outer_frame = egui::Frame::none().fill(Color32::TRANSPARENT);

        if self.window_mode == WindowMode::Pet {
            let mut is_hovered = false;
            egui::CentralPanel::default()
                .frame(outer_frame)
                .show(ctx, |ui| {
                    is_hovered = self.render_pet_mode(ui, ctx, time, pointer_pos, &pet_frame);
                });
            let repaint_ms = self
                .pet_controller
                .recommended_repaint_ms(is_hovered, &self.config.pet);
            ctx.request_repaint_after(std::time::Duration::from_millis(repaint_ms));
            return;
        }

        if self.window_mode == WindowMode::Island {
            let expanded_size = self.island_expanded_size(music_active);
            let (pointer_in_window, is_interacting, has_open_menu) = (
                ctx.input(|i| i.pointer.hover_pos().is_some()),
                ctx.input(|i| i.pointer.any_down()) || ctx.wants_pointer_input(),
                ctx.memory(|m| m.any_popup_open()),
            );
            let is_hidden_now = self.island_sm.visibility == IslandVisibility::Hidden;
            let island_in = IslandInput {
                now: time,
                dt,
                auto_hide: self.config.island_auto_hide,
                hide_delay_secs: self.config.island_hide_delay,
                show_on_hover: self.config.island_show_on_hover,
                keep_visible_during_playback: self.config.island_keep_visible_during_playback,
                keep_visible_during_ai: self.config.island_keep_visible_during_ai,
                pointer_in_island: !is_hidden_now && pointer_in_window,
                pointer_in_tab: is_hidden_now && pointer_in_window,
                is_dragging: self.island_dragging,
                is_interacting,
                has_open_menu,
                is_recording: self.active_recording.is_some(),
                music_active,
                ai_busy,
            };
            let out = self.island_sm.step(
                &island_in,
                self.config.island_position,
                self.current_monitor,
                self.config.island_margin,
                self.config.island_custom_position,
                expanded_size,
            );

            if self.island_sm.should_apply_geometry(out.size, out.pos) {
                ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(Vec2::new(10.0, 10.0)));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::from(out.size)));
                if self.session_kind != SessionKind::Wayland {
                    if let Some([x, y]) = out.pos {
                        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(Pos2::new(
                            x, y,
                        )));
                    }
                }
            }

            let sprite_ms = self
                .pet_controller
                .recommended_repaint_ms(false, &self.config.pet);
            let repaint_ms = if out.is_animating { 16 } else { sprite_ms };
            ctx.request_repaint_after(std::time::Duration::from_millis(repaint_ms));

            if out.visibility == IslandVisibility::Hidden {
                egui::CentralPanel::default()
                    .frame(outer_frame)
                    .show(ctx, |ui| {
                        self.render_island_hidden_tab(ui, &pet_frame, reduce);
                    });
                return;
            }

            egui::CentralPanel::default()
                .frame(outer_frame)
                .show(ctx, |ui| {
                    let island_frame = console_frame(12.0, 8.0);

                    island_frame.show(ui, |ui| {
                        ui.set_min_size(ui.available_size());
                        let controls_enabled = out.visibility != IslandVisibility::Hiding;
                        ui.add_enabled_ui(controls_enabled, |ui| {
                            self.render_compact_island(ui, ctx, pointer_pos, accent, &pet_frame);
                        });
                    });
                });
            return;
        }

        let repaint_ms = self
            .pet_controller
            .recommended_repaint_ms(false, &self.config.pet);
        ctx.request_repaint_after(std::time::Duration::from_millis(repaint_ms));

        egui::CentralPanel::default()
            .frame(outer_frame)
            .show(ctx, |ui| {
                let island_frame = console_frame(14.0, 10.0);

                island_frame.show(ui, |ui| {
                    ui.set_min_size(ui.available_size());

                    if self.window_mode == WindowMode::Island {
                        self.render_compact_island(ui, ctx, pointer_pos, accent, &pet_frame);
                        return;
                    }

                    // Buttons are allocated from the right so the name and status keep the rest.
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if pixel_button(
                                ui,
                                PixelIcon::Close,
                                "",
                                retro::DANGER,
                                Color32::WHITE,
                                "Cerrar",
                            )
                            .clicked()
                            {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                            if self.config.pet.enabled
                                && pixel_button(
                                    ui,
                                    PixelIcon::Pet,
                                    "",
                                    BUTTON,
                                    IVORY,
                                    "Cambiar a modo Mascota",
                                )
                                .clicked()
                            {
                                self.set_window_mode(ctx, WindowMode::Pet);
                            }
                            if pixel_button(
                                ui,
                                PixelIcon::Island,
                                "",
                                BUTTON,
                                IVORY,
                                "Cambiar a modo Isla",
                            )
                            .clicked()
                            {
                                self.set_window_mode(ctx, WindowMode::Island);
                            }
                            let (pin_tip, pin_bg) = if self.always_on_top {
                                ("Dejar de fijar la ventana", retro::GOLD_DARK)
                            } else {
                                ("Mantener siempre visible", BUTTON)
                            };
                            if pixel_button(ui, PixelIcon::Pin, "", pin_bg, IVORY, pin_tip)
                                .clicked()
                            {
                                self.always_on_top = !self.always_on_top;
                                let level = if self.always_on_top {
                                    egui::WindowLevel::AlwaysOnTop
                                } else {
                                    egui::WindowLevel::Normal
                                };
                                ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
                                apply_platform_always_on_top(self.always_on_top);
                            }

                            let drag_area = ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    draw_rusty(ui, 80.0, &pet_frame, reduce, pointer_pos, follow);
                                    ui.add_space(6.0);
                                    ui.vertical(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(&self.config.assistant_name)
                                                    .strong()
                                                    .size(16.0)
                                                    .color(IVORY),
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
                                                .rounding(0.0)
                                                .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                                .show(ui, |ui| {
                                                    ui.label(
                                                        RichText::new(badge)
                                                            .size(11.0)
                                                            .color(accent),
                                                    );
                                                });
                                        });
                                        let status_text = if self.data.pomodoro.running {
                                            format!(
                                                "Enfoque: {}",
                                                self.data.pomodoro.formatted_time()
                                            )
                                        } else if self.is_thinking {
                                            "Pensando...".to_string()
                                        } else {
                                            let pending =
                                                self.data.tasks.iter().filter(|t| !t.done).count();
                                            format!("{pending} pendientes • Arrastrar aqui")
                                        };
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(status_text)
                                                    .size(11.5)
                                                    .color(retro::MUTED),
                                            )
                                            .wrap(),
                                        );
                                    });
                                },
                            );
                            let _ = self.handle_drag(ui, ctx, drag_area.response.rect);
                        });
                    });

                    ui.add_space(8.0);

                    egui::Frame::none()
                        .fill(retro::PANEL)
                        .stroke(Stroke::new(2.0_f32, retro::BORDER))
                        .rounding(0.0)
                        .inner_margin(4.0)
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new(self.active_tab.title())
                                        .color(retro::GOLD_LIGHT)
                                        .size(13.0),
                                );
                                nav_pill(
                                    ui,
                                    &mut self.active_tab,
                                    ActiveTab::Chat,
                                    PixelIcon::Chat,
                                    "Chat",
                                    accent,
                                );
                                nav_pill(
                                    ui,
                                    &mut self.active_tab,
                                    ActiveTab::Tasks,
                                    PixelIcon::Tasks,
                                    "Tareas",
                                    accent,
                                );
                                nav_pill(
                                    ui,
                                    &mut self.active_tab,
                                    ActiveTab::Habits,
                                    PixelIcon::Habits,
                                    "Hábitos",
                                    accent,
                                );
                                nav_pill(
                                    ui,
                                    &mut self.active_tab,
                                    ActiveTab::Focus,
                                    PixelIcon::Focus,
                                    "Pomodoro",
                                    accent,
                                );
                                nav_pill(
                                    ui,
                                    &mut self.active_tab,
                                    ActiveTab::Music,
                                    PixelIcon::MusicNote,
                                    "Música",
                                    accent,
                                );
                                nav_pill(
                                    ui,
                                    &mut self.active_tab,
                                    ActiveTab::Organizer,
                                    PixelIcon::Organizer,
                                    "Organizador",
                                    accent,
                                );
                                nav_pill(
                                    ui,
                                    &mut self.active_tab,
                                    ActiveTab::Markdown,
                                    PixelIcon::Markdown,
                                    "MarkItDown",
                                    accent,
                                );
                                nav_pill(
                                    ui,
                                    &mut self.active_tab,
                                    ActiveTab::Compact,
                                    PixelIcon::Compact,
                                    "Compactar",
                                    accent,
                                );
                                nav_pill(
                                    ui,
                                    &mut self.active_tab,
                                    ActiveTab::Settings,
                                    PixelIcon::Settings,
                                    "Configuración",
                                    accent,
                                );
                            });
                        });

                    ui.add_space(8.0);

                    if music_active {
                        egui::Frame::none()
                            .fill(Color32::from_rgb(20, 27, 45))
                            .rounding(0.0)
                            .stroke(Stroke::new(1.0_f32, accent.linear_multiply(0.45)))
                            .inner_margin(egui::Margin::symmetric(8.0, 5.0))
                            .show(ui, |ui| {
                                self.render_music_mini_controls(ui, accent, false);
                            });
                        ui.add_space(6.0);
                    }

                    if let Some(banner) = &self.status_banner {
                        egui::Frame::none()
                            .fill(Color32::from_rgba_unmultiplied(250, 204, 21, 25))
                            .rounding(0.0)
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
                        ActiveTab::Music => self.render_music_tab(ui, ctx, accent),
                        ActiveTab::Organizer => self.render_organizer_tab(ui, accent),
                        ActiveTab::Markdown => self.render_markdown_tab(ui, ctx, accent),
                        ActiveTab::Compact => self.render_compact_tab(ui, ctx, accent),
                        ActiveTab::Settings => self.render_settings_tab(ui, accent),
                    }
                });
            });
    }
}

impl RustyDeskApp {
    fn render_pet_mode(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        time: f64,
        pointer_pos: Option<Pos2>,
        frame: &AnimationFrame,
    ) -> bool {
        let size = self.config.pet.size.pixels();
        let look = if self.config.pet.follow_cursor {
            pointer_pos
        } else {
            None
        };

        let resp = PetRenderer::render(
            ui,
            size,
            frame,
            self.config.pet.reduce_animations,
            look,
            self.config.pet.follow_cursor,
        );

        if resp.hovered() {
            self.pet_behavior.record_activity(time);
            self.pet_controller
                .on_event(PetEvent::UserHovered, &self.config.pet, self.mood);
        }

        // Distinguish short click (> open Island) from drag (>5px threshold)
        if resp.is_pointer_button_down_on() && ctx.input(|i| i.pointer.primary_down()) {
            if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                match self.pet_drag_origin {
                    None => {
                        self.pet_drag_origin = Some(pos);
                        self.pet_dragging = false;
                    }
                    Some(origin) => {
                        if !self.pet_dragging && origin.distance(pos) > 5.0 {
                            self.pet_dragging = true;
                            self.emit_pet_event(PetEvent::UserDragged);
                            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                        }
                    }
                }
            }
        }

        if ctx.input(|i| i.pointer.primary_released()) {
            if self.pet_dragging {
                if let Some(outer) = ctx.input(|i| i.viewport().outer_rect) {
                    self.config.pet.pet_pos = Some([outer.min.x, outer.min.y]);
                    let _ = self.config.save();
                }
            }
            self.pet_drag_origin = None;
            self.pet_dragging = false;
        }

        if resp.clicked() && !self.pet_dragging {
            self.emit_pet_event(PetEvent::UserClicked);
            self.set_window_mode(ctx, WindowMode::Island);
        }

        resp.context_menu(|ui| {
            ui.set_min_width(170.0);
            if ui.button("Abrir asistente").clicked() {
                self.set_window_mode(ctx, WindowMode::Expanded);
                ui.close_menu();
            }
            if ui.button("Abrir Isla").clicked() {
                self.set_window_mode(ctx, WindowMode::Island);
                ui.close_menu();
            }
            let pomo_label = if self.data.pomodoro.running {
                "Pausar Pomodoro"
            } else {
                "Iniciar Pomodoro"
            };
            if ui.button(pomo_label).clicked() {
                self.toggle_pomodoro_action();
                ui.close_menu();
            }
            if ui.button("Configuración").clicked() {
                self.active_tab = ActiveTab::Settings;
                self.set_window_mode(ctx, WindowMode::Expanded);
                ui.close_menu();
            }
            if self.music_player.is_active() {
                ui.separator();
                ui.label(
                    RichText::new(format!("♫ {}", self.current_music_title()))
                        .size(11.5)
                        .strong(),
                );
                ui.horizontal(|ui| {
                    let pause_lbl = if self.music_player.is_paused() {
                        "Reanudar"
                    } else {
                        "Pausar"
                    };
                    if ui.small_button(pause_lbl).clicked() {
                        self.music_player.toggle_pause();
                    }
                    if ui.small_button("Detener").clicked() {
                        self.stop_music();
                    }
                });
                self.render_volume_controls(ui, 90.0, true);
            }
            ui.separator();
            if ui.button("Ocultar mascota").clicked() {
                self.config.pet.enabled = false;
                let _ = self.config.save();
                self.set_window_mode(ctx, WindowMode::Island);
                ui.close_menu();
            }
            if ui.button("Cerrar rusty-desk").clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                ui.close_menu();
            }
        });

        resp.hovered()
    }

    fn render_island_hidden_tab(
        &mut self,
        ui: &mut egui::Ui,
        frame: &AnimationFrame,
        reduce: bool,
    ) {
        let avail = ui.available_rect_before_wrap();
        let painter = ui.painter();
        painter.rect_filled(
            avail,
            0.0,
            Color32::from_rgba_unmultiplied(NIGHT.r(), NIGHT.g(), NIGHT.b(), 245),
        );
        painter.rect_stroke(avail, 0.0, Stroke::new(2.0_f32, retro::GOLD));
        let side = crate::ui::sprites::display_side(avail.width().min(avail.height()), 96.0);
        let sprite = egui::Rect::from_center_size(avail.center(), Vec2::splat(side));
        crate::ui::sprites::paint_rusty(painter, sprite, ui.ctx(), frame, reduce, None);
    }

    fn render_compact_island(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        pointer_pos: Option<Pos2>,
        accent: Color32,
        frame: &AnimationFrame,
    ) {
        let music_active = self.music_player.is_active();
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                let drag_area = ui.horizontal(|ui| {
                    draw_rusty(
                        ui,
                        48.0,
                        frame,
                        self.config.pet.reduce_animations,
                        pointer_pos,
                        self.config.pet.follow_cursor,
                    );
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        let pomo_active = self.data.pomodoro.phase != PomodoroPhase::Idle;
                        let title_line = if pomo_active {
                            let icon = match self.data.pomodoro.phase {
                                PomodoroPhase::Work => "🎯",
                                PomodoroPhase::Break => "☕",
                                PomodoroPhase::Idle => "",
                            };
                            let pause_tag = if self.data.pomodoro.running {
                                ""
                            } else {
                                " (pausa)"
                            };
                            format!(
                                "{} {} {}{}",
                                self.config.assistant_name,
                                icon,
                                self.data.pomodoro.formatted_time(),
                                pause_tag
                            )
                        } else {
                            self.config.assistant_name.clone()
                        };
                        ui.label(
                            RichText::new(title_line)
                                .strong()
                                .size(13.0)
                                .color(Color32::WHITE),
                        );
                        let sub = if pomo_active {
                            if let Some(t) = self.data.active_task_display_title() {
                                truncate_chars(&t, 26)
                            } else if self.data.pomodoro.phase == PomodoroPhase::Break {
                                "Descanso".to_string()
                            } else {
                                "Enfoque activo".to_string()
                            }
                        } else if let Some(prompt) = self.data.pomodoro.pending_prompt {
                            match prompt {
                                PomodoroPrompt::WorkFinished => "¡Bloque terminado!".to_string(),
                                PomodoroPrompt::BreakFinished => "Descanso terminado".to_string(),
                            }
                        } else if self.is_thinking {
                            "Pensando...".to_string()
                        } else {
                            let pending = self.data.tasks.iter().filter(|t| !t.done).count();
                            format!("{pending} pendientes")
                        };
                        ui.label(RichText::new(sub).size(11.0).color(accent));
                    });
                });
                let drag_resp = self.handle_drag(ui, ctx, drag_area.response.rect);
                if drag_resp.double_clicked() && !drag_resp.dragged() {
                    self.set_window_mode(ctx, WindowMode::Expanded);
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if pixel_button(
                        ui,
                        PixelIcon::Close,
                        "",
                        retro::DANGER,
                        Color32::WHITE,
                        "Cerrar",
                    )
                    .clicked()
                    {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if self.config.pet.enabled
                        && pixel_button(
                            ui,
                            PixelIcon::Pet,
                            "",
                            BUTTON,
                            IVORY,
                            "Cambiar a modo Mascota",
                        )
                        .clicked()
                    {
                        self.set_window_mode(ctx, WindowMode::Pet);
                    }
                    if pixel_button(ui, PixelIcon::Expand, "", BUTTON, IVORY, "Expandir panel")
                        .clicked()
                    {
                        self.set_window_mode(ctx, WindowMode::Expanded);
                    }
                    if self.data.pomodoro.phase != PomodoroPhase::Idle {
                        let (p_icon, p_tip) = if self.data.pomodoro.running {
                            (PixelIcon::Pause, "Pausar Pomodoro")
                        } else {
                            (PixelIcon::Play, "Reanudar Pomodoro")
                        };
                        if pixel_button(
                            ui,
                            p_icon,
                            "",
                            accent,
                            Color32::from_rgb(15, 20, 32),
                            p_tip,
                        )
                        .clicked()
                        {
                            self.data.toggle_pomodoro_and_persist();
                            self.mood = if self.data.pomodoro.running
                                && self.data.pomodoro.phase == PomodoroPhase::Work
                            {
                                AssistantMood::Focused
                            } else {
                                AssistantMood::Idle
                            };
                            sfx::play(SfxEvent::Click, self.config.sfx_enabled);
                        }
                    } else {
                        let mic_bg = if self.active_recording.is_some() {
                            retro::DANGER
                        } else {
                            BUTTON
                        };
                        if pixel_button(
                            ui,
                            PixelIcon::Mic,
                            "",
                            mic_bg,
                            Color32::WHITE,
                            if self.active_recording.is_some() {
                                "Detener grabación"
                            } else {
                                "Hablar por micrófono"
                            },
                        )
                        .clicked()
                        {
                            self.toggle_voice_recording(ctx);
                        }
                    }
                });
            });

            if music_active {
                ui.add_space(4.0);
                self.render_music_mini_controls(ui, accent, true);
            }
        });
    }

    fn render_chat_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, accent: Color32) {
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            egui::Frame::none()
                .fill(Color32::from_rgba_unmultiplied(
                    accent.r(),
                    accent.g(),
                    accent.b(),
                    30,
                ))
                .stroke(Stroke::new(1.5_f32, accent))
                .rounding(0.0)
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
                    .rounding(0.0);
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
                .rounding(0.0)
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
                .rounding(0.0)
                .inner_margin(8.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Chats guardados").strong().size(12.0));
                        if total > PAGE_SIZE {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if self.history_page < max_page
                                        && ui.small_button(">").clicked()
                                    {
                                        self.history_page += 1;
                                    }
                                    ui.label(
                                        RichText::new(format!(
                                            "Pág {}/{}",
                                            self.history_page + 1,
                                            max_page + 1
                                        ))
                                        .size(11.0)
                                        .color(Color32::GRAY),
                                    );
                                    if self.history_page > 0 && ui.small_button("<").clicked() {
                                        self.history_page -= 1;
                                    }
                                },
                            );
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
                                        .rounding(0.0);
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
                            Color32::from_rgb(42, 32, 16),
                            retro::GOLD_DARK,
                            egui::Align::RIGHT,
                        )
                    } else {
                        (retro::PANEL, retro::BORDER, egui::Align::LEFT)
                    };
                    ui.with_layout(egui::Layout::top_down(align), |ui| {
                        egui::Frame::none()
                            .fill(bg)
                            .stroke(Stroke::new(1.0_f32, border))
                            .rounding(0.0)
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
                .rounding(0.0)
                .inner_margin(5.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (mic_text, mic_bg) = if self.active_recording.is_some() {
                            ("Parar", Color32::from_rgb(190, 18, 60))
                        } else {
                            ("Voz", Color32::from_rgb(30, 41, 59))
                        };
                        if pixel_button(
                            ui,
                            PixelIcon::Mic,
                            mic_text,
                            mic_bg,
                            Color32::WHITE,
                            "Grabar voz",
                        )
                        .clicked()
                        {
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

                        if pixel_button(
                            ui,
                            send_icon,
                            send_label,
                            send_fill,
                            send_fg,
                            if self.is_thinking {
                                "Cancelar respuesta"
                            } else {
                                "Enviar mensaje · Enter"
                            },
                        )
                        .clicked()
                        {
                            if self.is_thinking {
                                self.is_thinking = false;
                                self.mood = AssistantMood::Idle;
                                self.chat_ui
                                    .push(UiChatEntry::assistant("Solicitud cancelada."));
                            } else if !self.chat_input.trim().is_empty() {
                                let msg = std::mem::take(&mut self.chat_input);
                                self.send_user_message(msg, ctx);
                            }
                        } else if enter_pressed
                            && !self.is_thinking
                            && !self.chat_input.trim().is_empty()
                        {
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
                && !self.new_task_input.trim().is_empty()
            {
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

        if let Some(switch_id) = self.pending_focus_switch_task_id.clone() {
            let current_label = self
                .data
                .active_task_display_title()
                .or_else(|| self.data.pomodoro.active_task_title.clone())
                .unwrap_or_else(|| "la sesión actual".to_string());
            egui::Frame::none()
                .fill(Color32::from_rgb(30, 41, 59))
                .stroke(Stroke::new(1.5_f32, Color32::from_rgb(250, 204, 21)))
                .rounding(0.0)
                .inner_margin(8.0)
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!(
                            "Ya estás concentrado en «{current_label}». ¿Querés cambiar?"
                        ))
                        .size(12.0)
                        .color(Color32::WHITE),
                    );
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.button("Cambiar tarea").clicked() {
                            self.data.focus_on_task(&switch_id, true);
                            self.mood = AssistantMood::Focused;
                            self.emit_pet_event(PetEvent::PomodoroStarted);
                            sfx::play(SfxEvent::PomodoroStart, self.config.sfx_enabled);
                            self.pending_focus_switch_task_id = None;
                        }
                        if ui.button("Cancelar").clicked() {
                            self.pending_focus_switch_task_id = None;
                        }
                    });
                });
            ui.add_space(6.0);
        }

        let mut remove_idx = None;
        let mut toggled_task: Option<TaskItem> = None;
        let mut focus_task_id: Option<String> = None;
        let mut rename_commit: Option<(String, String)> = None;
        let mut cancel_edit = false;
        let active_task_id = self.data.pomodoro.active_task_id.clone();

        let pending_indices: Vec<usize> = self
            .data
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.done)
            .map(|(i, _)| i)
            .collect();
        let completed_indices: Vec<usize> = self
            .data
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.done)
            .map(|(i, _)| i)
            .collect();

        egui::ScrollArea::vertical().show(ui, |ui| {
            if self.data.tasks.is_empty() {
                ui.label(RichText::new("No hay tareas todavía.").color(Color32::GRAY));
                return;
            }

            ui.label(
                RichText::new(format!("Pendientes ({})", pending_indices.len()))
                    .strong()
                    .size(12.5)
                    .color(Color32::from_gray(220)),
            );
            ui.add_space(4.0);
            if pending_indices.is_empty() {
                ui.label(RichText::new("No hay tareas pendientes.").color(Color32::GRAY));
            }
            for idx in pending_indices {
                let task = &mut self.data.tasks[idx];
                let is_focused = active_task_id.as_deref() == Some(task.id.as_str())
                    && self.data.pomodoro.phase != PomodoroPhase::Idle;
                let is_editing = self.editing_task_id.as_deref() == Some(task.id.as_str());

                egui::Frame::none()
                    .fill(if is_focused {
                        Color32::from_rgb(30, 41, 68)
                    } else {
                        Color32::from_rgb(24, 31, 48)
                    })
                    .rounding(0.0)
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
                            if is_editing {
                                let edit_resp = ui.add(
                                    egui::TextEdit::singleline(&mut self.editing_task_title)
                                        .desired_width(180.0),
                                );
                                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    rename_commit =
                                        Some((task.id.clone(), self.editing_task_title.clone()));
                                } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                    cancel_edit = true;
                                } else if edit_resp.lost_focus()
                                    && !self.editing_task_title.trim().is_empty()
                                {
                                    // Keep editing until Enter/Escape or explicit button.
                                }
                                if ui.small_button("Guardar").clicked() {
                                    rename_commit =
                                        Some((task.id.clone(), self.editing_task_title.clone()));
                                }
                                if ui.small_button("Cancelar").clicked() {
                                    cancel_edit = true;
                                }
                            } else {
                                let label_txt = if is_focused {
                                    format!("🎯 {}", task.title)
                                } else {
                                    task.title.clone()
                                };
                                ui.label(RichText::new(label_txt).color(Color32::WHITE));
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if pixel_button(
                                            ui,
                                            PixelIcon::Clear,
                                            "",
                                            Color32::from_rgb(127, 29, 29),
                                            Color32::WHITE,
                                            "Eliminar tarea",
                                        )
                                        .clicked()
                                        {
                                            remove_idx = Some(idx);
                                        }
                                        if ui
                                            .small_button("Editar")
                                            .on_hover_text("Renombrar tarea")
                                            .clicked()
                                        {
                                            self.editing_task_id = Some(task.id.clone());
                                            self.editing_task_title = task.title.clone();
                                        }
                                        if pixel_button(
                                            ui,
                                            PixelIcon::Play,
                                            "Enfocarme",
                                            Color32::from_rgb(30, 64, 175),
                                            Color32::WHITE,
                                            "Iniciar Pomodoro con esta tarea",
                                        )
                                        .clicked()
                                        {
                                            focus_task_id = Some(task.id.clone());
                                        }
                                    },
                                );
                            }
                        });
                    });
                ui.add_space(4.0);
            }

            if !completed_indices.is_empty() {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("Completadas ({})", completed_indices.len()))
                            .strong()
                            .size(12.5)
                            .color(Color32::from_gray(160)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.confirm_clear_completed_tasks {
                            if ui.small_button("Cancelar").clicked() {
                                self.confirm_clear_completed_tasks = false;
                            }
                            if ui
                                .small_button(
                                    RichText::new("Confirmar")
                                        .color(Color32::from_rgb(248, 113, 113)),
                                )
                                .clicked()
                            {
                                let r = self.data.clear_completed_tasks();
                                self.report_persistence_result(r.map(|_| ()));
                                self.confirm_clear_completed_tasks = false;
                            }
                        } else if ui.small_button("Limpiar completadas").clicked() {
                            self.confirm_clear_completed_tasks = true;
                        }
                    });
                });
                ui.add_space(4.0);

                for idx in completed_indices {
                    let task = &mut self.data.tasks[idx];
                    egui::Frame::none()
                        .fill(Color32::from_rgb(18, 23, 36))
                        .rounding(0.0)
                        .inner_margin(8.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                if ui.checkbox(&mut task.done, "").changed() {
                                    sfx::play(SfxEvent::Click, self.config.sfx_enabled);
                                    toggled_task = Some(task.clone());
                                }
                                ui.label(
                                    RichText::new(&task.title)
                                        .strikethrough()
                                        .color(Color32::GRAY),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if pixel_button(
                                            ui,
                                            PixelIcon::Clear,
                                            "",
                                            Color32::from_rgb(90, 25, 25),
                                            Color32::WHITE,
                                            "Eliminar tarea",
                                        )
                                        .clicked()
                                        {
                                            remove_idx = Some(idx);
                                        }
                                    },
                                );
                            });
                        });
                    ui.add_space(4.0);
                }
            }
        });

        if cancel_edit {
            self.editing_task_id = None;
            self.editing_task_title.clear();
        }
        if let Some((tid, new_title)) = rename_commit {
            match self.data.rename_task(&tid, &new_title) {
                Ok(true) => {
                    self.editing_task_id = None;
                    self.editing_task_title.clear();
                }
                Ok(false) => {
                    self.status_banner =
                        Some("⚠️ El título de la tarea no puede estar vacío.".to_string());
                }
                Err(e) => self.report_persistence_result::<()>(Err(e)),
            }
        }
        if let Some(tid) = focus_task_id {
            match self.data.focus_on_task(&tid, false) {
                FocusStartResult::Started => {
                    self.mood = AssistantMood::Focused;
                    self.emit_pet_event(PetEvent::PomodoroStarted);
                    sfx::play(SfxEvent::PomodoroStart, self.config.sfx_enabled);
                }
                FocusStartResult::ConfirmSwitch { .. } => {
                    self.pending_focus_switch_task_id = Some(tid);
                }
            }
        }
        if let Some(t) = toggled_task {
            if t.done {
                self.emit_pet_event(PetEvent::TaskCompleted);
            }
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
                .rounding(0.0)
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
                            ui.label(
                                RichText::new(format!("{}/{}", h.current_count, h.daily_goal))
                                    .color(accent),
                            );
                        });
                    });
                    ui.add_space(4.0);
                    let progress = (h.current_count as f32 / h.daily_goal as f32).clamp(0.0, 1.0);
                    ui.add(
                        egui::ProgressBar::new(progress)
                            .fill(accent)
                            .show_percentage(),
                    );
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
                self.emit_pet_event_for_tool("log_habit", &out.status);
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
            ui.add_sized(
                [36.0, 24.0],
                egui::TextEdit::singleline(&mut self.new_habit_icon),
            );
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
                && !self.new_habit_name.trim().is_empty()
            {
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
        if let Some(prompt) = self.data.pomodoro.pending_prompt {
            let task_title = self.data.active_task_display_title();
            egui::Frame::none()
                .fill(Color32::from_rgb(24, 38, 60))
                .stroke(Stroke::new(1.5_f32, accent))
                .rounding(0.0)
                .inner_margin(10.0)
                .show(ui, |ui| match prompt {
                    PomodoroPrompt::WorkFinished => {
                        ui.label(
                            RichText::new("¡Terminaste tu bloque de concentración!")
                                .strong()
                                .size(13.5)
                                .color(Color32::WHITE),
                        );
                        let question = match &task_title {
                            Some(t) => format!("¿Cómo te fue con «{t}»?"),
                            None => "¿Cómo te fue con este bloque?".to_string(),
                        };
                        ui.label(RichText::new(question).size(12.0).color(accent));
                        ui.add_space(6.0);
                        if self.confirm_skip_break_continue {
                            ui.label(
                                RichText::new("¿Omitir el descanso e iniciar otro bloque ahora?")
                                    .size(11.5)
                                    .color(Color32::from_rgb(250, 204, 21)),
                            );
                            ui.horizontal(|ui| {
                                if ui.button("Sí, iniciar otro bloque").clicked() {
                                    self.confirm_skip_break_continue = false;
                                    self.data.continue_focused_task();
                                    self.mood = AssistantMood::Focused;
                                    self.emit_pet_event(PetEvent::PomodoroStarted);
                                    sfx::play(SfxEvent::PomodoroStart, self.config.sfx_enabled);
                                }
                                if ui.button("Cancelar").clicked() {
                                    self.confirm_skip_break_continue = false;
                                }
                            });
                        } else {
                            ui.horizontal_wrapped(|ui| {
                                if task_title.is_some() && ui.button("✅ Completada").clicked() {
                                    self.data.complete_focused_task();
                                    self.mood = AssistantMood::Happy;
                                    self.emit_pet_event(PetEvent::TaskCompleted);
                                    sfx::play(SfxEvent::TaskCompleted, self.config.sfx_enabled);
                                }
                                if ui.button("🔄 Necesito otro bloque").clicked() {
                                    if self.data.pomodoro.break_minutes > 0 {
                                        self.confirm_skip_break_continue = true;
                                    } else {
                                        self.data.continue_focused_task();
                                        self.mood = AssistantMood::Focused;
                                        self.emit_pet_event(PetEvent::PomodoroStarted);
                                        sfx::play(SfxEvent::PomodoroStart, self.config.sfx_enabled);
                                    }
                                }
                                if ui.button("☕ Descansar").clicked() {
                                    self.confirm_skip_break_continue = false;
                                    self.data.start_break_and_persist();
                                    self.mood = AssistantMood::Idle;
                                    sfx::play(SfxEvent::Click, self.config.sfx_enabled);
                                }
                            });
                        }
                    }
                    PomodoroPrompt::BreakFinished => {
                        ui.label(
                            RichText::new("Descanso terminado. ¿Seguimos?")
                                .strong()
                                .size(13.5)
                                .color(Color32::WHITE),
                        );
                        if let Some(t) = &task_title {
                            ui.label(
                                RichText::new(format!("Tarea: {t}"))
                                    .size(12.0)
                                    .color(accent),
                            );
                        }
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if ui.button("▶ Iniciar otro bloque").clicked() {
                                self.data.continue_focused_task();
                                self.mood = AssistantMood::Focused;
                                self.emit_pet_event(PetEvent::PomodoroStarted);
                                sfx::play(SfxEvent::PomodoroStart, self.config.sfx_enabled);
                            }
                            if ui.button("Terminar").clicked() {
                                self.data.dismiss_pomodoro_prompt(true);
                                self.mood = AssistantMood::Idle;
                            }
                        });
                    }
                });
            ui.add_space(8.0);
        }

        ui.vertical_centered(|ui| {
            ui.add_space(6.0);
            let phase_lbl = match self.data.pomodoro.phase {
                PomodoroPhase::Idle => "Listo para enfocar",
                PomodoroPhase::Work => "Enfoque activo",
                PomodoroPhase::Break => "Descanso",
            };
            ui.label(
                RichText::new(phase_lbl)
                    .size(15.0)
                    .color(Color32::LIGHT_GRAY),
            );
            if let Some(task_title) = self.data.active_task_display_title() {
                ui.label(
                    RichText::new(format!("🎯 {task_title}"))
                        .strong()
                        .size(13.0)
                        .color(accent),
                );
            }
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
                } else if self.data.pomodoro.phase != PomodoroPhase::Idle {
                    (PixelIcon::Play, "Reanudar")
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
                    self.emit_pet_event_for_tool("pause_pomodoro", &out.status);
                    if out.status == ToolStatus::Error {
                        self.status_banner = Some(format!("⚠️ {}", out.ui_message));
                    } else if self.data.pomodoro.running
                        && self.data.pomodoro.phase == PomodoroPhase::Work
                    {
                        self.mood = AssistantMood::Focused;
                        sfx::play(SfxEvent::PomodoroStart, self.config.sfx_enabled);
                    } else {
                        self.mood = AssistantMood::Idle;
                        sfx::play(SfxEvent::Click, self.config.sfx_enabled);
                    }
                }
                if self.data.pomodoro.phase == PomodoroPhase::Break {
                    if pixel_button(
                        ui,
                        PixelIcon::Clear,
                        "Terminar descanso",
                        Color32::from_rgb(51, 65, 85),
                        Color32::WHITE,
                        "Finalizar descanso anticipadamente",
                    )
                    .clicked()
                    {
                        self.data.reset_pomodoro_and_persist();
                        self.mood = AssistantMood::Idle;
                        self.emit_pet_event(PetEvent::PomodoroPaused);
                    }
                } else if pixel_button(
                    ui,
                    PixelIcon::Reset,
                    "Reiniciar",
                    Color32::from_rgb(51, 65, 85),
                    Color32::WHITE,
                    "Reiniciar temporizador",
                )
                .clicked()
                {
                    self.data.reset_pomodoro_and_persist();
                    self.mood = AssistantMood::Idle;
                }
            });
            ui.add_space(12.0);
            let (today_count, today_mins) = self.data.today_focus_summary();
            ui.label(RichText::new(format!("Hoy: {today_count} sesiones completadas")).strong());
            if today_mins > 0 {
                ui.label(
                    RichText::new(format!("{today_mins} minutos de concentración"))
                        .size(11.5)
                        .color(Color32::GRAY),
                );
            }
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

    fn spawn_m3u_picker(&self, ctx: &egui::Context) {
        let tx = self.bg_tx.clone();
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            match music::pick_m3u_file_dialog() {
                Ok(path) => {
                    let _ = tx.send(BgEvent::M3uPicked(path));
                }
                Err(err) => {
                    let _ = tx.send(BgEvent::M3uPickError(err));
                }
            }
            ctx_clone.request_repaint();
        });
    }

    fn import_m3u_from_path(&mut self, path: &std::path::Path) {
        match music::parse_m3u_file(path) {
            Ok((name, parsed_tracks)) => {
                let new_tracks: Vec<NewMusicTrack> = parsed_tracks
                    .into_iter()
                    .map(|t| NewMusicTrack {
                        title: t.title,
                        uri: t.uri,
                        duration_secs: t.duration_secs,
                    })
                    .collect();
                let source_str = path.to_string_lossy().to_string();
                match self
                    .data
                    .import_m3u_playlist(&name, Some(&source_str), &new_tracks)
                {
                    Ok(saved) => {
                        let count = saved.track_count;
                        let id = saved.id.clone();
                        self.select_music_playlist(&id);
                        self.music_status = Some(format!(
                            "Playlist '{}' guardada ({count} pistas).",
                            saved.name
                        ));
                        self.music_m3u_path_input.clear();
                    }
                    Err(e) => {
                        self.music_status = Some(format!("⚠️ Error guardando playlist: {e}"));
                    }
                }
            }
            Err(err) => {
                self.music_status = Some(format!("⚠️ {err}"));
            }
        }
    }

    fn add_music_url_from_input(&mut self, raw_url: &str, raw_title: Option<&str>) {
        match self.data.add_music_url(
            self.music_selected_playlist_id.as_deref(),
            raw_title,
            raw_url,
        ) {
            Ok((playlist, track)) => {
                if self.music_selected_playlist_id.as_deref() == Some(playlist.id.as_str()) {
                    self.music_tracks.push(track.clone());
                } else {
                    self.select_music_playlist(&playlist.id);
                }
                self.music_status = Some(format!(
                    "URL guardada en '{}': {}",
                    playlist.name, track.title
                ));
                self.music_url_input.clear();
                self.music_url_title_input.clear();
            }
            Err(e) => {
                self.music_status = Some(format!("⚠️ {e}"));
            }
        }
    }

    fn select_music_playlist(&mut self, playlist_id: &str) {
        self.music_selected_playlist_id = Some(playlist_id.to_string());
        self.music_current_track_idx = None;
        match self.data.load_playlist_tracks(playlist_id) {
            Ok(tracks) => {
                self.music_tracks = tracks;
            }
            Err(e) => {
                self.music_tracks.clear();
                self.music_status = Some(format!("⚠️ Error cargando pistas: {e}"));
            }
        }
    }

    fn play_saved_music_by_query(&mut self, query: Option<&str>) -> String {
        let Some(raw_q) = query.map(str::trim).filter(|q| !q.is_empty()) else {
            if self.music_tracks.is_empty() {
                let Some(first_id) = self.data.playlists.first().map(|p| p.id.clone()) else {
                    return "⚠️ No hay pistas guardadas. Usá `/musica <url> [nombre]` para guardar una.".to_string();
                };
                self.select_music_playlist(&first_id);
            }
            if self.music_tracks.is_empty() {
                return "⚠️ No hay pistas para reproducir.".to_string();
            }
            let idx = self.music_current_track_idx.unwrap_or(0);
            self.play_music_track_at(idx);
            return self
                .music_status
                .clone()
                .unwrap_or_else(|| "▶ Reproduciendo".to_string());
        };

        if let Ok(num) = raw_q.parse::<usize>() {
            if num >= 1 && num <= self.music_tracks.len() {
                self.play_music_track_at(num - 1);
                return self
                    .music_status
                    .clone()
                    .unwrap_or_else(|| "▶ Reproduciendo".to_string());
            }
        }

        let needle = raw_q.to_lowercase();
        if let Some(idx) = self.music_tracks.iter().position(|t| {
            t.title.to_lowercase().contains(&needle) || t.uri.to_lowercase().contains(&needle)
        }) {
            self.play_music_track_at(idx);
            return self
                .music_status
                .clone()
                .unwrap_or_else(|| "▶ Reproduciendo".to_string());
        }

        let playlist_ids: Vec<String> = self.data.playlists.iter().map(|p| p.id.clone()).collect();
        for pid in playlist_ids {
            if let Ok(tracks) = self.data.load_playlist_tracks(&pid) {
                if let Some(idx) = tracks.iter().position(|t| {
                    t.title.to_lowercase().contains(&needle)
                        || t.uri.to_lowercase().contains(&needle)
                }) {
                    self.select_music_playlist(&pid);
                    self.play_music_track_at(idx);
                    return self
                        .music_status
                        .clone()
                        .unwrap_or_else(|| "▶ Reproduciendo".to_string());
                }
            }
        }

        format!("⚠️ No encontré ninguna pista guardada que coincida con '{raw_q}'.")
    }

    fn sync_audio_volume(&mut self, vol: u8, save_now: bool) {
        self.config.audio_volume = vol.min(100);
        self.music_player.set_volume(self.config.audio_volume);
        if save_now {
            let _ = self.config.save();
        }
    }

    fn toggle_audio_mute(&mut self) {
        let next = self.music_player.toggle_mute();
        self.config.audio_muted = next;
        let _ = self.config.save();
    }

    fn render_volume_controls(&mut self, ui: &mut egui::Ui, slider_width: f32, show_pct: bool) {
        let muted = self.config.audio_muted;
        let mute_icon = if muted {
            PixelIcon::VolumeMuted
        } else {
            PixelIcon::VolumeOn
        };
        let mute_bg = if muted {
            Color32::from_rgb(153, 27, 27)
        } else {
            Color32::from_rgb(30, 41, 59)
        };
        if pixel_button(
            ui,
            mute_icon,
            "",
            mute_bg,
            Color32::WHITE,
            if muted { "Activar sonido" } else { "Silenciar" },
        )
        .clicked()
        {
            self.toggle_audio_mute();
        }

        let mut vol = self.config.audio_volume;
        ui.scope(|ui| {
            ui.spacing_mut().slider_width = slider_width;
            let resp = ui
                .add(egui::Slider::new(&mut vol, 0..=100).show_value(false))
                .on_hover_text(format!("Volumen: {}%", self.config.audio_volume));
            if resp.changed() {
                self.sync_audio_volume(vol, false);
            }
            if resp.drag_stopped() || resp.lost_focus() {
                let _ = self.config.save();
            }
        });

        if show_pct {
            let pct_text = if muted {
                format!("0% ({}%)", self.config.audio_volume)
            } else {
                format!("{}%", self.config.audio_volume)
            };
            ui.label(RichText::new(pct_text).size(11.0).color(if muted {
                Color32::from_gray(140)
            } else {
                Color32::from_gray(220)
            }));
        }
    }

    fn play_music_track_at(&mut self, idx: usize) {
        let Some(track) = self.music_tracks.get(idx).cloned() else {
            return;
        };
        self.music_player.set_volume(self.config.audio_volume);
        self.music_player.set_muted(self.config.audio_muted);
        match self.music_player.play(&track.uri) {
            Ok(()) => {
                self.music_current_track_idx = Some(idx);
                self.music_status = Some(format!("▶ Reproduciendo: {}", track.title));
            }
            Err(e) => {
                self.music_status = Some(format!("⚠️ {e}"));
            }
        }
    }

    fn play_next_music_track(&mut self) {
        if self.music_tracks.is_empty() {
            self.music_current_track_idx = None;
            return;
        }
        let next = match self.music_current_track_idx {
            Some(i) if i + 1 < self.music_tracks.len() => i + 1,
            _ => {
                self.music_current_track_idx = None;
                self.music_status = Some("Playlist finalizada.".to_string());
                return;
            }
        };
        self.play_music_track_at(next);
    }

    fn play_prev_music_track(&mut self) {
        if self.music_tracks.is_empty() {
            self.music_current_track_idx = None;
            return;
        }
        let prev = match self.music_current_track_idx {
            Some(i) if i > 0 => i - 1,
            _ => self.music_tracks.len() - 1,
        };
        self.play_music_track_at(prev);
    }

    fn play_next_music_track_manual(&mut self) {
        if self.music_tracks.is_empty() {
            self.music_current_track_idx = None;
            return;
        }
        let next = match self.music_current_track_idx {
            Some(i) if i + 1 < self.music_tracks.len() => i + 1,
            _ => 0,
        };
        self.play_music_track_at(next);
    }

    fn stop_music(&mut self) {
        self.music_player.stop();
        self.music_status = Some("Reproducción detenida.".to_string());
    }

    fn current_music_title(&self) -> String {
        self.music_current_track_idx
            .and_then(|idx| self.music_tracks.get(idx))
            .map(|t| t.title.clone())
            .unwrap_or_else(|| "Música en reproducción".to_string())
    }

    fn render_music_mini_controls(
        &mut self,
        ui: &mut egui::Ui,
        accent: Color32,
        compact_island: bool,
    ) {
        let title = self.current_music_title();
        let has_multiple = self.music_tracks.len() > 1;
        let is_paused = self.music_player.is_paused();

        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                let (icon_rect, _) =
                    ui.allocate_exact_size(Vec2::splat(13.0), egui::Sense::hover());
                paint_pixel_icon(ui.painter(), icon_rect, PixelIcon::MusicNote, accent);

                let controls_w = if has_multiple { 122.0 } else { 62.0 }
                    + if compact_island { 0.0 } else { 140.0 };
                let title_w = (ui.available_width() - controls_w).max(60.0);

                ui.allocate_ui_with_layout(
                    Vec2::new(title_w, 22.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        let lbl = ui.add(
                            egui::Label::new(
                                RichText::new(&title).size(11.5).color(Color32::WHITE),
                            )
                            .truncate()
                            .sense(if compact_island {
                                egui::Sense::hover()
                            } else {
                                egui::Sense::click()
                            }),
                        );
                        if !compact_island && lbl.clicked() {
                            self.active_tab = ActiveTab::Music;
                        }
                    },
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if has_multiple
                        && pixel_button(
                            ui,
                            PixelIcon::NextTrack,
                            "",
                            Color32::from_rgb(30, 41, 59),
                            Color32::WHITE,
                            "Siguiente pista",
                        )
                        .clicked()
                    {
                        self.play_next_music_track_manual();
                    }

                    if pixel_button(
                        ui,
                        PixelIcon::Stop,
                        "",
                        Color32::from_rgb(153, 27, 27),
                        Color32::WHITE,
                        "Detener música",
                    )
                    .clicked()
                    {
                        self.stop_music();
                    }

                    if pixel_button(
                        ui,
                        if is_paused {
                            PixelIcon::Play
                        } else {
                            PixelIcon::Pause
                        },
                        "",
                        Color32::from_rgb(30, 41, 59),
                        Color32::WHITE,
                        if is_paused { "Reanudar" } else { "Pausar" },
                    )
                    .clicked()
                    {
                        self.music_player.toggle_pause();
                    }

                    if has_multiple
                        && pixel_button(
                            ui,
                            PixelIcon::PrevTrack,
                            "",
                            Color32::from_rgb(30, 41, 59),
                            Color32::WHITE,
                            "Pista anterior",
                        )
                        .clicked()
                    {
                        self.play_prev_music_track();
                    }

                    if !compact_island {
                        self.render_volume_controls(ui, 68.0, true);
                    }
                });
            });

            if compact_island {
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    self.render_volume_controls(ui, 185.0, true);
                });
            }
        });
    }

    fn render_music_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, accent: Color32) {
        if let Some(sel_id) = self.music_selected_playlist_id.clone() {
            if self
                .data
                .playlists
                .iter()
                .find(|p| p.id == sel_id)
                .is_some_and(|p| p.track_count != self.music_tracks.len())
            {
                if let Ok(tracks) = self.data.load_playlist_tracks(&sel_id) {
                    self.music_tracks = tracks;
                }
            }
        }
        ui.label(RichText::new("Agregar playlist (.m3u / .m3u8)").strong());
        ui.horizontal(|ui| {
            let input_resp = ui.add_sized(
                [ui.available_width() - 185.0, 26.0],
                egui::TextEdit::singleline(&mut self.music_m3u_path_input)
                    .hint_text("Ruta al archivo .m3u..."),
            );
            let enter = input_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (pixel_button(
                ui,
                PixelIcon::Save,
                "Agregar",
                accent,
                Color32::from_rgb(15, 20, 32),
                "Importar archivo .m3u a SQLite",
            )
            .clicked()
                || enter)
                && !self.music_m3u_path_input.trim().is_empty()
            {
                let trimmed = self.music_m3u_path_input.trim().to_string();
                if music::is_remote_or_uri(&trimmed) {
                    self.add_music_url_from_input(&trimmed, None);
                    self.music_m3u_path_input.clear();
                } else {
                    let p = PathBuf::from(trimmed);
                    self.import_m3u_from_path(&p);
                }
            }
            if pixel_button(
                ui,
                PixelIcon::Attach,
                "Elegir .m3u",
                Color32::from_rgb(30, 41, 59),
                Color32::WHITE,
                "Buscar archivo .m3u en el sistema",
            )
            .clicked()
            {
                self.spawn_m3u_picker(ctx);
            }
        });

        ui.add_space(6.0);
        ui.label(RichText::new("Agregar URL (radio / stream / audio)").strong());
        ui.horizontal(|ui| {
            let url_resp = ui.add_sized(
                [ui.available_width() * 0.56, 26.0],
                egui::TextEdit::singleline(&mut self.music_url_input)
                    .hint_text("https://... (stream o audio)"),
            );
            let title_resp = ui.add_sized(
                [(ui.available_width() - 108.0).max(70.0), 26.0],
                egui::TextEdit::singleline(&mut self.music_url_title_input)
                    .hint_text("Nombre (opcional)"),
            );
            let enter = (url_resp.lost_focus() || title_resp.lost_focus())
                && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (pixel_button(
                ui,
                PixelIcon::Save,
                "Guardar URL",
                accent,
                Color32::from_rgb(15, 20, 32),
                "Guardar URL en SQLite",
            )
            .clicked()
                || enter)
                && !self.music_url_input.trim().is_empty()
            {
                let url = self.music_url_input.clone();
                let title = self.music_url_title_input.clone();
                self.add_music_url_from_input(&url, Some(&title));
            }
        });

        ui.add_space(6.0);
        // Minimum playback controls: Play, Stop, Volume
        let is_active = self.music_player.is_active();
        let is_paused = self.music_player.is_paused();
        egui::Frame::none()
            .fill(Color32::from_rgb(20, 27, 45))
            .rounding(0.0)
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if !is_active {
                        if pixel_button(
                            ui,
                            PixelIcon::Play,
                            "Play",
                            Color32::from_rgb(22, 163, 74),
                            Color32::WHITE,
                            "Reproducir pista seleccionada o primera",
                        )
                        .clicked()
                        {
                            let idx = self.music_current_track_idx.unwrap_or(0);
                            self.play_music_track_at(idx);
                        }
                    } else if pixel_button(
                        ui,
                        if is_paused {
                            PixelIcon::Play
                        } else {
                            PixelIcon::Pause
                        },
                        if is_paused { "Seguir" } else { "Pausa" },
                        Color32::from_rgb(30, 64, 175),
                        Color32::WHITE,
                        if is_paused {
                            "Reanudar reproducción"
                        } else {
                            "Pausar reproducción"
                        },
                    )
                    .clicked()
                    {
                        self.music_player.toggle_pause();
                    }
                    if pixel_button(
                        ui,
                        PixelIcon::Stop,
                        "Stop",
                        Color32::from_rgb(153, 27, 27),
                        Color32::WHITE,
                        "Detener reproducción",
                    )
                    .clicked()
                    {
                        self.stop_music();
                    }

                    ui.add_space(6.0);
                    self.render_volume_controls(ui, 100.0, true);
                });

                let now_playing = match (is_active, is_paused, self.music_current_track_idx) {
                    (true, true, Some(idx)) => self
                        .music_tracks
                        .get(idx)
                        .map(|t| format!("⏸ {} (en pausa)", t.title))
                        .unwrap_or_else(|| "⏸ En pausa".to_string()),
                    (true, false, Some(idx)) => self
                        .music_tracks
                        .get(idx)
                        .map(|t| format!("▶ {}", t.title))
                        .unwrap_or_else(|| "▶ Reproduciendo".to_string()),
                    _ => self
                        .music_status
                        .clone()
                        .unwrap_or_else(|| "Sin reproducción activa.".to_string()),
                };
                ui.add_space(4.0);
                ui.label(RichText::new(now_playing).size(11.5).color(accent));
            });

        ui.add_space(6.0);
        ui.separator();

        // Saved playlists in SQLite
        ui.label(RichText::new("Playlists guardadas (SQLite)").strong());
        let mut selected_to_load: Option<String> = None;
        let mut playlist_to_delete: Option<String> = None;

        if self.data.playlists.is_empty() {
            ui.label(
                RichText::new("Todavía no agregaste archivos .m3u.")
                    .size(11.5)
                    .color(Color32::GRAY),
            );
        } else {
            egui::ScrollArea::vertical()
                .id_salt("music_playlists_scroll")
                .max_height(110.0)
                .show(ui, |ui| {
                    for pl in &self.data.playlists {
                        let is_selected = self
                            .music_selected_playlist_id
                            .as_deref()
                            .is_some_and(|id| id == pl.id);
                        ui.horizontal(|ui| {
                            let label = format!("{} ({} pistas)", pl.name, pl.track_count);
                            if ui.selectable_label(is_selected, label).clicked() {
                                selected_to_load = Some(pl.id.clone());
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if pixel_button(
                                        ui,
                                        PixelIcon::Clear,
                                        "",
                                        Color32::from_rgb(127, 29, 29),
                                        Color32::WHITE,
                                        "Eliminar playlist",
                                    )
                                    .clicked()
                                    {
                                        playlist_to_delete = Some(pl.id.clone());
                                    }
                                },
                            );
                        });
                    }
                });
        }

        if let Some(id) = selected_to_load {
            self.select_music_playlist(&id);
        }
        if let Some(id) = playlist_to_delete {
            if self.music_selected_playlist_id.as_deref() == Some(id.as_str()) {
                self.music_player.stop();
                self.music_selected_playlist_id = None;
                self.music_tracks.clear();
                self.music_current_track_idx = None;
            }
            let r = self.data.delete_playlist(&id);
            self.report_persistence_result(r);
        }

        ui.add_space(4.0);
        ui.separator();
        ui.label(RichText::new("Pistas").strong());

        let mut clicked_track: Option<usize> = None;
        let mut track_to_delete: Option<(usize, String, String)> = None;
        egui::ScrollArea::vertical()
            .id_salt("music_tracks_scroll")
            .show(ui, |ui| {
                if self.music_tracks.is_empty() {
                    ui.label(
                        RichText::new("Seleccioná una playlist para ver sus pistas.")
                            .size(11.5)
                            .color(Color32::GRAY),
                    );
                } else {
                    for (idx, track) in self.music_tracks.iter().enumerate() {
                        let active = self.music_current_track_idx == Some(idx);
                        let label = match track.duration_secs {
                            Some(d) => {
                                format!("{}. {} ({}:{:02})", idx + 1, track.title, d / 60, d % 60)
                            }
                            None => format!("{}. {}", idx + 1, track.title),
                        };
                        ui.horizontal(|ui| {
                            if ui.selectable_label(active, label).clicked() {
                                clicked_track = Some(idx);
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if pixel_button(
                                        ui,
                                        PixelIcon::Clear,
                                        "",
                                        Color32::from_rgb(127, 29, 29),
                                        Color32::WHITE,
                                        "Eliminar pista",
                                    )
                                    .clicked()
                                    {
                                        track_to_delete = Some((
                                            idx,
                                            track.playlist_id.clone(),
                                            track.id.clone(),
                                        ));
                                    }
                                },
                            );
                        });
                    }
                }
            });

        if let Some((idx, playlist_id, track_id)) = track_to_delete {
            if self.music_current_track_idx == Some(idx) {
                self.music_player.stop();
                self.music_current_track_idx = None;
            } else if let Some(cur) = self.music_current_track_idx {
                if cur > idx {
                    self.music_current_track_idx = Some(cur - 1);
                }
            }
            self.music_tracks.remove(idx);
            let r = self.data.delete_music_track(&playlist_id, &track_id);
            self.report_persistence_result(r);
        }

        if let Some(idx) = clicked_track {
            self.play_music_track_at(idx);
        }
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
                    format!(
                        "Vista previa: {} archivo(s) para organizar.",
                        self.organizer.last_preview.len()
                    )
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
                        .rounding(0.0)
                        .inner_margin(6.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("[{}]", plan.category)).color(accent),
                                );
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
            .rounding(0.0)
            .inner_margin(14.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    let title = if is_hovering_file {
                        "Soltá el archivo acá para convertirlo"
                    } else {
                        "Arrastrá y soltá un archivo acá"
                    };
                    ui.label(
                        RichText::new(title)
                            .strong()
                            .size(14.0)
                            .color(if is_hovering_file {
                                accent
                            } else {
                                Color32::WHITE
                            }),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(
                            "PDF, DOCX, XLSX, PPTX, HTML, CSV, XML, RSS — 100% local sin LLM",
                        )
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
                    self.status_banner =
                        Some("Markdown normal copiado al portapapeles.".to_string());
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
        ui.label(
            RichText::new("Texto original:")
                .size(11.5)
                .color(Color32::from_gray(180)),
        );
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
            if !compacted.is_empty()
                && pixel_button(
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
        ui.label(
            RichText::new("Resultado compactado:")
                .size(11.5)
                .color(Color32::from_gray(180)),
        );
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

    fn render_settings_tab(&mut self, ui: &mut egui::Ui, accent: Color32) {
        egui::Frame::none()
            .fill(Color32::from_rgb(20, 27, 45))
            .rounding(0.0)
            .inner_margin(4.0)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (tab, label) in [
                        (SettingsSubTab::Ai, "IA y Audio"),
                        (SettingsSubTab::ExternalTools, "Herramientas"),
                        (SettingsSubTab::Position, "Posición e Isla"),
                        (SettingsSubTab::Pet, "Mascota"),
                    ] {
                        let selected = self.settings_sub_tab == tab;
                        let fill = if selected {
                            accent
                        } else {
                            Color32::from_rgb(30, 41, 59)
                        };
                        let text_color = if selected {
                            Color32::from_rgb(15, 20, 32)
                        } else {
                            Color32::from_gray(220)
                        };
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(label).size(12.0).color(text_color),
                                )
                                .fill(fill)
                                .rounding(0.0),
                            )
                            .clicked()
                        {
                            self.settings_sub_tab = tab;
                        }
                    }
                });
            });

        ui.add_space(8.0);

        let save_bar_h = 38.0;
        let scroll_h = (ui.available_height() - save_bar_h).max(120.0);
        egui::ScrollArea::vertical()
            .id_salt("settings_scroll")
            .max_height(scroll_h)
            .auto_shrink([false, false])
            .show(ui, |ui| match self.settings_sub_tab {
                SettingsSubTab::Ai => self.render_settings_ai_section(ui),
                SettingsSubTab::ExternalTools => {
                    self.render_settings_external_tools_section(ui, accent)
                }
                SettingsSubTab::Position => self.render_settings_position_section(ui),
                SettingsSubTab::Pet => self.render_settings_pet_section(ui),
            });

        ui.add_space(8.0);
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

    fn render_settings_external_tools_section(&mut self, ui: &mut egui::Ui, accent: Color32) {
        let ext = &self.config.external_tools;
        let (tav_status, tav_used, tav_budget) = self
            .free_tools
            .provider_status(ext, SearchProviderId::Tavily);
        let (brv_status, brv_used, brv_budget) = self
            .free_tools
            .provider_status(ext, SearchProviderId::Brave);

        ui.label(RichText::new("Herramientas externas (Free Tools Engine)").strong());
        ui.add_space(4.0);

        egui::Grid::new("external_tools_status_grid")
            .striped(true)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                ui.label(RichText::new("Herramienta").strong().size(11.5));
                ui.label(RichText::new("Proveedor").strong().size(11.5));
                ui.label(RichText::new("Estado").strong().size(11.5));
                ui.end_row();

                let status_color = |st: ProviderHealthStatus| match st {
                    ProviderHealthStatus::Available => Color32::from_rgb(52, 211, 153),
                    ProviderHealthStatus::NotConfigured | ProviderHealthStatus::Disabled => {
                        Color32::GRAY
                    }
                    ProviderHealthStatus::QuotaExhausted
                    | ProviderHealthStatus::TemporaryError
                    | ProviderHealthStatus::NeedsVerification => Color32::from_rgb(250, 204, 21),
                };

                ui.label("Búsqueda web");
                ui.label("Tavily");
                ui.label(RichText::new(tav_status.label()).color(status_color(tav_status)));
                ui.end_row();

                ui.label("Búsqueda alternativa");
                ui.label("Brave");
                ui.label(RichText::new(brv_status.label()).color(status_color(brv_status)));
                ui.end_row();

                ui.label("Lectura web");
                ui.label("Local");
                ui.label(RichText::new(if ext.web_fetch_enabled {
                    "Disponible"
                } else {
                    "Deshabilitado"
                }));
                ui.end_row();

                ui.label("Clima");
                ui.label("Open-Meteo");
                ui.label(RichText::new(if ext.weather_enabled {
                    "Disponible"
                } else {
                    "Deshabilitado"
                }));
                ui.end_row();

                ui.label("Cotizaciones");
                ui.label("Frankfurter");
                ui.label(RichText::new(if ext.exchange_enabled {
                    "Disponible"
                } else {
                    "Deshabilitado"
                }));
                ui.end_row();
            });

        ui.add_space(6.0);
        ui.label(
            RichText::new(format!(
                "Tavily: {tav_used} / {tav_budget} créditos estimados · Brave: {brv_used} / {brv_budget} créditos estimados"
            ))
            .size(11.5)
            .color(accent),
        );

        ui.add_space(6.0);
        ui.separator();
        ui.label("Proveedor de búsqueda preferido:");
        ui.horizontal(|ui| {
            ui.radio_value(
                &mut self.config.external_tools.preferred_search_provider,
                SearchProviderId::Tavily,
                "Tavily",
            );
            ui.radio_value(
                &mut self.config.external_tools.preferred_search_provider,
                SearchProviderId::Brave,
                "Brave",
            );
        });

        ui.add_space(6.0);
        ui.label(RichText::new("Tavily").strong());
        ui.checkbox(
            &mut self.config.external_tools.tavily.enabled,
            "Habilitar Tavily",
        );
        ui.label("API Key (Tavily):");
        ui.add(
            egui::TextEdit::singleline(&mut self.config.external_tools.tavily.api_key)
                .password(true)
                .desired_width(f32::INFINITY),
        );
        ui.horizontal(|ui| {
            ui.label("Presupuesto mensual:");
            ui.add(
                egui::DragValue::new(&mut self.config.external_tools.tavily.monthly_budget)
                    .range(0..=100_000),
            );
            if ui.small_button("Probar conexión").clicked() {
                self.spawn_provider_test(SearchProviderId::Tavily);
            }
        });

        ui.add_space(6.0);
        ui.label(RichText::new("Brave Search (alternativa)").strong());
        ui.checkbox(
            &mut self.config.external_tools.brave.enabled,
            "Habilitar Brave",
        );
        ui.label("API Key (Brave):");
        ui.add(
            egui::TextEdit::singleline(&mut self.config.external_tools.brave.api_key)
                .password(true)
                .desired_width(f32::INFINITY),
        );
        ui.horizontal(|ui| {
            ui.label("Presupuesto mensual:");
            ui.add(
                egui::DragValue::new(&mut self.config.external_tools.brave.monthly_budget)
                    .range(0..=100_000),
            );
            if ui.small_button("Probar conexión").clicked() {
                self.spawn_provider_test(SearchProviderId::Brave);
            }
        });
        ui.checkbox(
            &mut self.config.external_tools.brave.allow_paid_fallback,
            "Permitir usar Brave como respaldo automático (solo si no genera cargos)",
        );

        if let Some(status) = &self.provider_test_status {
            ui.add_space(4.0);
            ui.label(RichText::new(status).size(11.5).color(accent));
        }

        ui.add_space(6.0);
        ui.separator();
        ui.checkbox(
            &mut self.config.external_tools.web_fetch_enabled,
            "Habilitar lectura web (web_fetch)",
        );
        ui.checkbox(
            &mut self.config.external_tools.weather_enabled,
            "Habilitar clima (Open-Meteo)",
        );
        ui.checkbox(
            &mut self.config.external_tools.exchange_enabled,
            "Habilitar cotizaciones (Frankfurter)",
        );
    }

    fn spawn_provider_test(&mut self, pid: SearchProviderId) {
        self.provider_test_status = Some(format!("Probando {}...", pid.label()));
        let free_tools = Arc::clone(&self.free_tools);
        let cfg = self.config.external_tools.clone();
        let tx = self.bg_tx.clone();
        self.rt.spawn(async move {
            let msg = match free_tools.test_search_provider(&cfg, pid).await {
                Ok(ok) => format!("✓ {ok}"),
                Err(err) => format!("⚠️ {err}"),
            };
            let _ = tx.send(BgEvent::ProviderTestResult(msg));
        });
    }

    fn render_settings_ai_section(&mut self, ui: &mut egui::Ui) {
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
        ui.checkbox(
            &mut self.config.force_mock,
            "Forzar Modo Mock Local (sin red)",
        );
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
    }

    fn render_settings_position_section(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Isla").strong());
        ui.add_space(4.0);
        ui.label(
            RichText::new("Posición:")
                .size(12.0)
                .color(Color32::from_gray(200)),
        );
        ui.horizontal_wrapped(|ui| {
            for pos in [
                IslandPosition::Top,
                IslandPosition::Bottom,
                IslandPosition::Left,
                IslandPosition::Right,
                IslandPosition::Custom,
            ] {
                if ui
                    .radio_value(&mut self.config.island_position, pos, pos.label())
                    .changed()
                {
                    self.island_sm.invalidate_geometry();
                }
            }
        });

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Margen desde el borde (px):");
            if ui
                .add(egui::Slider::new(&mut self.config.island_margin, 0.0..=48.0).step_by(2.0))
                .changed()
            {
                self.island_sm.invalidate_geometry();
            }
        });

        ui.add_space(6.0);
        ui.separator();
        ui.checkbox(&mut self.config.island_auto_hide, "Ocultar automáticamente");
        ui.checkbox(
            &mut self.config.island_remember_position,
            "Recordar posición",
        );
        if ui
            .checkbox(&mut self.config.always_on_top, "Mantener siempre visible")
            .changed()
        {
            self.always_on_top = self.config.always_on_top;
        }

        ui.add_enabled_ui(self.config.island_auto_hide, |ui| {
            ui.label(
                RichText::new("Tiempo de espera:")
                    .size(12.0)
                    .color(Color32::from_gray(200)),
            );
            ui.horizontal(|ui| {
                for (secs, label) in [(2u32, "2 segundos"), (5, "5 segundos"), (10, "10 segundos")]
                {
                    ui.radio_value(&mut self.config.island_hide_delay, secs, label);
                }
            });
            ui.checkbox(
                &mut self.config.island_show_on_hover,
                "Mostrar al acercar el mouse",
            );
            ui.checkbox(
                &mut self.config.island_keep_visible_during_playback,
                "Mantener visible durante reproducción",
            );
            ui.checkbox(
                &mut self.config.island_keep_visible_during_ai,
                "Mantener visible durante operación de IA",
            );
        });

        if self.session_kind == SessionKind::Wayland {
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "Nota Wayland: el compositor controla la posición absoluta en pantalla; la autoocultación colapsa la Isla a la pestaña interactiva.",
                )
                .size(11.0)
                .color(Color32::from_rgb(250, 204, 21)),
            );
        }
    }

    fn render_settings_pet_section(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Mascota de escritorio").strong());
        ui.checkbox(&mut self.config.pet.enabled, "Habilitar Modo Mascota");
        ui.checkbox(
            &mut self.config.pet.always_on_top,
            "Mantener mascota siempre visible",
        );
        ui.checkbox(
            &mut self.config.pet.follow_cursor,
            "Seguir el cursor con los ojos",
        );
        ui.checkbox(
            &mut self.config.pet.auto_animations,
            "Animaciones autónomas (curiosidad)",
        );
        ui.checkbox(
            &mut self.config.pet.celebrate_tasks,
            "Celebrar al completar tareas y hábitos",
        );
        ui.checkbox(
            &mut self.config.pet.reduce_animations,
            "Reducir animaciones (accesibilidad)",
        );

        ui.horizontal(|ui| {
            ui.label("Tamaño:");
            for sz in [PetSize::Small, PetSize::Normal, PetSize::Large] {
                ui.selectable_value(&mut self.config.pet.size, sz, sz.label());
            }
        });

        ui.horizontal(|ui| {
            ui.label("Intensidad:");
            for level in [
                PetIntensity::Subtle,
                PetIntensity::Normal,
                PetIntensity::Expressive,
            ] {
                ui.selectable_value(&mut self.config.pet.intensity, level, level.label());
            }
        });

        ui.horizontal(|ui| {
            ui.label("Dormir tras (min, 0 = nunca):");
            ui.add(egui::DragValue::new(&mut self.config.pet.sleep_after_mins).range(0..=120));
            if ui.small_button("Restablecer posición").clicked() {
                self.config.pet.pet_pos = None;
                self.status_banner = Some("Posición de mascota restablecida.".to_string());
            }
        });
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
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(32.0, 28.0), egui::Sense::click());
    let pressed = resp.is_pointer_button_down_on();
    let hovered = resp.hovered();
    if ui.is_rect_visible(rect) {
        let fill = if selected {
            accent
        } else if hovered {
            BUTTON_HOVER
        } else {
            BUTTON
        };
        let icon_color = if selected { NIGHT } else { IVORY };
        paint_pixel_bevel(
            ui.painter(),
            rect,
            fill,
            hovered,
            pressed,
            resp.has_focus(),
            ui.is_enabled(),
        );
        let shift = if pressed {
            Vec2::new(1.0, 1.0)
        } else {
            Vec2::ZERO
        };
        let icon_rect = egui::Rect::from_center_size(rect.center() + shift, Vec2::splat(16.0));
        paint_pixel_icon(ui.painter(), icon_rect, icon, icon_color);
    }
    if resp.clicked() {
        *active = tab;
    }
    resp.on_hover_text(RichText::new(label).size(12.0).color(IVORY));
}

impl ActiveTab {
    fn title(self) -> &'static str {
        match self {
            Self::Chat => "Chat",
            Self::Tasks => "Tareas",
            Self::Habits => "Hábitos",
            Self::Focus => "Pomodoro",
            Self::Music => "Música",
            Self::Organizer => "Organizador",
            Self::Markdown => "MarkItDown",
            Self::Compact => "Compactar",
            Self::Settings => "Configuración",
        }
    }
}

fn draw_pomodoro_ring(
    ui: &mut egui::Ui,
    size: f32,
    progress: f32,
    time_text: &str,
    accent: Color32,
) {
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
        let p = Pos2::new(
            center.x + angle.cos() * radius,
            center.y + angle.sin() * radius,
        );
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

fn truncate_chars(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        s.to_string()
    } else if max_chars <= 1 {
        "…".to_string()
    } else {
        let prefix: String = s.chars().take(max_chars - 1).collect();
        format!("{prefix}…")
    }
}

fn is_external_tool_name(name: &str) -> bool {
    matches!(
        name,
        "web_search" | "web_fetch" | "get_weather" | "get_exchange_rate"
    )
}

async fn execute_external_tool_async(
    engine: &FreeToolsEngine,
    cfg: &crate::config::ExternalToolsConfig,
    name: &str,
    args: &serde_json::Value,
) -> Result<String, String> {
    match name {
        "web_search" => {
            let query = args["query"].as_str().unwrap_or("");
            let max_results = args["max_results"].as_u64().unwrap_or(5) as usize;
            let resp = engine
                .execute_web_search(cfg, query, max_results)
                .await
                .map_err(|e| e.to_string())?;
            Ok(format_search_response_for_user_and_llm(&resp))
        }
        "web_fetch" => {
            let url = args["url"].as_str().unwrap_or("");
            engine
                .execute_web_fetch(cfg, url)
                .await
                .map_err(|e| e.to_string())
        }
        "get_weather" => {
            let city = args["city"]
                .as_str()
                .or_else(|| args["location"].as_str())
                .unwrap_or("Buenos Aires");
            engine
                .execute_weather(cfg, city)
                .await
                .map_err(|e| e.to_string())
        }
        "get_exchange_rate" => {
            let base = args["base"].as_str().unwrap_or("USD");
            let target = args["target"].as_str().unwrap_or("EUR");
            let amount = args["amount"].as_f64().unwrap_or(1.0);
            engine
                .execute_exchange_rate(cfg, base, target, amount)
                .await
                .map_err(|e| e.to_string())
        }
        _ => Err(format!("Herramienta externa desconocida: {name}")),
    }
}
