//! Sprite sheet for the 16-bit Rusty robot.
//!
//! `AnimationController` decides the state. This module decides which cell of
//! the embedded sheet represents that state, loads the sheet once, and paints
//! it with nearest-neighbor sampling so the pixels stay square.

use crate::ui::pet::animation::{AnimationFrame, AnimationState};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Vec2};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::io::Cursor;
use std::sync::OnceLock;

pub const NATIVE_SIZE: u32 = 96;
pub const FRAMES_PER_STATE: u32 = 4;
pub const SHEET_COLUMNS: u32 = 4;
pub const SHEET_ROWS: u32 = 14;
pub const SPRITE_FPS: f32 = 8.0;
pub const FRAME_MS: u32 = 125;

const SHEET_PNG: &[u8] = include_bytes!("../../assets/rusty-16bit/rusty-spritesheet.png");
const MANIFEST_JSON: &str = include_str!("../../assets/rusty-16bit/manifest.json");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct SpriteManifest {
    pub name: String,
    pub version: u32,
    pub frame_width: u32,
    pub frame_height: u32,
    pub fps: u32,
    pub columns: u32,
    pub rows: u32,
    pub sheet: String,
    pub format: String,
    pub states: BTreeMap<String, ManifestState>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ManifestState {
    pub row: u32,
    pub frames: Vec<String>,
    pub frame_duration_ms: u32,
}

pub fn parse_manifest(json: &str) -> Result<SpriteManifest, String> {
    serde_json::from_str(json).map_err(|err| format!("manifest JSON: {err}"))
}

pub fn embedded_manifest() -> Result<SpriteManifest, String> {
    parse_manifest(MANIFEST_JSON)
}

/// Checks that the manifest describes exactly the packed robot sheet.
pub fn validate_manifest(manifest: &SpriteManifest) -> Result<(), String> {
    let mut errors = Vec::new();
    if manifest.version == 0 {
        errors.push("version must be at least 1".to_string());
    }
    if manifest.frame_width != NATIVE_SIZE || manifest.frame_height != NATIVE_SIZE {
        errors.push(format!(
            "frame size {}x{}, expected {NATIVE_SIZE}x{NATIVE_SIZE}",
            manifest.frame_width, manifest.frame_height
        ));
    }
    if manifest.columns != SHEET_COLUMNS || manifest.rows != SHEET_ROWS {
        errors.push(format!(
            "sheet grid {}x{}, expected {SHEET_COLUMNS}x{SHEET_ROWS}",
            manifest.columns, manifest.rows
        ));
    }
    if manifest.fps != SPRITE_FPS as u32 {
        errors.push(format!(
            "fps {}, expected {}",
            manifest.fps, SPRITE_FPS as u32
        ));
    }
    if manifest.sheet != "rusty-spritesheet.png" {
        errors.push(format!("unexpected sheet name {}", manifest.sheet));
    }
    if manifest.states.len() != AnimationState::ALL.len() {
        errors.push(format!(
            "expected {} states, found {}",
            AnimationState::ALL.len(),
            manifest.states.len()
        ));
    }

    let mut seen_rows = Vec::new();
    for state in AnimationState::ALL {
        let key = state.manifest_key();
        let Some(entry) = manifest.states.get(key) else {
            errors.push(format!("missing state {key}"));
            continue;
        };
        let expected_row = state_row(state);
        if entry.row != expected_row {
            errors.push(format!(
                "{key} row is {}, expected {expected_row}",
                entry.row
            ));
        }
        if seen_rows.contains(&entry.row) {
            errors.push(format!("duplicated row {}", entry.row));
        }
        seen_rows.push(entry.row);
        if entry.frames.len() != FRAMES_PER_STATE as usize {
            errors.push(format!(
                "{key} has {} frames, expected {FRAMES_PER_STATE}",
                entry.frames.len()
            ));
        }
        if entry.frame_duration_ms != FRAME_MS {
            errors.push(format!(
                "{key} frame duration {} ms, expected {FRAME_MS}",
                entry.frame_duration_ms
            ));
        }
        for (index, path) in entry.frames.iter().enumerate() {
            let expected = frame_path(state, index as u32);
            if path != &expected {
                errors.push(format!(
                    "{key} frame {index} is {path}, expected {expected}"
                ));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

pub const fn state_row(state: AnimationState) -> u32 {
    match state {
        AnimationState::Idle => 0,
        AnimationState::Blink => 1,
        AnimationState::LookAround => 2,
        AnimationState::Sleep => 3,
        AnimationState::WakeUp => 4,
        AnimationState::Thinking => 5,
        AnimationState::Listening => 6,
        AnimationState::Happy => 7,
        AnimationState::Celebrate => 8,
        AnimationState::Focused => 9,
        AnimationState::Error => 10,
        AnimationState::Glitch => 11,
        AnimationState::PomodoroFinished => 12,
        AnimationState::ListeningMusic => 13,
    }
}

pub fn frame_path(state: AnimationState, index: u32) -> String {
    format!("frames/{}/{:02}.png", state.folder(), index)
}

/// Still frame that reads as the state. Blink uses the open-eyed pose so a
/// reduced-motion frame never freezes mid-blink.
pub const fn representative_frame(state: AnimationState) -> u32 {
    match state {
        AnimationState::Blink => 0,
        _ => 0,
    }
}

/// Frame column for `elapsed_secs` at `fps`. Reduced motion stays on the
/// representative still. Invalid numbers also fall back to that still.
pub fn select_frame(
    elapsed_secs: f32,
    fps: f32,
    frame_count: u32,
    reduce_animations: bool,
    state: AnimationState,
) -> u32 {
    if reduce_animations || frame_count == 0 || !fps.is_finite() || fps <= 0.0 {
        return representative_frame(state);
    }
    if !elapsed_secs.is_finite() || elapsed_secs < 0.0 {
        return representative_frame(state);
    }
    let index = (elapsed_secs * fps).floor();
    if !index.is_finite() || index < 0.0 {
        return representative_frame(state);
    }
    (index as u32) % frame_count
}

/// Which sheet cell to show. While Rusty is idle and cursor following is on,
/// a clear left/right glance uses the existing LookAround frames.
pub fn presented_cell(
    state: AnimationState,
    elapsed_ms: u32,
    reduce_animations: bool,
    look_x: Option<f32>,
) -> (AnimationState, u32) {
    let elapsed_secs = elapsed_ms as f32 / 1000.0;
    let frame = select_frame(
        elapsed_secs,
        SPRITE_FPS,
        FRAMES_PER_STATE,
        reduce_animations,
        state,
    );
    if reduce_animations {
        return (state, frame);
    }
    if state == AnimationState::Idle {
        if let Some(x) = look_x {
            if x <= -0.35 {
                return (AnimationState::LookAround, 0);
            }
            if x >= 0.35 {
                return (AnimationState::LookAround, 2);
            }
        }
    }
    (state, frame)
}

/// Largest integer multiple, or exact integer divisor, of `native` that fits in `slot`.
pub fn fit_integer_scale(slot: f32, native: f32) -> f32 {
    if !slot.is_finite() || !native.is_finite() || slot <= 0.0 || native <= 0.0 {
        return 0.0;
    }
    let up = (slot / native).floor();
    if up >= 1.0 {
        return native * up;
    }
    let divisor = (native / slot).ceil().max(1.0);
    native / divisor
}

/// Prefer the integer fit when it still fills most of the slot.
/// Smaller pet presets keep their configured size and rely on nearest filtering.
pub fn display_side(slot: f32, native: f32) -> f32 {
    let fitted = fit_integer_scale(slot, native);
    if slot > 0.0 && fitted / slot >= 0.85 {
        fitted
    } else {
        slot
    }
}

/// UV rect of one cell, inset by half a texel so nearest sampling does not bleed.
pub fn frame_uv(
    column: u32,
    row: u32,
    frame: u32,
    sheet_width: u32,
    sheet_height: u32,
) -> [f32; 4] {
    let x = column.saturating_mul(frame);
    let y = row.saturating_mul(frame);
    let width = sheet_width.max(1) as f32;
    let height = sheet_height.max(1) as f32;
    let u0 = (x as f32 + 0.5) / width;
    let v0 = (y as f32 + 0.5) / height;
    let u1 = (x as f32 + frame as f32 - 0.5) / width;
    let v1 = (y as f32 + frame as f32 - 0.5) / height;
    [u0, v0, u1, v1]
}

pub fn decode_rgba(bytes: &[u8]) -> Result<DecodedImage, String> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    // ImageMagick stores these sprites as indexed PNGs. Expand the palette
    // (and tRNS) so the renderer always sees straight RGBA.
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().map_err(|err| err.to_string())?;
    let buffer_size = reader
        .output_buffer_size()
        .ok_or("PNG output size is not known yet")?;
    let mut buffer = vec![0; buffer_size];
    let info = reader
        .next_frame(&mut buffer)
        .map_err(|err| err.to_string())?;
    let src = &buffer[..info.buffer_size()];
    let pixels = match info.color_type {
        png::ColorType::Rgba => src.to_vec(),
        png::ColorType::Rgb => {
            let mut expanded = Vec::with_capacity(src.len() / 3 * 4);
            for chunk in src.as_chunks::<3>().0 {
                expanded.extend_from_slice(&[chunk[0], chunk[1], chunk[2], 255]);
            }
            expanded
        }
        other => return Err(format!("unsupported PNG color type {other:?}")),
    };
    let expected = (info.width as usize) * (info.height as usize) * 4;
    if pixels.len() != expected {
        return Err(format!(
            "decoded {} bytes, expected {expected}",
            pixels.len()
        ));
    }
    Ok(DecodedImage {
        width: info.width,
        height: info.height,
        pixels,
    })
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn crop_frame(
    sheet: &DecodedImage,
    frame: u32,
    column: u32,
    row: u32,
) -> Result<Vec<u8>, String> {
    let width = frame as usize;
    let height = frame as usize;
    let origin_x = column as usize * width;
    let origin_y = row as usize * height;
    if origin_x + width > sheet.width as usize || origin_y + height > sheet.height as usize {
        return Err("frame is outside the sheet".to_string());
    }
    let mut out = vec![0; width * height * 4];
    for y in 0..height {
        let src = ((origin_y + y) * sheet.width as usize + origin_x) * 4;
        let dst = y * width * 4;
        out[dst..dst + width * 4].copy_from_slice(&sheet.pixels[src..src + width * 4]);
    }
    Ok(out)
}

/// Integer nearest-neighbor scale. Every source pixel becomes a `scale` square.
#[cfg_attr(not(test), allow(dead_code))]
pub fn scale_nearest(src: &[u8], width: u32, height: u32, scale: u32) -> Result<Vec<u8>, String> {
    if scale == 0 {
        return Err("scale must be at least 1".to_string());
    }
    let expected = (width as usize) * (height as usize) * 4;
    if src.len() != expected {
        return Err(format!("rgba len {}, expected {expected}", src.len()));
    }
    let dst_w = width as usize * scale as usize;
    let dst_h = height as usize * scale as usize;
    let mut out = vec![0; dst_w * dst_h * 4];
    for y in 0..dst_h {
        let sy = y / scale as usize;
        for x in 0..dst_w {
            let sx = x / scale as usize;
            let from = (sy * width as usize + sx) * 4;
            let to = (y * dst_w + x) * 4;
            out[to..to + 4].copy_from_slice(&src[from..from + 4]);
        }
    }
    Ok(out)
}

fn decoded_sheet() -> Option<&'static DecodedImage> {
    static SHEET: OnceLock<Option<DecodedImage>> = OnceLock::new();
    SHEET
        .get_or_init(|| {
            if let Err(err) = embedded_manifest().and_then(|manifest| validate_manifest(&manifest))
            {
                eprintln!("Rusty sprite manifest: {err}");
            }
            decode_rgba(SHEET_PNG).ok()
        })
        .as_ref()
}

fn sheet_texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let id = egui::Id::new("rusty-16bit-sheet");
    if let Some(handle) = ctx.data(|data| data.get_temp::<egui::TextureHandle>(id)) {
        return Some(handle);
    }
    let decoded = decoded_sheet()?;
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [decoded.width as usize, decoded.height as usize],
        &decoded.pixels,
    );
    let handle = ctx.load_texture("rusty-16bit-sheet", image, egui::TextureOptions::NEAREST);
    ctx.data_mut(|data| data.insert_temp(id, handle.clone()));
    Some(handle)
}

pub fn paint_rusty(
    painter: &egui::Painter,
    rect: Rect,
    ctx: &egui::Context,
    frame: &AnimationFrame,
    reduce_animations: bool,
    look_x: Option<f32>,
) {
    // Animated gaze settles before head rotation; existing look sprites stay intact.
    let gaze = if !reduce_animations && frame.state == AnimationState::Idle {
        if frame.motion.gaze_x.abs() > 0.35 {
            Some(frame.motion.gaze_x)
        } else {
            look_x
        }
    } else {
        look_x
    };
    let (state, column) = presented_cell(frame.state, frame.elapsed_ms, reduce_animations, gaze);
    let Some(texture) = sheet_texture(ctx) else {
        painter.rect_filled(rect, 0.0, Color32::from_rgb(242, 160, 21));
        return;
    };
    let uv = frame_uv(
        column,
        state_row(state),
        NATIVE_SIZE,
        SHEET_COLUMNS * NATIVE_SIZE,
        SHEET_ROWS * NATIVE_SIZE,
    );
    let pose = frame.motion;
    if reduce_animations
        || (pose.tilt.abs() < 0.01
            && pose.x.abs() < 0.01
            && pose.y.abs() < 0.01
            && (pose.scale_x - 1.0).abs() < 0.001
            && (pose.scale_y - 1.0).abs() < 0.001)
    {
        painter.image(
            texture.id(),
            rect,
            Rect::from_min_max(Pos2::new(uv[0], uv[1]), Pos2::new(uv[2], uv[3])),
            Color32::WHITE,
        );
        return;
    }

    // Rotate the textured quad, not the PNG itself: nearest filtering preserves
    // the 16-bit pixels and all original state sprite frames remain available.
    let pixel = rect.width() / NATIVE_SIZE as f32;
    let center = rect.center() + Vec2::new(pose.x * pixel, pose.y * pixel);
    let rad = pose.tilt.to_radians();
    let (sin, cos) = rad.sin_cos();
    let half = rect.width() * 0.5;
    let mut mesh = egui::Mesh::with_texture(texture.id());
    for (sx, sy, u, v) in [
        (-1.0, -1.0, uv[0], uv[1]),
        (1.0, -1.0, uv[2], uv[1]),
        (1.0, 1.0, uv[2], uv[3]),
        (-1.0, 1.0, uv[0], uv[3]),
    ] {
        let x = sx * half * pose.scale_x;
        let y = sy * half * pose.scale_y;
        mesh.vertices.push(egui::epaint::Vertex {
            pos: center + Vec2::new(x * cos - y * sin, x * sin + y * cos),
            uv: Pos2::new(u, v),
            color: Color32::WHITE,
        });
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    painter.add(egui::Shape::mesh(mesh));
}

pub fn draw_rusty(
    ui: &mut egui::Ui,
    slot: f32,
    frame: &AnimationFrame,
    reduce_animations: bool,
    look_pos: Option<Pos2>,
    follow_cursor: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(slot), Sense::click_and_drag());
    let look_x = if follow_cursor {
        look_pos.map(|pos| ((pos.x - rect.center().x) / 160.0).clamp(-1.0, 1.0))
    } else {
        None
    };
    let side = display_side(slot, NATIVE_SIZE as f32);
    let draw_rect = Rect::from_center_size(rect.center(), Vec2::splat(side));
    paint_rusty(
        ui.painter(),
        draw_rect,
        ui.ctx(),
        frame,
        reduce_animations,
        look_x,
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn asset_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/rusty-16bit")
    }

    #[test]
    fn manifest_parses_and_matches_every_animation_state() {
        let manifest = embedded_manifest().expect("embedded manifest");
        validate_manifest(&manifest).expect("manifest contract");
        let from_disk = std::fs::read_to_string(asset_root().join("manifest.json")).unwrap();
        assert_eq!(from_disk, MANIFEST_JSON);
    }

    #[test]
    fn manifest_rejects_a_missing_state() {
        let mut manifest = embedded_manifest().unwrap();
        manifest.states.remove("Blink");
        let err = validate_manifest(&manifest).unwrap_err();
        assert!(err.contains("Blink"), "{err}");
    }

    #[test]
    fn every_referenced_frame_exists_and_matches_the_sheet() {
        let manifest = embedded_manifest().unwrap();
        validate_manifest(&manifest).unwrap();
        let sheet = decode_rgba(SHEET_PNG).unwrap();
        assert_eq!((sheet.width, sheet.height), (384, 1344));
        let root = asset_root();
        for state in AnimationState::ALL {
            let entry = &manifest.states[state.manifest_key()];
            for (column, relative) in entry.frames.iter().enumerate() {
                let path = root.join(relative);
                assert!(path.is_file(), "missing {}", path.display());
                let bytes = std::fs::read(&path).unwrap();
                let image = decode_rgba(&bytes).unwrap();
                assert_eq!(
                    (image.width, image.height),
                    (NATIVE_SIZE, NATIVE_SIZE),
                    "{relative}"
                );
                let cropped = crop_frame(&sheet, NATIVE_SIZE, column as u32, entry.row).unwrap();
                assert_eq!(cropped, image.pixels, "{relative} differs from the sheet");
            }
        }
    }

    #[test]
    fn sheet_contains_transparency_and_the_yellow_robot() {
        let sheet = decode_rgba(SHEET_PNG).unwrap();
        let mut transparent = 0;
        let mut gold = 0;
        for pixel in sheet.pixels.as_chunks::<4>().0 {
            if pixel[3] == 0 {
                transparent += 1;
            }
            if pixel[3] > 200 && pixel[0] > 200 && pixel[1] > 120 && pixel[2] < 90 {
                gold += 1;
            }
        }
        assert!(transparent > 1_000, "transparent pixels: {transparent}");
        assert!(gold > 1_000, "gold pixels: {gold}");
    }

    #[test]
    fn select_frame_follows_eight_fps_and_wraps() {
        let state = AnimationState::Idle;
        assert_eq!(select_frame(0.0, 8.0, 4, false, state), 0);
        assert_eq!(select_frame(0.124, 8.0, 4, false, state), 0);
        assert_eq!(select_frame(0.125, 8.0, 4, false, state), 1);
        assert_eq!(select_frame(0.375, 8.0, 4, false, state), 3);
        assert_eq!(select_frame(0.5, 8.0, 4, false, state), 0);
        assert_eq!(select_frame(1.0, 8.0, 4, false, state), 0);
    }

    #[test]
    fn select_frame_rejects_bad_time_and_honors_reduced_motion() {
        for state in AnimationState::ALL {
            assert_eq!(select_frame(f32::NAN, 8.0, 4, false, state), 0);
            assert_eq!(select_frame(-0.2, 8.0, 4, false, state), 0);
            assert_eq!(select_frame(0.2, 0.0, 4, false, state), 0);
            // 0.2s would be the closed-eye blink frame if motion were enabled.
            assert_eq!(select_frame(0.2, 8.0, 4, true, state), 0);
            assert_eq!(representative_frame(state), 0);
        }
        assert_eq!(select_frame(0.2, 8.0, 4, false, AnimationState::Blink), 1);
    }

    #[test]
    fn cursor_glance_uses_look_frames_only_while_idle() {
        assert_eq!(
            presented_cell(AnimationState::Idle, 0, false, Some(-0.8)),
            (AnimationState::LookAround, 0)
        );
        assert_eq!(
            presented_cell(AnimationState::Idle, 0, false, Some(0.8)),
            (AnimationState::LookAround, 2)
        );
        assert_eq!(
            presented_cell(AnimationState::Idle, 200, false, Some(0.1)),
            (AnimationState::Idle, 1)
        );
        assert_eq!(
            presented_cell(AnimationState::Thinking, 0, false, Some(1.0)),
            (AnimationState::Thinking, 0)
        );
        assert_eq!(
            presented_cell(AnimationState::Idle, 200, true, Some(1.0)),
            (AnimationState::Idle, 0)
        );
    }

    #[test]
    fn integer_scale_fits_without_stretching_pixels() {
        assert_eq!(fit_integer_scale(96.0, 96.0), 96.0);
        assert_eq!(fit_integer_scale(192.0, 96.0), 192.0);
        assert_eq!(fit_integer_scale(104.0, 96.0), 96.0);
        assert_eq!(fit_integer_scale(80.0, 96.0), 48.0);
        assert_eq!(fit_integer_scale(64.0, 96.0), 48.0);
        assert_eq!(fit_integer_scale(48.0, 96.0), 48.0);
        assert_eq!(fit_integer_scale(24.0, 96.0), 24.0);
        assert_eq!(fit_integer_scale(0.0, 96.0), 0.0);
        assert_eq!(display_side(104.0, 96.0), 96.0);
        assert_eq!(display_side(80.0, 96.0), 80.0);
        assert_eq!(display_side(64.0, 96.0), 64.0);
        assert_eq!(display_side(24.0, 96.0), 24.0);
    }

    #[test]
    fn nearest_neighbor_scale_copies_whole_pixels() {
        let src = [1, 2, 3, 255, 9, 8, 7, 255, 4, 5, 6, 128, 0, 0, 0, 0];
        let scaled = scale_nearest(&src, 2, 2, 2).unwrap();
        assert_eq!(scaled.len(), 8 * 8);
        assert_eq!(&scaled[0..4], &[1, 2, 3, 255]);
        assert_eq!(&scaled[4..8], &[1, 2, 3, 255]);
        // Destination row 1 is still the first source pixel. Row 2 starts the next one.
        assert_eq!(&scaled[16..20], &[1, 2, 3, 255]);
        assert_eq!(&scaled[8..12], &[9, 8, 7, 255]);
        assert_eq!(&scaled[32..36], &[4, 5, 6, 128]);
        assert_eq!(&scaled[36..40], &[4, 5, 6, 128]);
        assert!(scale_nearest(&src, 2, 2, 0).is_err());
    }

    #[test]
    fn frame_uv_insets_half_a_texel() {
        let [u0, v0, u1, v1] = frame_uv(1, 2, 96, 384, 1344);
        assert!((u0 - 96.5 / 384.0).abs() < 1e-5);
        assert!((u1 - 191.5 / 384.0).abs() < 1e-5);
        assert!((v0 - (192.5 / 1344.0)).abs() < 1e-5);
        assert!((v1 - (287.5 / 1344.0)).abs() < 1e-5);
    }

    #[test]
    fn writes_visual_previews() {
        let sheet = decode_rgba(SHEET_PNG).unwrap();
        let root = PathBuf::from("/tmp");
        let strip = expression_strip(&sheet);
        write_png(
            &root.join("rusty-16bit-expressions.png"),
            96 * 7,
            96,
            &strip,
        )
        .unwrap();
        let mock = interface_mock(&sheet);
        write_png(&root.join("rusty-16bit-ui-mock.png"), 480, 300, &mock).unwrap();
        let check = decode_rgba(&std::fs::read("/tmp/rusty-16bit-ui-mock.png").unwrap()).unwrap();
        assert_eq!((check.width, check.height), (480, 300));
    }

    fn expression_strip(sheet: &DecodedImage) -> Vec<u8> {
        let states = [
            AnimationState::Idle,
            AnimationState::Blink,
            AnimationState::LookAround,
            AnimationState::Happy,
            AnimationState::Focused,
            AnimationState::Sleep,
            AnimationState::Celebrate,
        ];
        let mut canvas = vec![0; 96 * states.len() * 96 * 4];
        for (index, state) in states.iter().enumerate() {
            let column = if *state == AnimationState::Blink {
                2
            } else {
                0
            };
            blit_frame(
                &mut canvas,
                96 * states.len() as u32,
                sheet,
                column,
                state_row(*state),
                index as u32 * 96,
                0,
                1,
            );
        }
        canvas
    }

    fn interface_mock(sheet: &DecodedImage) -> Vec<u8> {
        let width = 480u32;
        let height = 300u32;
        let mut canvas = vec![0; (width * height * 4) as usize];
        fill(&mut canvas, width, 0, 0, width, height, [10, 16, 32, 255]);
        stroke_rect(&mut canvas, width, 8, 8, 464, 284, [90, 110, 145, 255], 2);
        fill(&mut canvas, width, 16, 16, 448, 40, [22, 32, 52, 255]);
        stroke_rect(&mut canvas, width, 16, 16, 448, 40, [90, 110, 145, 255], 2);
        let icons = [
            crate::ui::icons8bit::PixelIcon::Chat,
            crate::ui::icons8bit::PixelIcon::Tasks,
            crate::ui::icons8bit::PixelIcon::Habits,
            crate::ui::icons8bit::PixelIcon::Focus,
            crate::ui::icons8bit::PixelIcon::MusicNote,
            crate::ui::icons8bit::PixelIcon::Settings,
        ];
        for (index, icon) in icons.iter().enumerate() {
            let x = 28 + index as u32 * 40;
            fill(&mut canvas, width, x, 24, 28, 26, [28, 40, 64, 255]);
            stroke_rect(&mut canvas, width, x, 24, 28, 26, [255, 219, 97, 255], 2);
            blit_icon(
                &mut canvas,
                width,
                icon.bitmap(),
                x + 6,
                29,
                2,
                [255, 247, 223, 255],
            );
        }
        blit_frame(
            &mut canvas,
            width,
            sheet,
            0,
            state_row(AnimationState::Happy),
            24,
            78,
            1,
        );
        blit_frame(
            &mut canvas,
            width,
            sheet,
            2,
            state_row(AnimationState::Blink),
            140,
            78,
            1,
        );
        blit_frame(
            &mut canvas,
            width,
            sheet,
            0,
            state_row(AnimationState::Sleep),
            256,
            78,
            1,
        );
        blit_frame(
            &mut canvas,
            width,
            sheet,
            0,
            state_row(AnimationState::Celebrate),
            372,
            78,
            1,
        );
        fill(&mut canvas, width, 150, 196, 300, 36, [10, 16, 32, 255]);
        stroke_rect(
            &mut canvas,
            width,
            150,
            196,
            300,
            36,
            [242, 160, 21, 255],
            2,
        );
        fill(&mut canvas, width, 166, 210, 180, 4, [255, 247, 223, 255]);
        fill(&mut canvas, width, 166, 220, 120, 4, [176, 188, 210, 255]);
        canvas
    }

    #[allow(clippy::too_many_arguments)]
    fn blit_frame(
        canvas: &mut [u8],
        canvas_w: u32,
        sheet: &DecodedImage,
        column: u32,
        row: u32,
        dx: u32,
        dy: u32,
        scale: u32,
    ) {
        let frame = crop_frame(sheet, NATIVE_SIZE, column, row).unwrap();
        let scaled = scale_nearest(&frame, NATIVE_SIZE, NATIVE_SIZE, scale).unwrap();
        let side = NATIVE_SIZE * scale;
        for y in 0..side {
            for x in 0..side {
                let pixel =
                    &scaled[((y * side + x) * 4) as usize..((y * side + x) * 4 + 4) as usize];
                if pixel[3] == 0 {
                    continue;
                }
                put(
                    canvas,
                    canvas_w,
                    dx + x,
                    dy + y,
                    [pixel[0], pixel[1], pixel[2], pixel[3]],
                );
            }
        }
    }

    fn blit_icon(
        canvas: &mut [u8],
        canvas_w: u32,
        bitmap: [u8; 8],
        dx: u32,
        dy: u32,
        scale: u32,
        color: [u8; 4],
    ) {
        for row in 0..8u32 {
            for col in 0..8u32 {
                if bitmap[row as usize] & (1 << (7 - col)) == 0 {
                    continue;
                }
                for y in 0..scale {
                    for x in 0..scale {
                        put(
                            canvas,
                            canvas_w,
                            dx + col * scale + x,
                            dy + row * scale + y,
                            color,
                        );
                    }
                }
            }
        }
    }

    fn fill(canvas: &mut [u8], canvas_w: u32, x: u32, y: u32, w: u32, h: u32, color: [u8; 4]) {
        for py in y..y + h {
            for px in x..x + w {
                put(canvas, canvas_w, px, py, color);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn stroke_rect(
        canvas: &mut [u8],
        canvas_w: u32,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        color: [u8; 4],
        thickness: u32,
    ) {
        fill(canvas, canvas_w, x, y, w, thickness, color);
        fill(canvas, canvas_w, x, y + h - thickness, w, thickness, color);
        fill(canvas, canvas_w, x, y, thickness, h, color);
        fill(canvas, canvas_w, x + w - thickness, y, thickness, h, color);
    }

    fn put(canvas: &mut [u8], canvas_w: u32, x: u32, y: u32, color: [u8; 4]) {
        let index = ((y * canvas_w + x) * 4) as usize;
        if index + 4 <= canvas.len() {
            canvas[index..index + 4].copy_from_slice(&color);
        }
    }

    fn write_png(
        path: &std::path::Path,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<(), String> {
        let file = std::fs::File::create(path).map_err(|err| err.to_string())?;
        let mut encoder = png::Encoder::new(file, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|err| err.to_string())?;
        writer
            .write_image_data(pixels)
            .map_err(|err| err.to_string())?;
        Ok(())
    }
}
