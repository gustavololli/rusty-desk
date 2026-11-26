//! Which conversation the Chat tab shows.
//!
//! Switching sections must not allocate a new conversation. Only an explicit
//! "Nuevo" does that. The Chat tab returns to the conversation already open.

#[derive(Debug, Clone, PartialEq)]
pub struct ActiveConversation {
    pub id: String,
    pub messages: Vec<String>,
    pub draft: String,
    pub scroll_y: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SectionView {
    pub conversation: ActiveConversation,
    /// Radar inbox hides the transcript. The Chat tab closes it.
    pub news_inbox_open: bool,
    /// Scroll offset to reapply when the transcript is shown again.
    pub scroll_hold: Option<f32>,
}

/// `from_chat` / `to_chat` describe the section change. The conversation itself
/// is returned untouched.
pub fn change_section(
    chat: ActiveConversation,
    from_chat: bool,
    to_chat: bool,
    news_inbox_open: bool,
    scroll_hold: Option<f32>,
) -> SectionView {
    let scroll_hold = if from_chat && !to_chat {
        Some(chat.scroll_y)
    } else {
        scroll_hold
    };
    SectionView {
        conversation: chat,
        news_inbox_open: if to_chat { false } else { news_inbox_open },
        scroll_hold,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ActiveConversation {
        ActiveConversation {
            id: "conv-42".into(),
            messages: vec!["Hola".into(), "Reproduciendo: cadena heat".into()],
            draft: "poné otra".into(),
            scroll_y: 180.0,
        }
    }

    #[test]
    fn chat_then_settings_then_chat_keeps_the_same_conversation() {
        let open = sample();
        let away = change_section(open.clone(), true, false, true, None);
        assert_eq!(away.conversation, open);
        assert!(away.news_inbox_open);
        assert_eq!(away.scroll_hold, Some(180.0));

        let back = change_section(
            away.conversation,
            false,
            true,
            away.news_inbox_open,
            away.scroll_hold,
        );
        assert_eq!(back.conversation.id, open.id);
        assert_eq!(back.conversation.messages, open.messages);
        assert_eq!(back.conversation.draft, open.draft);
        assert_eq!(back.conversation.scroll_y, open.scroll_y);
        assert!(!back.news_inbox_open);
        assert_eq!(back.scroll_hold, Some(180.0));
    }
}
