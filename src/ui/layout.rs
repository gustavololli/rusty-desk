//! Pure layout decisions for the main window.
//!
//! The chat paints from these numbers so breakpoints, collapse, wrapping and
//! the retired pet-mode migration can be tested without opening a window.

use crate::ui::pet::WindowMode;

/// Content width at which the saved-chat list may dock on the left.
/// Narrower windows keep the conversation column and a button to open history.
pub const SIDEBAR_BREAKPOINT: f32 = 720.0;
pub const HISTORY_SIDEBAR_WIDTH: f32 = 228.0;
pub const HISTORY_GUTTER: f32 = 8.0;
/// Reading measure for the conversation and the composer. Wider windows center it.
pub const READING_MAX_WIDTH: f32 = 640.0;
/// Composer inner width below which the action buttons move to their own row.
/// Keeps Send / Cancel clickable beside the field at the minimum window size.
pub const COMPOSER_WRAP_BELOW: f32 = 360.0;
pub const BUBBLE_INNER_MARGIN: f32 = 9.0;
/// Stroke on both sides of a bubble, kept inside the column.
pub const BUBBLE_STROKE: f32 = 2.0;
pub const EXPANDED_FALLBACK_SIZE: [f32; 2] = [450.0, 650.0];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryPane {
    Closed,
    Sidebar,
    Inline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryChrome {
    pub pane: HistoryPane,
    /// Set when the sidebar was closed only because the window became too narrow.
    /// Widening the window opens it again. An explicit close clears the flag.
    pub reopen_when_wide: bool,
}

impl HistoryChrome {
    pub const fn startup() -> Self {
        Self {
            pane: HistoryPane::Closed,
            reopen_when_wide: true,
        }
    }

    pub const fn closed_by_user() -> Self {
        Self {
            pane: HistoryPane::Closed,
            reopen_when_wide: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChatColumnGeometry {
    pub sidebar_width: f32,
    pub gutter: f32,
    pub column_width: f32,
    pub column_leading: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MaximizeStep {
    Maximize { restore_size: [f32; 2] },
    Restore { size: [f32; 2] },
}

pub fn allows_sidebar(content_width: f32) -> bool {
    content_width.is_finite() && content_width >= SIDEBAR_BREAKPOINT
}

/// Collapse or restore the history pane after the window width changes.
pub fn history_after_resize(chrome: HistoryChrome, content_width: f32) -> HistoryChrome {
    let wide = allows_sidebar(content_width);
    match chrome.pane {
        HistoryPane::Sidebar if !wide => HistoryChrome {
            pane: HistoryPane::Closed,
            reopen_when_wide: true,
        },
        HistoryPane::Closed if wide && chrome.reopen_when_wide => HistoryChrome {
            pane: HistoryPane::Sidebar,
            reopen_when_wide: true,
        },
        HistoryPane::Inline if wide => HistoryChrome {
            pane: HistoryPane::Sidebar,
            reopen_when_wide: true,
        },
        _ => chrome,
    }
}

pub fn toggle_history(chrome: HistoryChrome, content_width: f32) -> HistoryChrome {
    if chrome.pane != HistoryPane::Closed {
        HistoryChrome::closed_by_user()
    } else if allows_sidebar(content_width) {
        HistoryChrome {
            pane: HistoryPane::Sidebar,
            reopen_when_wide: true,
        }
    } else {
        HistoryChrome {
            pane: HistoryPane::Inline,
            reopen_when_wide: false,
        }
    }
}

pub fn chat_column_geometry(content_width: f32, pane: HistoryPane) -> ChatColumnGeometry {
    let width = if content_width.is_finite() {
        content_width.max(0.0)
    } else {
        READING_MAX_WIDTH
    };
    let sidebar = if pane == HistoryPane::Sidebar {
        HISTORY_SIDEBAR_WIDTH.min(width)
    } else {
        0.0
    };
    let gutter = if sidebar > 0.0 { HISTORY_GUTTER } else { 0.0 };
    let rest = (width - sidebar - gutter).max(0.0);
    let column = rest.min(READING_MAX_WIDTH);
    let leading = ((rest - column) * 0.5).max(0.0);
    ChatColumnGeometry {
        sidebar_width: sidebar,
        gutter,
        column_width: column,
        column_leading: leading,
    }
}

pub fn composer_wraps(composer_inner_width: f32) -> bool {
    !composer_inner_width.is_finite() || composer_inner_width < COMPOSER_WRAP_BELOW
}

/// Text width inside a bubble so the frame padding stays inside `column_width`.
pub fn bubble_text_width(column_width: f32, inner_margin: f32) -> f32 {
    let column = if column_width.is_finite() {
        column_width.max(0.0)
    } else {
        0.0
    };
    let margin = if inner_margin.is_finite() {
        inner_margin.max(0.0)
    } else {
        0.0
    };
    (column - margin * 2.0 - BUBBLE_STROKE).max(0.0)
}

pub fn maximize_step(
    maximized: bool,
    current_size: [f32; 2],
    saved_size: Option<[f32; 2]>,
) -> MaximizeStep {
    if maximized {
        MaximizeStep::Restore {
            size: saved_size.unwrap_or(EXPANDED_FALLBACK_SIZE),
        }
    } else {
        let restore_size = if current_size[0].is_finite() && current_size[1].is_finite() {
            current_size
        } else {
            EXPANDED_FALLBACK_SIZE
        };
        MaximizeStep::Maximize { restore_size }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownBlock<'a> {
    Prose(&'a str),
    Code { lang: &'a str, body: &'a str },
}

/// Splits fenced code out of Markdown so each fence can scroll on its own.
/// Prose, including links and inline code, stays in order around the fences.
pub fn split_fenced_blocks(text: &str) -> Vec<MarkdownBlock<'_>> {
    let mut blocks = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("```") {
        if start > 0 {
            blocks.push(MarkdownBlock::Prose(&rest[..start]));
        }
        let after = &rest[start + 3..];
        let (lang, body_and_rest) = match after.find('\n') {
            Some(nl) => (after[..nl].trim(), &after[nl + 1..]),
            None => (after.trim(), ""),
        };
        if let Some(end) = body_and_rest.find("```") {
            blocks.push(MarkdownBlock::Code {
                lang,
                body: &body_and_rest[..end],
            });
            rest = &body_and_rest[end + 3..];
        } else {
            blocks.push(MarkdownBlock::Code {
                lang,
                body: body_and_rest,
            });
            return blocks;
        }
    }
    if !rest.is_empty() {
        blocks.push(MarkdownBlock::Prose(rest));
    }
    blocks
}

/// Pet mode is no longer a window. A saved installation opens the main window.
/// Island and the notch stay as they were.
pub fn startup_window_mode(saved: WindowMode) -> WindowMode {
    match saved {
        WindowMode::Pet => WindowMode::Expanded,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidebar_opens_only_on_a_wide_window_and_collapses_when_shrunk() {
        let startup = HistoryChrome::startup();
        let narrow = history_after_resize(startup, 450.0);
        assert_eq!(narrow.pane, HistoryPane::Closed);
        assert!(narrow.reopen_when_wide);
        assert!(!allows_sidebar(719.0));
        assert!(allows_sidebar(SIDEBAR_BREAKPOINT));

        let wide = history_after_resize(narrow, 1100.0);
        assert_eq!(wide.pane, HistoryPane::Sidebar);

        let restored = history_after_resize(wide, 640.0);
        assert_eq!(restored.pane, HistoryPane::Closed);
        assert!(
            restored.reopen_when_wide,
            "shrinking leaves a way to reopen"
        );

        let again = history_after_resize(restored, 1280.0);
        assert_eq!(again.pane, HistoryPane::Sidebar);
    }

    #[test]
    fn explicit_close_stays_closed_and_narrow_open_uses_the_inline_list() {
        let open = history_after_resize(HistoryChrome::startup(), 1000.0);
        let closed = toggle_history(open, 1000.0);
        assert_eq!(closed, HistoryChrome::closed_by_user());
        let still = history_after_resize(history_after_resize(closed, 400.0), 1400.0);
        assert_eq!(still.pane, HistoryPane::Closed);

        let inline = toggle_history(HistoryChrome::closed_by_user(), 360.0);
        assert_eq!(inline.pane, HistoryPane::Inline);
        assert_eq!(
            history_after_resize(inline, 980.0).pane,
            HistoryPane::Sidebar
        );
    }

    #[test]
    fn column_stays_inside_the_content_width_and_centers_when_wider_than_the_measure() {
        let compact = chat_column_geometry(320.0, HistoryPane::Closed);
        assert_eq!(compact.sidebar_width, 0.0);
        assert_eq!(compact.column_width, 320.0);
        assert_eq!(compact.column_leading, 0.0);

        let wide = chat_column_geometry(1400.0, HistoryPane::Sidebar);
        assert_eq!(wide.sidebar_width, HISTORY_SIDEBAR_WIDTH);
        assert_eq!(wide.column_width, READING_MAX_WIDTH);
        let rest = 1400.0 - HISTORY_SIDEBAR_WIDTH - HISTORY_GUTTER;
        assert!((wide.column_leading - (rest - READING_MAX_WIDTH) * 0.5).abs() < 0.01);
        assert!(
            wide.sidebar_width + wide.gutter + wide.column_leading * 2.0 + wide.column_width
                <= 1400.01
        );

        let infinite = chat_column_geometry(f32::INFINITY, HistoryPane::Closed);
        assert_eq!(infinite.column_width, READING_MAX_WIDTH);
    }

    #[test]
    fn composer_wraps_below_the_single_row_budget() {
        assert!(composer_wraps(COMPOSER_WRAP_BELOW - 1.0));
        assert!(!composer_wraps(COMPOSER_WRAP_BELOW));
        assert!(composer_wraps(f32::NAN));
        assert!(!composer_wraps(520.0));
    }

    #[test]
    fn bubble_text_keeps_its_padding_inside_the_column() {
        let text = bubble_text_width(292.0, BUBBLE_INNER_MARGIN);
        assert!((text - (292.0 - BUBBLE_INNER_MARGIN * 2.0 - BUBBLE_STROKE)).abs() < 0.01);
        assert!(text + BUBBLE_INNER_MARGIN * 2.0 + BUBBLE_STROKE <= 292.0);
        assert_eq!(bubble_text_width(8.0, BUBBLE_INNER_MARGIN), 0.0);
        assert_eq!(bubble_text_width(f32::INFINITY, BUBBLE_INNER_MARGIN), 0.0);
    }

    #[test]
    fn maximize_remembers_the_size_to_restore() {
        assert_eq!(
            maximize_step(false, [500.0, 700.0], None),
            MaximizeStep::Maximize {
                restore_size: [500.0, 700.0]
            }
        );
        assert_eq!(
            maximize_step(true, [1920.0, 1080.0], Some([500.0, 700.0])),
            MaximizeStep::Restore {
                size: [500.0, 700.0]
            }
        );
        assert_eq!(
            maximize_step(true, [1920.0, 1080.0], None),
            MaximizeStep::Restore {
                size: EXPANDED_FALLBACK_SIZE
            }
        );
        assert_eq!(
            maximize_step(false, [f32::NAN, 10.0], None),
            MaximizeStep::Maximize {
                restore_size: EXPANDED_FALLBACK_SIZE
            }
        );
    }

    #[test]
    fn saved_pet_mode_starts_in_the_main_window() {
        assert_eq!(startup_window_mode(WindowMode::Pet), WindowMode::Expanded);
        assert_eq!(startup_window_mode(WindowMode::Island), WindowMode::Island);
        assert_eq!(
            startup_window_mode(WindowMode::Expanded),
            WindowMode::Expanded
        );
    }

    #[test]
    fn fenced_code_is_split_out_of_the_prose() {
        let text = "Mirá https://example.com/muy/largo\n\n```rust\nfn main() { println!(\"hola\"); }\n```\n\nDespués";
        let blocks = split_fenced_blocks(text);
        assert_eq!(blocks.len(), 3);
        assert!(matches!(blocks[0], MarkdownBlock::Prose(p) if p.contains("example.com")));
        match blocks[1] {
            MarkdownBlock::Code { lang, body } => {
                assert_eq!(lang, "rust");
                assert!(body.contains("println"));
                assert!(!body.contains("```"));
            }
            MarkdownBlock::Prose(_) => panic!("expected a code fence"),
        }
        assert!(matches!(blocks[2], MarkdownBlock::Prose(p) if p.contains("Después")));

        let open = split_fenced_blocks("```\nlet x = 1;");
        assert_eq!(
            open,
            vec![MarkdownBlock::Code {
                lang: "",
                body: "let x = 1;"
            }]
        );
        assert_eq!(
            split_fenced_blocks("solo texto"),
            vec![MarkdownBlock::Prose("solo texto")]
        );
    }
}
