use crate::state::AssistantMood;
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
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let center = rect.center();
    let px_size = size / GRID as f32;

    let accent = match mood {
        AssistantMood::Idle => Color32::from_rgb(56, 189, 248),      // Cyan 8-bit
        AssistantMood::Listening => Color32::from_rgb(244, 63, 94),  // Rose 8-bit
        AssistantMood::Thinking => Color32::from_rgb(168, 85, 247),  // Purple 8-bit
        AssistantMood::Happy => Color32::from_rgb(52, 211, 153),     // Emerald 8-bit
        AssistantMood::Focused => Color32::from_rgb(129, 140, 248),  // Indigo 8-bit
    };

    // Normalized look vector toward the cursor (even outside the window)
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

    // Quantized 1-pixel vertical bobbing for 8-bit feel
    let bob_px = ((time * 3.0).sin() * 0.6).round() as f32;
    let origin = Pos2::new(
        rect.min.x,
        rect.min.y + bob_px * px_size,
    );

    // 3D light direction shifts subtly with the cursor
    let lx = -0.45 + look.x * 0.25;
    let ly = -0.50 + look.y * 0.25;
    let lz = 0.74_f32;
    let l_len = (lx * lx + ly * ly + lz * lz).sqrt();
    let (lx, ly, lz) = (lx / l_len, ly / l_len, lz / l_len);

    // Render 20x20 8-bit sphere with quantized 4-band 3D lighting
    let half = (GRID as f32 - 1.0) * 0.5;
    let radius_g = half - 0.8;

    for gy in 0..GRID {
        for gx in 0..GRID {
            let nx = (gx as f32 - half) / radius_g;
            let ny = (gy as f32 - half) / radius_g;
            let r2 = nx * nx + ny * ny;

            if r2 > 1.05 {
                // Optional subtle pixel pulse aura when listening
                if mood == AssistantMood::Listening && r2 <= 1.25 && ((gx + gy + (time * 8.0) as i32) % 3 == 0) {
                    draw_pixel(&painter, origin, gx, gy, px_size, Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 110));
                }
                continue;
            }

            let color = if r2 > 0.82 {
                // Outer 8-bit pixel rim
                accent
            } else {
                let nz = (1.0 - r2).max(0.0).sqrt();
                let dot = (nx * lx + ny * ly + nz * lz).clamp(0.0, 1.0);
                if dot > 0.86 {
                    Color32::from_rgb(76, 98, 145) // Specular highlight pixel
                } else if dot > 0.58 {
                    Color32::from_rgb(38, 50, 78)  // Lit band
                } else if dot > 0.30 {
                    Color32::from_rgb(22, 30, 48)  // Mid band
                } else {
                    Color32::from_rgb(12, 16, 28)  // Shadow band
                }
            };

            draw_pixel(&painter, origin, gx, gy, px_size, color);
        }
    }

    // Face coordinates on the 20x20 grid shifted by 3D look vector
    let shift_x = (look.x * 2.6).round() as i32;
    let shift_y = (look.y * 2.2).round() as i32;
    let cx = 10 + shift_x;
    let cy = 9 + shift_y;

    let blink = (time % 4.0) > 3.85;

    if blink {
        // 8-bit closed eyes (- -)
        draw_pixel(&painter, origin, cx - 4, cy, px_size, accent);
        draw_pixel(&painter, origin, cx - 3, cy, px_size, accent);
        draw_pixel(&painter, origin, cx + 2, cy, px_size, accent);
        draw_pixel(&painter, origin, cx + 3, cy, px_size, accent);
    } else {
        match mood {
            AssistantMood::Happy => {
                // 8-bit ^ ^ eyes
                for dx in [-3, 2] {
                    draw_pixel(&painter, origin, cx + dx - 1, cy + 1, px_size, accent);
                    draw_pixel(&painter, origin, cx + dx, cy, px_size, accent);
                    draw_pixel(&painter, origin, cx + dx + 1, cy + 1, px_size, accent);
                }
            }
            AssistantMood::Focused => {
                // 8-bit visor bar
                for vx in (cx - 4)..=(cx + 3) {
                    draw_pixel(&painter, origin, vx, cy, px_size, accent);
                    draw_pixel(&painter, origin, vx, cy + 1, px_size, Color32::from_rgb(67, 56, 202));
                }
            }
            AssistantMood::Thinking => {
                let step = ((time * 6.0) as i32) % 3;
                for (i, dx) in [-3, 0, 3].iter().enumerate() {
                    let c = if i as i32 == step { Color32::WHITE } else { accent };
                    draw_pixel(&painter, origin, cx + dx, cy, px_size, c);
                }
            }
            _ => {
                // 2x2 pixel eyes with bright white glint
                for ex in [cx - 4, cx + 2] {
                    draw_pixel(&painter, origin, ex, cy, px_size, accent);
                    draw_pixel(&painter, origin, ex + 1, cy, px_size, accent);
                    draw_pixel(&painter, origin, ex, cy + 1, px_size, accent);
                    draw_pixel(&painter, origin, ex + 1, cy + 1, px_size, Color32::WHITE);
                }
            }
        }
    }

    // 8-bit mouth
    let my = cy + 4;
    match mood {
        AssistantMood::Happy => {
            draw_pixel(&painter, origin, cx - 3, my - 1, px_size, accent);
            draw_pixel(&painter, origin, cx - 2, my, px_size, accent);
            draw_pixel(&painter, origin, cx - 1, my, px_size, accent);
            draw_pixel(&painter, origin, cx, my, px_size, accent);
            draw_pixel(&painter, origin, cx + 1, my, px_size, accent);
            draw_pixel(&painter, origin, cx + 2, my - 1, px_size, accent);
        }
        AssistantMood::Listening => {
            let open = ((time * 8.0) as i32) % 2 == 0;
            draw_pixel(&painter, origin, cx - 1, my, px_size, accent);
            draw_pixel(&painter, origin, cx, my, px_size, accent);
            if open {
                draw_pixel(&painter, origin, cx - 1, my + 1, px_size, accent);
                draw_pixel(&painter, origin, cx, my + 1, px_size, accent);
            }
        }
        _ => {
            draw_pixel(&painter, origin, cx - 2, my, px_size, accent);
            draw_pixel(&painter, origin, cx - 1, my, px_size, accent);
            draw_pixel(&painter, origin, cx, my, px_size, accent);
            draw_pixel(&painter, origin, cx + 1, my, px_size, accent);
        }
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
