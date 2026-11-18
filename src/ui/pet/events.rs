use serde::{Deserialize, Serialize};

/// Display mode of the rusty-desk window.
/// All three modes share the exact same logical application state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum WindowMode {
    #[default]
    Expanded,
    Island,
    Pet,
}

/// Priority levels for interrupting or scheduling pet animations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EventPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

/// Semantic events emitted by rusty-desk subsystems to drive pet reactions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PetEvent {
    UserClicked,
    UserHovered,
    UserDragged,
    AiThinking,
    AiReplyReady,
    AiError,
    TaskCompleted,
    AllTasksCompleted,
    HabitCompleted,
    PomodoroStarted,
    PomodoroPaused,
    PomodoroFinished,
    VoiceListeningStart,
    VoiceListeningStop,
    MarkdownConverted,
    ErrorOccurred,
    ModeChanged(WindowMode),
    IdleTimeout,
    CuriosityTrigger,
    LongWorkSession,
    UserReturned,
    SectionEmpty(EmptySection),
}

/// Only sections with a meaningful empty state; settings/focus aren't "empty".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmptySection {
    Tasks,
    Habits,
    Music,
    Chat,
    Markdown,
    Organizer,
}

impl PetEvent {
    /// Returns the preemption priority of this event.
    pub fn priority(self) -> EventPriority {
        match self {
            Self::PomodoroFinished | Self::ErrorOccurred | Self::AiError => EventPriority::Critical,
            Self::TaskCompleted
            | Self::AllTasksCompleted
            | Self::HabitCompleted
            | Self::AiReplyReady
            | Self::MarkdownConverted
            | Self::UserClicked
            | Self::VoiceListeningStart
            | Self::AiThinking => EventPriority::High,
            Self::PomodoroStarted
            | Self::PomodoroPaused
            | Self::VoiceListeningStop
            | Self::ModeChanged(_)
            | Self::UserDragged
            | Self::UserHovered => EventPriority::Normal,
            Self::UserReturned => EventPriority::Normal,
            Self::IdleTimeout | Self::CuriosityTrigger | Self::SectionEmpty(_)
            | Self::LongWorkSession => EventPriority::Low,
        }
    }
}
