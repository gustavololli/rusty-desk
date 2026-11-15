use crate::state::AssistantMood;
use crate::ui::avatar::draw_avatar_frame;
use eframe::egui::{self, Pos2};

use super::animation::AnimationFrame;

/// Renders Rusty in floating Pet Mode on a fully transparent canvas.
pub struct PetRenderer;

impl PetRenderer {
    pub fn render(
        ui: &mut egui::Ui,
        size: f32,
        mood: AssistantMood,
        time: f64,
        look_target: Option<Pos2>,
        frame: &AnimationFrame,
    ) -> egui::Response {
        draw_avatar_frame(ui, size, mood, time, look_target, frame)
    }
}
