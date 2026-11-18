//! Rusty's avatar. The procedural sphere is gone: every call paints the
//! yellow 16-bit robot from the shared sprite sheet.

use crate::ui::pet::animation::AnimationFrame;
use crate::ui::sprites;
use eframe::egui::{self, Pos2};

pub fn draw_rusty(
    ui: &mut egui::Ui,
    size: f32,
    frame: &AnimationFrame,
    reduce_animations: bool,
    look_pos: Option<Pos2>,
    follow_cursor: bool,
) -> egui::Response {
    sprites::draw_rusty(ui, size, frame, reduce_animations, look_pos, follow_cursor)
}
