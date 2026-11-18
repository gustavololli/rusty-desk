//! Shared 16-bit palette and console-control painting.
//!
//! Screens should use these colors and the bevel helper instead of inventing
//! another retro style.

use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};

pub const NIGHT: Color32 = Color32::from_rgb(10, 16, 32);
pub const PANEL: Color32 = Color32::from_rgb(22, 32, 52);
pub const BUTTON: Color32 = Color32::from_rgb(28, 40, 64);
pub const BUTTON_HOVER: Color32 = Color32::from_rgb(40, 56, 88);
pub const BUTTON_PRESSED: Color32 = Color32::from_rgb(16, 24, 40);
pub const BORDER: Color32 = Color32::from_rgb(90, 110, 145);
pub const BORDER_DARK: Color32 = Color32::from_rgb(6, 10, 20);
pub const GOLD: Color32 = Color32::from_rgb(242, 160, 21);
pub const GOLD_LIGHT: Color32 = Color32::from_rgb(255, 219, 97);
pub const GOLD_DARK: Color32 = Color32::from_rgb(170, 87, 7);
pub const OXIDE_DARK: Color32 = Color32::from_rgb(110, 49, 31);
pub const OXIDE_COPPER: Color32 = Color32::from_rgb(185, 91, 42);
pub const CYAN: Color32 = Color32::from_rgb(58, 232, 247);
pub const EMERALD: Color32 = Color32::from_rgb(52, 211, 153);
pub const ERROR: Color32 = Color32::from_rgb(229, 105, 115);
pub const DANGER: Color32 = Color32::from_rgb(122, 36, 48);
pub const IVORY: Color32 = Color32::from_rgb(255, 247, 223);
pub const MUTED: Color32 = Color32::from_rgb(176, 188, 210);
pub const FIELD: Color32 = Color32::from_rgb(7, 11, 20);

pub const BUTTON_HEIGHT: f32 = 26.0;
pub const ICON_SIZE: f32 = 16.0;
pub const BUTTON_PAD_X: f32 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelButtonLayout {
    pub width: f32,
    pub height: f32,
    pub icon_size: f32,
    pub pad_x: f32,
    pub gap: f32,
}

pub fn pixel_button_layout(text_width: f32, has_label: bool) -> PixelButtonLayout {
    let gap = if has_label { 4.0 } else { 0.0 };
    let text = if has_label { text_width.max(0.0) } else { 0.0 };
    PixelButtonLayout {
        width: BUTTON_PAD_X * 2.0 + ICON_SIZE + gap + text,
        height: BUTTON_HEIGHT,
        icon_size: ICON_SIZE,
        pad_x: BUTTON_PAD_X,
        gap,
    }
}

pub fn shade(color: Color32, delta: i16) -> Color32 {
    let shift = |channel: u8| (i16::from(channel) + delta).clamp(0, 255) as u8;
    Color32::from_rgb(shift(color.r()), shift(color.g()), shift(color.b()))
}

pub fn console_frame(horizontal: f32, vertical: f32) -> egui::Frame {
    egui::Frame::none()
        .fill(Color32::from_rgba_unmultiplied(
            NIGHT.r(),
            NIGHT.g(),
            NIGHT.b(),
            248,
        ))
        .stroke(Stroke::new(2.0_f32, OXIDE_DARK))
        .rounding(egui::Rounding::ZERO)
        .inner_margin(egui::Margin::symmetric(horizontal, vertical))
}

#[derive(Debug, Clone, Copy)]
pub enum BezelSize {
    Window,
    Island,
    Notch,
}

/// Layered pixel-metal bezel, drawn inside the viewport with no image assets.
/// Large windows have steel rails and riveted copper corners; the island keeps
/// lighter rails, and the notch uses a tiny version without screws.
pub fn paint_oxidized_bezel(painter: &egui::Painter, rect: Rect, size: BezelSize) {
    if rect.width() < 12.0 || rect.height() < 12.0 {
        return;
    }
    let (rail, corner) = match size {
        BezelSize::Window => (9.0, 18.0),
        BezelSize::Island => (6.0, 11.0),
        BezelSize::Notch => (3.0, 5.0),
    };
    let steel = Color32::from_rgb(39, 53, 65);
    let steel_light = Color32::from_rgb(87, 105, 114);
    let steel_shadow = Color32::from_rgb(17, 26, 34);

    // Inset rails: shadow, highlight, weathered steel, deep recess and brass lip.
    for (inset, color) in [
        (0.5, OXIDE_DARK),
        (1.5, steel_shadow),
        (2.5, steel_light),
        (3.5, steel),
        (rail - 2.0, steel_shadow),
        (rail - 1.0, GOLD_DARK),
    ] {
        if inset >= 0.0 && inset < rail && rect.width() > inset * 2.0 {
            painter.rect_stroke(rect.shrink(inset), 0.0, Stroke::new(1.0, color));
        }
    }
    if matches!(size, BezelSize::Window | BezelSize::Island) {
        // Metal rail fill between inner and outer strokes, with directional lighting.
        for i in 4..(rail as i32 - 2) {
            let color = if i % 3 == 0 { steel_light } else { steel };
            painter.rect_stroke(rect.shrink(i as f32 + 0.5), 0.0, Stroke::new(1.0, color));
        }
    }

    // Pixel-stepped copper plates on each corner, like a worn 16-bit console.
    for (right, bottom) in [(false, false), (true, false), (false, true), (true, true)] {
        let x = if right { rect.right() - corner } else { rect.left() };
        let y = if bottom { rect.bottom() - corner } else { rect.top() };
        for step in 0..3 {
            let unit = 2.0;
            let offset = step as f32 * unit;
            let color = match step {
                0 => OXIDE_DARK,
                1 => OXIDE_COPPER,
                _ => GOLD_DARK,
            };
            let px = if right { x + offset } else { x };
            let py = if bottom { y + offset } else { y };
            painter.rect_filled(
                Rect::from_min_size(Pos2::new(px, py), Vec2::new(corner - offset, unit)),
                0.0,
                color,
            );
            let vx = if right { x + corner - unit } else { x + offset };
            let vy = if bottom { y + offset } else { y };
            painter.rect_filled(
                Rect::from_min_size(Pos2::new(vx, vy), Vec2::new(unit, corner - offset)),
                0.0,
                color,
            );
        }
        if !matches!(size, BezelSize::Notch) {
            let center = Pos2::new(x + corner / 2.0, y + corner / 2.0);
            painter.rect_filled(Rect::from_center_size(center, Vec2::splat(6.0)), 0.0, steel_shadow);
            painter.rect_filled(Rect::from_center_size(center, Vec2::splat(4.0)), 0.0, GOLD_DARK);
            painter.line_segment(
                [center + Vec2::new(-1.0, 1.0), center + Vec2::new(1.0, -1.0)],
                Stroke::new(1.0, steel_shadow),
            );
        }
    }

    // Deterministic oxide specks, constrained to the rail so the UI stays crisp.
    if matches!(size, BezelSize::Window) {
        for n in 0..18 {
            let t = (n as f32 + 0.6) / 19.0;
            let x = rect.left() + rect.width() * t;
            let y = if n % 2 == 0 { rect.top() + 4.0 } else { rect.bottom() - 6.0 };
            painter.rect_filled(
                Rect::from_min_size(Pos2::new(x, y), Vec2::splat(if n % 3 == 0 { 3.0 } else { 2.0 })),
                0.0,
                if n % 4 == 0 { OXIDE_COPPER } else { OXIDE_DARK },
            );
        }
    }
}

pub fn paint_pixel_bevel(
    painter: &egui::Painter,
    rect: Rect,
    fill: Color32,
    hovered: bool,
    pressed: bool,
    focused: bool,
    enabled: bool,
) {
    if enabled && !pressed {
        let shadow = rect.translate(Vec2::new(2.0, 2.0));
        painter.rect_filled(shadow, 0.0, Color32::from_rgba_unmultiplied(0, 0, 0, 170));
    }
    painter.rect_filled(rect, 0.0, fill);
    let light = if !enabled {
        BORDER
    } else if hovered || focused {
        GOLD_LIGHT
    } else {
        BORDER
    };
    let dark = if pressed { GOLD_DARK } else { BORDER_DARK };
    let (edge_a, edge_b) = if pressed {
        (dark, light)
    } else {
        (light, dark)
    };
    let thickness = 2.0;
    painter.rect_filled(
        Rect::from_min_max(Pos2::new(rect.min.x, rect.max.y - thickness), rect.max),
        0.0,
        edge_b,
    );
    painter.rect_filled(
        Rect::from_min_max(Pos2::new(rect.max.x - thickness, rect.min.y), rect.max),
        0.0,
        edge_b,
    );
    painter.rect_filled(
        Rect::from_min_size(rect.min, Vec2::new(rect.width(), thickness)),
        0.0,
        edge_a,
    );
    painter.rect_filled(
        Rect::from_min_size(rect.min, Vec2::new(thickness, rect.height())),
        0.0,
        edge_a,
    );
    if focused && enabled {
        painter.rect_stroke(rect, 0.0, Stroke::new(1.0_f32, GOLD));
    }
}

pub fn apply_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(IVORY);
    visuals.hyperlink_color = CYAN;
    visuals.faint_bg_color = PANEL;
    visuals.extreme_bg_color = FIELD;
    visuals.code_bg_color = FIELD;
    visuals.warn_fg_color = GOLD;
    visuals.error_fg_color = ERROR;
    visuals.window_fill = NIGHT;
    visuals.panel_fill = Color32::TRANSPARENT;
    visuals.window_stroke = Stroke::new(2.0_f32, GOLD);
    visuals.window_rounding = egui::Rounding::ZERO;
    visuals.menu_rounding = egui::Rounding::ZERO;
    visuals.window_shadow = egui::epaint::Shadow {
        offset: Vec2::new(3.0, 3.0),
        blur: 0.0,
        spread: 0.0,
        color: Color32::from_black_alpha(190),
    };
    visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(GOLD.r(), GOLD.g(), GOLD.b(), 90);
    visuals.selection.stroke = Stroke::new(1.0_f32, GOLD_LIGHT);

    let widgets = [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ];
    for widget in widgets {
        widget.rounding = egui::Rounding::ZERO;
        widget.expansion = 0.0;
    }
    visuals.widgets.noninteractive.bg_fill = PANEL;
    visuals.widgets.noninteractive.weak_bg_fill = PANEL;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, IVORY);
    visuals.widgets.inactive.bg_fill = BUTTON;
    visuals.widgets.inactive.weak_bg_fill = BUTTON;
    visuals.widgets.inactive.bg_stroke = Stroke::new(2.0_f32, BORDER);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, IVORY);
    visuals.widgets.hovered.bg_fill = BUTTON_HOVER;
    visuals.widgets.hovered.weak_bg_fill = BUTTON_HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(2.0_f32, GOLD);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, GOLD_LIGHT);
    visuals.widgets.active.bg_fill = BUTTON_PRESSED;
    visuals.widgets.active.weak_bg_fill = BUTTON_PRESSED;
    visuals.widgets.active.bg_stroke = Stroke::new(2.0_f32, GOLD_DARK);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, IVORY);
    visuals.widgets.open.bg_fill = BUTTON_HOVER;
    visuals.widgets.open.weak_bg_fill = BUTTON_HOVER;
    visuals.widgets.open.bg_stroke = Stroke::new(2.0_f32, GOLD);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0_f32, IVORY);

    let mut style = (*ctx.style()).clone();
    style.visuals = visuals;
    style.spacing.button_padding = Vec2::new(8.0, 5.0);
    style.spacing.item_spacing = Vec2::new(6.0, 6.0);
    style.spacing.interact_size = Vec2::new(28.0, 26.0);
    for font in style.text_styles.values_mut() {
        font.family = egui::FontFamily::Monospace;
    }
    ctx.set_style(style);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn button_layout_is_stable_for_icons_and_labels() {
        let icon = pixel_button_layout(0.0, false);
        assert_eq!(icon.width, 28.0);
        assert_eq!(icon.height, 26.0);
        assert_eq!(icon.icon_size, 16.0);
        let labeled = pixel_button_layout(40.0, true);
        assert_eq!(labeled.width, 72.0);
        assert_eq!(labeled.gap, 4.0);
    }

    #[test]
    fn palette_keeps_gold_distinct_from_the_night_background() {
        assert_ne!(GOLD, NIGHT);
        assert!(NIGHT.r() < 40 && NIGHT.b() > NIGHT.r());
        assert!(GOLD.r() > 200 && GOLD.g() > 120 && GOLD.b() < 80);
        assert_ne!(CYAN, ERROR);
        assert_ne!(EMERALD, GOLD);
        assert!(IVORY.r() > 230 && MUTED.r() < IVORY.r());
    }

    #[test]
    fn shade_stays_inside_the_channel_range() {
        let brighter = shade(Color32::from_rgb(250, 10, 0), 20);
        assert_eq!(brighter, Color32::from_rgb(255, 30, 20));
        let darker = shade(Color32::from_rgb(5, 0, 8), -20);
        assert_eq!(darker, Color32::from_rgb(0, 0, 0));
    }
}
