use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Vec2};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelIcon {
    Chat,
    Tasks,
    Habits,
    Focus,
    Organizer,
    Markdown,
    Compact,
    Settings,
    NewChat,
    History,
    Mic,
    Attach,
    Send,
    Copy,
    Save,
    Clear,
    Close,
    Pin,
    Island,
    Play,
    Pause,
    Reset,
    Eye,
    Check,
    Undo,
    FolderOpen,
}

impl PixelIcon {
    /// Returns an 8x8 1-bit bitmap (each u8 is one row, MSB = leftmost pixel).
    pub const fn bitmap(self) -> [u8; 8] {
        match self {
            // Speech bubble
            Self::Chat => [
                0b00000000,
                0b01111110,
                0b10000001,
                0b10101001,
                0b10000001,
                0b01111110,
                0b00110000,
                0b00100000,
            ],
            // Checklist
            Self::Tasks => [
                0b00000000,
                0b11001111,
                0b11000000,
                0b00001111,
                0b01000000,
                0b10101111,
                0b01000000,
                0b00000000,
            ],
            // 8-bit Heart
            Self::Habits => [
                0b00000000,
                0b01100110,
                0b11111111,
                0b11111111,
                0b01111110,
                0b00111100,
                0b00011000,
                0b00000000,
            ],
            // Hourglass
            Self::Focus => [
                0b01111110,
                0b01000010,
                0b00111100,
                0b00011000,
                0b00011000,
                0b00100100,
                0b01111110,
                0b01111110,
            ],
            // Folder
            Self::Organizer => [
                0b00000000,
                0b01110000,
                0b11111110,
                0b10000001,
                0b10000001,
                0b10000001,
                0b11111111,
                0b00000000,
            ],
            // Document
            Self::Markdown => [
                0b01111100,
                0b01000110,
                0b01011010,
                0b01000010,
                0b01011110,
                0b01000010,
                0b01111110,
                0b00000000,
            ],
            // Compress arrows (inward)
            Self::Compact => [
                0b00011000,
                0b00111100,
                0b00011000,
                0b11111111,
                0b11111111,
                0b00011000,
                0b00111100,
                0b00011000,
            ],
            // Gear
            Self::Settings => [
                0b00011000,
                0b01011010,
                0b00111100,
                0b11100111,
                0b11100111,
                0b00111100,
                0b01011010,
                0b00011000,
            ],
            // Plus (+)
            Self::NewChat => [
                0b00000000,
                0b00011000,
                0b00011000,
                0b01111110,
                0b01111110,
                0b00011000,
                0b00011000,
                0b00000000,
            ],
            // Clock / History
            Self::History => [
                0b00111100,
                0b01000010,
                0b10010001,
                0b10010001,
                0b10011101,
                0b10000001,
                0b01000010,
                0b00111100,
            ],
            // Microphone
            Self::Mic => [
                0b00011000,
                0b00111100,
                0b00111100,
                0b00111100,
                0b01011010,
                0b00111100,
                0b00011000,
                0b00111100,
            ],
            // Paperclip / Attach
            Self::Attach => [
                0b00011100,
                0b00100010,
                0b00101010,
                0b00101010,
                0b00101010,
                0b00101010,
                0b00100100,
                0b00011000,
            ],
            // Right arrow / Send
            Self::Send => [
                0b00000000,
                0b00001000,
                0b00001100,
                0b01111110,
                0b01111111,
                0b01111110,
                0b00001100,
                0b00001000,
            ],
            // Two overlapping sheets / Copy
            Self::Copy => [
                0b00111110,
                0b00100010,
                0b11111010,
                0b10001010,
                0b10001010,
                0b10001110,
                0b11111000,
                0b00000000,
            ],
            // 8-bit Floppy disk / Save
            Self::Save => [
                0b11111100,
                0b10110110,
                0b10110111,
                0b10000001,
                0b10111101,
                0b10100101,
                0b10111101,
                0b11111111,
            ],
            // Trash / Clear
            Self::Clear => [
                0b00111100,
                0b11111111,
                0b01000010,
                0b01011010,
                0b01011010,
                0b01011010,
                0b01111110,
                0b00000000,
            ],
            // X / Close
            Self::Close => [
                0b00000000,
                0b01000010,
                0b00100100,
                0b00011000,
                0b00011000,
                0b00100100,
                0b01000010,
                0b00000000,
            ],
            // Pushpin
            Self::Pin => [
                0b00111100,
                0b00011000,
                0b00011000,
                0b01111110,
                0b00011000,
                0b00011000,
                0b00011000,
                0b00000000,
            ],
            // Window / Island
            Self::Island => [
                0b00000000,
                0b11111111,
                0b11111111,
                0b10000001,
                0b10000001,
                0b10000001,
                0b11111111,
                0b00000000,
            ],
            // Play triangle
            Self::Play => [
                0b00100000,
                0b00110000,
                0b00111000,
                0b00111100,
                0b00111100,
                0b00111000,
                0b00110000,
                0b00100000,
            ],
            // Pause bars
            Self::Pause => [
                0b00000000,
                0b01100110,
                0b01100110,
                0b01100110,
                0b01100110,
                0b01100110,
                0b01100110,
                0b00000000,
            ],
            // Reset / Rewind
            Self::Reset => [
                0b00111100,
                0b01000010,
                0b10000001,
                0b11100001,
                0b01000001,
                0b00000010,
                0b01111100,
                0b00000000,
            ],
            // Eye / Preview
            Self::Eye => [
                0b00000000,
                0b00111100,
                0b01000010,
                0b10011001,
                0b10011001,
                0b01000010,
                0b00111100,
                0b00000000,
            ],
            // Checkmark / Apply
            Self::Check => [
                0b00000000,
                0b00000001,
                0b00000011,
                0b10000110,
                0b11001100,
                0b01111000,
                0b00110000,
                0b00000000,
            ],
            // Undo arrow
            Self::Undo => [
                0b00000000,
                0b00100000,
                0b01100000,
                0b11111100,
                0b01100010,
                0b00100010,
                0b00011100,
                0b00000000,
            ],
            // Open file / folder
            Self::FolderOpen => [
                0b01110000,
                0b10001110,
                0b10000010,
                0b11111111,
                0b10000001,
                0b10000001,
                0b11111111,
                0b00000000,
            ],
        }
    }
}

/// Paints an 8x8 pixel-art icon inside `rect` using crisp unrounded squares.
pub fn paint_pixel_icon(painter: &egui::Painter, rect: Rect, icon: PixelIcon, color: Color32) {
    let bitmap = icon.bitmap();
    let cell_w = rect.width() / 8.0;
    let cell_h = rect.height() / 8.0;

    for (row, &bits) in bitmap.iter().enumerate() {
        for col in 0..8 {
            if (bits & (1 << (7 - col))) != 0 {
                let x = rect.min.x + col as f32 * cell_w;
                let y = rect.min.y + row as f32 * cell_h;
                let px = Rect::from_min_size(Pos2::new(x, y), Vec2::new(cell_w.ceil(), cell_h.ceil()));
                painter.rect_filled(px, 0.0, color);
            }
        }
    }
}

/// Draws a clickable button with a crisp 8-bit icon and optional text label.
pub fn pixel_button(
    ui: &mut egui::Ui,
    icon: PixelIcon,
    label: &str,
    fill: Color32,
    text_color: Color32,
    tooltip: &str,
) -> egui::Response {
    let icon_size = 11.0;
    let font_id = egui::FontId::proportional(11.5);
    let galley = if label.is_empty() {
        None
    } else {
        Some(ui.painter().layout_no_wrap(label.to_string(), font_id, text_color))
    };

    let text_w = galley.as_ref().map_or(0.0, |g| g.size().x);
    let gap = if label.is_empty() { 0.0 } else { 5.0 };
    let pad_x = 8.0;
    let height = 24.0;
    let width = pad_x * 2.0 + icon_size + gap + text_w;

    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    if ui.is_rect_visible(rect) {
        let bg = if resp.hovered() {
            Color32::from_rgb(
                fill.r().saturating_add(18),
                fill.g().saturating_add(18),
                fill.b().saturating_add(18),
            )
        } else {
            fill
        };
        ui.painter().rect_filled(rect, 4.0, bg);

        let icon_rect = Rect::from_min_size(
            Pos2::new(rect.min.x + pad_x, rect.center().y - icon_size * 0.5),
            Vec2::splat(icon_size),
        );
        paint_pixel_icon(ui.painter(), icon_rect, icon, text_color);

        if let Some(g) = galley {
            let text_pos = Pos2::new(
                icon_rect.max.x + gap,
                rect.center().y - g.size().y * 0.5,
            );
            ui.painter().galley(text_pos, g, text_color);
        }
    }

    if tooltip.is_empty() {
        resp
    } else {
        resp.on_hover_text(RichText::new(tooltip).size(11.5))
    }
}
