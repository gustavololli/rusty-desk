use crate::state::AssistantMood;
use crate::ui::pet::animation::{
    AnimationFrame, AnimationState, EyeExpression, MouthExpression, ParticleKind,
};
use eframe::egui::{self, Color32, Pos2, Rect, Rounding, Vec2};

const GRID: i32 = 20;

/// Draws an 8-bit pixelated 3D-shaded sphere avatar that tracks the mouse cursor.
pub fn draw_avatar(
    ui: &mut egui::Ui,
    size: f32,
    mood: AssistantMood,
    time: f64,
    look_target: Option<Pos2>,
) {
    let blink = (time % 4.0) > 3.85;
    let bob_px = ((time * 3.0).sin() * 0.6).round() as i32;
    let eye = if blink {
        EyeExpression::Closed
    } else {
        match mood {
            AssistantMood::Happy => EyeExpression::Happy,
            AssistantMood::Focused => EyeExpression::Squint,
            _ => EyeExpression::Normal,
        }
    };
    let mouth = match mood {
        AssistantMood::Happy => MouthExpression::Smile,
        AssistantMood::Listening => {
            if ((time * 8.0) as i32) % 2 == 0 {
                MouthExpression::TalkOpen
            } else {
                MouthExpression::Neutral
            }
        }
        _ => MouthExpression::Neutral,
    };
    let (look_override_x, look_override_y) = if mood == AssistantMood::Thinking {
        (Some(1), Some(-1))
    } else {
        (None, None)
    };
    let particle = match mood {
        AssistantMood::Listening => ParticleKind::SoundPulse,
        AssistantMood::Thinking => ParticleKind::ThoughtDots,
        _ => ParticleKind::None,
    };
    let frame = AnimationFrame {
        state: AnimationState::Idle,
        bob_px,
        offset_x_px: 0,
        offset_y_px: 0,
        look_override_x,
        look_override_y,
        glitch_scanline: None,
        eye,
        mouth,
        blush: mood == AssistantMood::Happy,
        particle,
        particle_step: ((time * 8.0) as u8) % 8,
        accent_override: None,
        headphones: false,
    };
    draw_avatar_frame(ui, size, mood, time, look_target, &frame);
}

/// Draws Rusty with full procedural animation state while preserving his 20x20 8-bit identity.
pub fn draw_avatar_frame(
    ui: &mut egui::Ui,
    size: f32,
    mood: AssistantMood,
    time: f64,
    look_target: Option<Pos2>,
    frame: &AnimationFrame,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let center = rect.center();
    let px_size = size / GRID as f32;

    let accent = match frame.accent_override {
        Some([r, g, b]) => Color32::from_rgb(r, g, b),
        None => mood_accent_color(mood),
    };

    let look = match look_target {
        Some(pos) => {
            let delta = pos - center;
            let dist = delta.length();
            if dist < 1.0 {
                Vec2::ZERO
            } else {
                let strength = (dist / 160.0).clamp(0.0, 1.0);
                (delta / dist) * strength
            }
        }
        None => Vec2::ZERO,
    };

    let origin = Pos2::new(
        rect.min.x + frame.offset_x_px as f32 * px_size,
        rect.min.y + (frame.bob_px + frame.offset_y_px) as f32 * px_size,
    );

    let lx = -0.45 + look.x * 0.25;
    let ly = -0.50 + look.y * 0.25;
    let lz = 0.74_f32;
    let l_len = (lx * lx + ly * ly + lz * lz).sqrt();
    let (lx, ly, lz) = (lx / l_len, ly / l_len, lz / l_len);

    let half = (GRID as f32 - 1.0) * 0.5;
    let radius_g = half - 0.8;

    for gy in 0..GRID {
        let row_shift = match frame.glitch_scanline {
            Some(line) if (gy - line).abs() <= 1 => {
                if gy % 2 == 0 {
                    1
                } else {
                    -1
                }
            }
            _ => 0,
        };
        for gx in 0..GRID {
            let nx = (gx as f32 - half) / radius_g;
            let ny = (gy as f32 - half) / radius_g;
            let r2 = nx * nx + ny * ny;

            if r2 > 1.05 {
                if frame.particle == ParticleKind::SoundPulse
                    && r2 <= 1.25
                    && ((gx + gy + (time * 8.0) as i32) % 3 == 0)
                {
                    draw_pixel(
                        &painter,
                        origin,
                        gx + row_shift,
                        gy,
                        px_size,
                        Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 110),
                    );
                }
                continue;
            }

            let color = if r2 > 0.82 {
                accent
            } else {
                let nz = (1.0 - r2).max(0.0).sqrt();
                let dot = (nx * lx + ny * ly + nz * lz).clamp(0.0, 1.0);
                if dot > 0.86 {
                    Color32::from_rgb(76, 98, 145)
                } else if dot > 0.58 {
                    Color32::from_rgb(38, 50, 78)
                } else if dot > 0.30 {
                    Color32::from_rgb(22, 30, 48)
                } else {
                    Color32::from_rgb(12, 16, 28)
                }
            };

            draw_pixel(&painter, origin, gx + row_shift, gy, px_size, color);
        }
    }

    let shift_x = frame
        .look_override_x
        .unwrap_or_else(|| (look.x * 2.6).round() as i32);
    let shift_y = frame
        .look_override_y
        .unwrap_or_else(|| (look.y * 2.2).round() as i32);
    let cx = 10 + shift_x;
    let cy = 9 + shift_y;

    draw_eyes(&painter, origin, cx, cy, px_size, accent, frame.eye);
    if frame.blush {
        draw_blush(&painter, origin, cx, cy, px_size);
    }
    draw_mouth(&painter, origin, cx, cy + 4, px_size, accent, frame.mouth);
    if frame.headphones {
        draw_headphones(&painter, origin, px_size, accent);
    }
    draw_particles(
        &painter,
        origin,
        px_size,
        accent,
        frame.particle,
        frame.particle_step,
    );

    response
}

pub fn mood_accent_color(mood: AssistantMood) -> Color32 {
    match mood {
        AssistantMood::Idle => Color32::from_rgb(56, 189, 248),
        AssistantMood::Listening => Color32::from_rgb(244, 63, 94),
        AssistantMood::Thinking => Color32::from_rgb(168, 85, 247),
        AssistantMood::Happy => Color32::from_rgb(52, 211, 153),
        AssistantMood::Focused => Color32::from_rgb(129, 140, 248),
    }
}

fn draw_eyes(
    painter: &egui::Painter,
    origin: Pos2,
    cx: i32,
    cy: i32,
    px_size: f32,
    accent: Color32,
    eye: EyeExpression,
) {
    match eye {
        EyeExpression::Closed => {
            draw_pixel(painter, origin, cx - 4, cy, px_size, accent);
            draw_pixel(painter, origin, cx - 3, cy, px_size, accent);
            draw_pixel(painter, origin, cx + 2, cy, px_size, accent);
            draw_pixel(painter, origin, cx + 3, cy, px_size, accent);
        }
        EyeExpression::Happy => {
            for dx in [-3, 2] {
                draw_pixel(painter, origin, cx + dx - 1, cy + 1, px_size, accent);
                draw_pixel(painter, origin, cx + dx, cy, px_size, accent);
                draw_pixel(painter, origin, cx + dx + 1, cy + 1, px_size, accent);
            }
        }
        EyeExpression::Wink => {
            // Left eye: elongated 2x3 normal eye
            draw_elongated_eye(painter, origin, cx - 4, cy, px_size, accent, 3);
            // Right eye: cheerful wink arch
            draw_pixel(painter, origin, cx + 1, cy + 1, px_size, accent);
            draw_pixel(painter, origin, cx + 2, cy, px_size, accent);
            draw_pixel(painter, origin, cx + 3, cy + 1, px_size, accent);
        }
        EyeExpression::Curious => {
            // Asymmetrical expressive eyes (one 2x2, one 2x3)
            draw_elongated_eye(painter, origin, cx - 4, cy, px_size, accent, 2);
            draw_elongated_eye(painter, origin, cx + 2, cy, px_size, accent, 3);
        }
        EyeExpression::Squint => {
            // Narrowed focused slits with glowing center
            for ex in [cx - 4, cx + 1] {
                draw_pixel(painter, origin, ex, cy, px_size, accent);
                draw_pixel(painter, origin, ex + 1, cy, px_size, Color32::WHITE);
                draw_pixel(painter, origin, ex + 2, cy, px_size, accent);
            }
        }
        EyeExpression::WideAlert => {
            // Extra-tall 2x4 wide-eyed surprise
            for ex in [cx - 4, cx + 2] {
                draw_elongated_eye(painter, origin, ex, cy, px_size, accent, 4);
            }
        }
        EyeExpression::ErrorCross => {
            for ex in [cx - 4, cx + 2] {
                draw_pixel(painter, origin, ex, cy, px_size, accent);
                draw_pixel(painter, origin, ex + 1, cy + 1, px_size, accent);
                draw_pixel(painter, origin, ex + 1, cy, px_size, Color32::WHITE);
                draw_pixel(painter, origin, ex, cy + 1, px_size, accent);
            }
        }
        EyeExpression::Dizzy => {
            for ex in [cx - 4, cx + 2] {
                draw_pixel(painter, origin, ex, cy - 1, px_size, accent);
                draw_pixel(painter, origin, ex + 1, cy, px_size, Color32::WHITE);
                draw_pixel(painter, origin, ex, cy + 1, px_size, accent);
                draw_pixel(painter, origin, ex - 1, cy, px_size, accent);
            }
        }
        EyeExpression::Normal => {
            // Grokbot-style elongated 2x3 vertical eyes
            for ex in [cx - 4, cx + 2] {
                draw_elongated_eye(painter, origin, ex, cy, px_size, accent, 3);
            }
        }
    }
}

/// Draws a 2px-wide vertical elongated eye of `height` (2, 3, or 4 px) with a top-right glint.
fn draw_elongated_eye(
    painter: &egui::Painter,
    origin: Pos2,
    ex: i32,
    cy: i32,
    px_size: f32,
    accent: Color32,
    height: i32,
) {
    let (y_start, y_end) = match height {
        2 => (cy, cy + 1),
        4 => (cy - 1, cy + 2),
        _ => (cy - 1, cy + 1),
    };
    for gy in y_start..=y_end {
        for dx in 0..=1 {
            let c = if dx == 1 && gy == y_start {
                Color32::WHITE
            } else {
                accent
            };
            draw_pixel(painter, origin, ex + dx, gy, px_size, c);
        }
    }
}

fn draw_blush(painter: &egui::Painter, origin: Pos2, cx: i32, cy: i32, px_size: f32) {
    let pink = Color32::from_rgba_unmultiplied(244, 114, 182, 180);
    draw_pixel(painter, origin, cx - 5, cy + 2, px_size, pink);
    draw_pixel(painter, origin, cx - 4, cy + 2, px_size, pink);
    draw_pixel(painter, origin, cx + 3, cy + 2, px_size, pink);
    draw_pixel(painter, origin, cx + 4, cy + 2, px_size, pink);
}

fn draw_mouth(
    painter: &egui::Painter,
    origin: Pos2,
    cx: i32,
    my: i32,
    px_size: f32,
    accent: Color32,
    mouth: MouthExpression,
) {
    match mouth {
        MouthExpression::Smile => {
            draw_pixel(painter, origin, cx - 3, my - 1, px_size, accent);
            draw_pixel(painter, origin, cx - 2, my, px_size, accent);
            draw_pixel(painter, origin, cx - 1, my, px_size, accent);
            draw_pixel(painter, origin, cx, my, px_size, accent);
            draw_pixel(painter, origin, cx + 1, my, px_size, accent);
            draw_pixel(painter, origin, cx + 2, my - 1, px_size, accent);
        }
        MouthExpression::TalkOpen => {
            draw_pixel(painter, origin, cx - 1, my, px_size, accent);
            draw_pixel(painter, origin, cx, my, px_size, accent);
            draw_pixel(painter, origin, cx - 1, my + 1, px_size, accent);
            draw_pixel(painter, origin, cx, my + 1, px_size, accent);
        }
        MouthExpression::SleepSmall => {
            draw_pixel(painter, origin, cx - 1, my, px_size, accent);
            draw_pixel(painter, origin, cx, my, px_size, accent);
        }
        MouthExpression::Surprised => {
            draw_pixel(painter, origin, cx - 1, my - 1, px_size, accent);
            draw_pixel(painter, origin, cx, my - 1, px_size, accent);
            draw_pixel(painter, origin, cx - 1, my, px_size, accent);
            draw_pixel(painter, origin, cx, my, px_size, accent);
        }
        MouthExpression::Frown => {
            draw_pixel(painter, origin, cx - 2, my + 1, px_size, accent);
            draw_pixel(painter, origin, cx - 1, my, px_size, accent);
            draw_pixel(painter, origin, cx, my, px_size, accent);
            draw_pixel(painter, origin, cx + 1, my + 1, px_size, accent);
        }
        MouthExpression::Neutral => {
            draw_pixel(painter, origin, cx - 2, my, px_size, accent);
            draw_pixel(painter, origin, cx - 1, my, px_size, accent);
            draw_pixel(painter, origin, cx, my, px_size, accent);
            draw_pixel(painter, origin, cx + 1, my, px_size, accent);
        }
    }
}

fn draw_particles(
    painter: &egui::Painter,
    origin: Pos2,
    px_size: f32,
    accent: Color32,
    particle: ParticleKind,
    step: u8,
) {
    match particle {
        ParticleKind::None => {}
        ParticleKind::SoundPulse => {
            // Side equalizer bars inspired by voice/listening indicators
            let half_h = match step % 4 {
                0 => 1,
                1 => 2,
                2 => 3,
                _ => 2,
            };
            for dy in -half_h..=half_h {
                draw_pixel(painter, origin, 0, 9 + dy, px_size, accent);
                draw_pixel(painter, origin, 19, 9 + dy, px_size, accent);
            }
        }
        ParticleKind::ThoughtDots => {
            let active = (step / 2) % 3;
            draw_pixel(painter, origin, 13, 3, px_size, accent);
            for (i, gx) in [15, 17, 19].iter().enumerate() {
                let c = if i as u8 == active {
                    Color32::WHITE
                } else {
                    accent
                };
                draw_pixel(painter, origin, *gx, 1, px_size, c);
            }
        }
        ParticleKind::Sparkles => {
            let gold = Color32::from_rgb(250, 204, 21);
            let coords = match step % 4 {
                0 => [(2, 2), (17, 3), (3, 16)],
                1 => [(3, 1), (16, 2), (17, 15)],
                2 => [(1, 4), (18, 4), (2, 15)],
                _ => [(2, 3), (17, 1), (16, 17)],
            };
            for (px, py) in coords {
                draw_pixel(painter, origin, px, py, px_size, gold);
            }
        }
        ParticleKind::SleepZzz => {
            let z_color = Color32::from_rgb(147, 197, 253);
            let dy = (step / 2) as i32 % 2;
            // Small pixel 'z' in top-right corner
            draw_pixel(painter, origin, 15, 2 - dy, px_size, z_color);
            draw_pixel(painter, origin, 16, 2 - dy, px_size, z_color);
            draw_pixel(painter, origin, 15, 3 - dy, px_size, z_color);
            draw_pixel(painter, origin, 15, 4 - dy, px_size, z_color);
            draw_pixel(painter, origin, 16, 4 - dy, px_size, z_color);
        }
        ParticleKind::Exclamation => {
            let c = if step.is_multiple_of(2) {
                Color32::WHITE
            } else {
                accent
            };
            draw_pixel(painter, origin, 17, 1, px_size, c);
            draw_pixel(painter, origin, 17, 2, px_size, c);
            draw_pixel(painter, origin, 17, 4, px_size, c);
        }
        ParticleKind::Question => {
            let c = Color32::from_rgb(252, 165, 165);
            draw_pixel(painter, origin, 16, 1, px_size, c);
            draw_pixel(painter, origin, 17, 1, px_size, c);
            draw_pixel(painter, origin, 17, 2, px_size, c);
            draw_pixel(painter, origin, 16, 4, px_size, c);
        }
        ParticleKind::MusicNotes => {
            let note_a = Color32::from_rgb(52, 211, 153);
            let note_b = Color32::from_rgb(250, 204, 21);
            let dy_a = ((step / 2) % 2) as i32;
            let dy_b = (((step / 2) + 1) % 2) as i32;

            // Right-side eighth note ♪
            draw_pixel(painter, origin, 17, 1 - dy_a, px_size, note_a);
            draw_pixel(painter, origin, 18, 1 - dy_a, px_size, note_a);
            draw_pixel(painter, origin, 17, 2 - dy_a, px_size, note_a);
            draw_pixel(painter, origin, 17, 3 - dy_a, px_size, note_a);
            draw_pixel(painter, origin, 16, 4 - dy_a, px_size, note_a);
            draw_pixel(painter, origin, 17, 4 - dy_a, px_size, note_a);

            // Left-side small note
            draw_pixel(painter, origin, 2, 2 - dy_b, px_size, note_b);
            draw_pixel(painter, origin, 3, 2 - dy_b, px_size, note_b);
            draw_pixel(painter, origin, 2, 3 - dy_b, px_size, note_b);
            draw_pixel(painter, origin, 1, 4 - dy_b, px_size, note_b);
            draw_pixel(painter, origin, 2, 4 - dy_b, px_size, note_b);
        }
    }
}

fn draw_headphones(painter: &egui::Painter, origin: Pos2, px_size: f32, accent: Color32) {
    let band = Color32::from_rgb(71, 85, 105);
    let band_hi = Color32::from_rgb(148, 163, 184);
    let cup_outer = Color32::from_rgb(30, 41, 59);
    let cup_pad = Color32::from_rgb(51, 65, 85);

    // Top headband arc
    for gx in 5..=14 {
        let c = if (7..=12).contains(&gx) {
            band_hi
        } else {
            band
        };
        draw_pixel(painter, origin, gx, 0, px_size, c);
    }
    draw_pixel(painter, origin, 4, 1, px_size, band);
    draw_pixel(painter, origin, 15, 1, px_size, band);
    draw_pixel(painter, origin, 3, 2, px_size, band);
    draw_pixel(painter, origin, 16, 2, px_size, band);
    for gy in 3..=6 {
        draw_pixel(painter, origin, 2, gy, px_size, band);
        draw_pixel(painter, origin, 17, gy, px_size, band);
    }

    // Left and right earcups with accent LED stripe
    for gy in 7..=12 {
        draw_pixel(painter, origin, 1, gy, px_size, cup_outer);
        draw_pixel(painter, origin, 2, gy, px_size, cup_pad);
        draw_pixel(painter, origin, 18, gy, px_size, cup_outer);
        draw_pixel(painter, origin, 17, gy, px_size, cup_pad);
    }
    for gy in 8..=11 {
        draw_pixel(painter, origin, 0, gy, px_size, accent);
        draw_pixel(painter, origin, 19, gy, px_size, accent);
    }
}

#[inline]
fn draw_pixel(painter: &egui::Painter, origin: Pos2, gx: i32, gy: i32, px: f32, color: Color32) {
    if !(0..GRID).contains(&gx) || !(0..GRID).contains(&gy) {
        return;
    }
    let min = Pos2::new(origin.x + gx as f32 * px, origin.y + gy as f32 * px);
    let max = Pos2::new(min.x + px + 0.35, min.y + px + 0.35);
    painter.rect_filled(Rect::from_min_max(min, max), Rounding::ZERO, color);
}
