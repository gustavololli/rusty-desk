//! Nine-slice oxidized pixel frame shared by all Rusty Desk window modes.
//! Corners keep their shape; only straight center rails stretch.

use eframe::egui::{self, Color32, Pos2, Rect};

use super::retro::{self, BezelSize};

const FRAME_PNG: &[u8] = include_bytes!("../../assets/frames/rusty-oxidized-9slice.png");
// The generated asset has a wide enough corner plate for the first 12%.
const SOURCE_CORNER: f32 = 0.12;

fn corner_points(rect: Rect, size: BezelSize) -> ([f32; 4], [f32; 4]) {
    let desired: f32 = match size {
        BezelSize::Window => 22.0,
        BezelSize::Island => 14.0,
        BezelSize::Notch => 6.0,
    };
    // Always reserve a non-negative center, even on the tiny 88x24 notch.
    let corner = desired
        .min((rect.width() * 0.5).floor())
        .min((rect.height() * 0.5).floor());
    (
        [
            rect.left(),
            rect.left() + corner,
            rect.right() - corner,
            rect.right(),
        ],
        [
            rect.top(),
            rect.top() + corner,
            rect.bottom() - corner,
            rect.bottom(),
        ],
    )
}

fn texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let id = egui::Id::new("rusty-oxidized-frame-9slice-v1");
    if let Some(handle) = ctx.data(|data| data.get_temp::<egui::TextureHandle>(id)) {
        return Some(handle);
    }
    let image = crate::ui::sprites::decode_rgba(FRAME_PNG).ok()?;
    let color_image = egui::ColorImage::from_rgba_unmultiplied(
        [image.width as usize, image.height as usize],
        &image.pixels,
    );
    let handle = ctx.load_texture(
        "rusty-oxidized-frame-9slice-v1",
        color_image,
        egui::TextureOptions::NEAREST,
    );
    ctx.data_mut(|data| data.insert_temp(id, handle.clone()));
    Some(handle)
}

pub fn paint_frame(painter: &egui::Painter, rect: Rect, ctx: &egui::Context, size: BezelSize) {
    if rect.width() < 12.0 || rect.height() < 12.0 {
        return;
    }
    let Some(handle) = texture(ctx) else {
        retro::paint_oxidized_bezel(painter, rect, size);
        return;
    };
    let (xs, ys) = corner_points(rect, size);
    let us = [0.0, SOURCE_CORNER, 1.0 - SOURCE_CORNER, 1.0];
    let vs = us;
    for row in 0..3 {
        for col in 0..3 {
            if row == 1 && col == 1 {
                continue; // Transparent center; never cover the application's controls.
            }
            let destination = Rect::from_min_max(
                Pos2::new(xs[col], ys[row]),
                Pos2::new(xs[col + 1], ys[row + 1]),
            );
            if destination.width() <= 0.0 || destination.height() <= 0.0 {
                continue;
            }
            let uv = Rect::from_min_max(
                Pos2::new(us[col], vs[row]),
                Pos2::new(us[col + 1], vs[row + 1]),
            );
            painter.image(handle.id(), destination, uv, Color32::WHITE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Vec2;

    #[test]
    fn generated_border_has_transparent_center() {
        let image = crate::ui::sprites::decode_rgba(FRAME_PNG).expect("valid PNG");
        assert_eq!(image.width, image.height);
        let x = (image.width / 2) as usize;
        let y = (image.height / 2) as usize;
        let alpha = image.pixels[(y * image.width as usize + x) * 4 + 3];
        assert_eq!(alpha, 0, "9-slice center should never cover controls");
    }

    #[test]
    fn corners_fit_expanded_island_and_hidden_notch() {
        for (w, h, size) in [
            (580.0, 820.0, BezelSize::Window),
            (440.0, 100.0, BezelSize::Island),
            (88.0, 24.0, BezelSize::Notch),
            (28.0, 28.0, BezelSize::Notch),
        ] {
            let rect = Rect::from_min_size(Pos2::new(13.0, 8.0), Vec2::new(w, h));
            let (xs, ys) = corner_points(rect, size);
            assert!(xs.windows(2).all(|pair| pair[0] <= pair[1]));
            assert!(ys.windows(2).all(|pair| pair[0] <= pair[1]));
            assert_eq!(xs[0], rect.left());
            assert_eq!(xs[3], rect.right());
            assert_eq!(ys[0], rect.top());
            assert_eq!(ys[3], rect.bottom());
        }
    }
}
