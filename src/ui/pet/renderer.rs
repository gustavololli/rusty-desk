use crate::ui::avatar::draw_rusty;
use crate::ui::pet::animation::AnimationFrame;
use eframe::egui::{self, Pos2};

/// Draws Rusty's sprite. The floating pet window is retired; the header,
/// island and notch call `draw_rusty` directly and this wrapper stays for that sprite.
#[allow(dead_code)]
pub struct PetRenderer;

#[allow(dead_code)]
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
