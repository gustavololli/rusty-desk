use crate::ui::avatar::draw_rusty;
use crate::ui::pet::animation::AnimationFrame;
use eframe::egui::{self, Pos2};

/// Renders the 16-bit Rusty robot in floating Pet mode.
pub struct PetRenderer;

impl PetRenderer {
    pub fn render(
        ui: &mut egui::Ui,
        size: f32,
        frame: &AnimationFrame,
        reduce_animations: bool,
        look_target: Option<Pos2>,
        follow_cursor: bool,
    ) -> egui::Response {
        draw_rusty(
            ui,
            size,
            frame,
            reduce_animations,
            look_target,
            follow_cursor,
        )
    }
}
