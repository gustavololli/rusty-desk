use eframe::egui;
use std::sync::{Arc, Mutex};

/// Compile-time host detection; native backends are selected by the target OS.
pub fn host_os_label() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "Windows"
    }
    #[cfg(target_os = "linux")]
    {
        "Linux"
    }
    #[cfg(target_os = "macos")]
    {
        "macOS"
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        "Otro sistema"
    }
}

/// Display server session kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    X11,
    Wayland,
    Unknown,
}

/// Logical monitor rectangle in egui points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonitorRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Default for MonitorRect {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        }
    }
}

impl MonitorRect {
    pub fn as_tuple(self) -> (f32, f32, f32, f32) {
        (self.x, self.y, self.width, self.height)
    }
}

/// Classifies a desktop session from the variables a display server exports.
/// `XDG_SESSION_TYPE` wins. An empty value falls through to the display name.
pub fn session_kind_from_env(
    xdg_session_type: &str,
    wayland_display: Option<&str>,
    display: Option<&str>,
) -> SessionKind {
    let xdg = xdg_session_type.to_lowercase();
    if xdg == "wayland" {
        return SessionKind::Wayland;
    }
    if xdg == "x11" {
        return SessionKind::X11;
    }
    if wayland_display.is_some_and(|v| !v.is_empty()) {
        return SessionKind::Wayland;
    }
    if display.is_some_and(|v| !v.is_empty()) {
        return SessionKind::X11;
    }
    SessionKind::Unknown
}

/// Detects whether the current Linux desktop session is X11 or Wayland.
pub fn detect_session_kind() -> SessionKind {
    let wayland = std::env::var("WAYLAND_DISPLAY").ok();
    let display = std::env::var("DISPLAY").ok();
    session_kind_from_env(
        &std::env::var("XDG_SESSION_TYPE").unwrap_or_default(),
        wayland.as_deref(),
        display.as_deref(),
    )
}

/// Queries the X11 root screen dimensions in physical pixels.
pub fn query_x11_virtual_bounds() -> Option<(f32, f32, f32, f32)> {
    #[cfg(not(target_os = "linux"))]
    {
        return None;
    }
    #[cfg(target_os = "linux")]
    {
        use x11rb::connection::Connection;
        let (conn, screen_num) = x11rb::connect(None).ok()?;
        let screen = conn.setup().roots.get(screen_num)?;
        Some((
            0.0,
            0.0,
            f32::from(screen.width_in_pixels),
            f32::from(screen.height_in_pixels),
        ))
    }
}

/// Computes the current monitor's logical bounds in egui points, accounting for
/// HiDPI `pixels_per_point` and multi-monitor virtual desktops.
pub fn resolve_monitor_rect(
    egui_monitor_size: Option<[f32; 2]>,
    outer_center: Option<[f32; 2]>,
    x11_virtual_physical: Option<(f32, f32, f32, f32)>,
    pixels_per_point: f32,
) -> MonitorRect {
    let scale = if pixels_per_point.is_finite() && pixels_per_point > 0.1 {
        pixels_per_point
    } else {
        1.0
    };

    let virtual_logical = x11_virtual_physical.map(|(vx, vy, vw, vh)| {
        (
            vx / scale,
            vy / scale,
            (vw / scale).max(320.0),
            (vh / scale).max(240.0),
        )
    });

    match (egui_monitor_size, virtual_logical) {
        (Some([mw, mh]), Some((vx, vy, vw, vh))) if mw > 100.0 && mh > 100.0 => {
            // If virtual desktop spans multiple monitors, infer the active monitor tile from window center.
            if let Some([cx, cy]) = outer_center {
                let cols = (vw / mw).round().max(1.0);
                let rows = (vh / mh).round().max(1.0);
                let col = ((cx - vx) / mw).floor().clamp(0.0, cols - 1.0);
                let row = ((cy - vy) / mh).floor().clamp(0.0, rows - 1.0);
                let x = (vx + col * mw).clamp(vx, (vx + vw - mw).max(vx));
                let y = (vy + row * mh).clamp(vy, (vy + vh - mh).max(vy));
                MonitorRect {
                    x,
                    y,
                    width: mw,
                    height: mh,
                }
            } else {
                MonitorRect {
                    x: vx,
                    y: vy,
                    width: mw,
                    height: mh,
                }
            }
        }
        (Some([mw, mh]), None) if mw > 100.0 && mh > 100.0 => MonitorRect {
            x: 0.0,
            y: 0.0,
            width: mw,
            height: mh,
        },
        (None, Some((vx, vy, vw, vh))) => MonitorRect {
            x: vx,
            y: vy,
            width: vw,
            height: vh,
        },
        _ => MonitorRect::default(),
    }
}

/// Detects the current monitor in logical egui points from `egui::Context` and X11 virtual bounds.
pub fn detect_current_monitor(
    ctx: &egui::Context,
    x11_virtual_physical: Option<(f32, f32, f32, f32)>,
) -> MonitorRect {
    let ppp = ctx.pixels_per_point();
    let (mon_size, outer_center) = ctx.input(|i| {
        let m = i.viewport().monitor_size.map(|v| [v.x, v.y]);
        let c = i
            .viewport()
            .outer_rect
            .map(|r| [r.center().x, r.center().y]);
        (m, c)
    });
    resolve_monitor_rect(mon_size, outer_center, x11_virtual_physical, ppp)
}

/// Spawns a background X11 pointer tracker for eye-following in Pet/Island modes.
pub fn spawn_global_cursor_tracker() -> Arc<Mutex<Option<(i16, i16)>>> {
    let pos = Arc::new(Mutex::new(None));
    let pos_clone = Arc::clone(&pos);

    #[cfg(target_os = "windows")]
    {
        std::thread::spawn(move || {
            use windows::Win32::Foundation::POINT;
            use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
            loop {
                let mut point = POINT::default();
                if unsafe { GetCursorPos(&mut point) }.is_ok() {
                    if let Ok(mut guard) = pos_clone.lock() {
                        *guard = Some((
                            point.x.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
                            point.y.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
                        ));
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(33));
            }
        });
    }

    #[cfg(target_os = "linux")]
    std::thread::spawn(move || {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::ConnectionExt;

        let Ok((conn, screen_num)) = x11rb::connect(None) else {
            return;
        };
        let root = conn.setup().roots[screen_num].root;
        loop {
            if let Ok(cookie) = conn.query_pointer(root) {
                if let Ok(reply) = cookie.reply() {
                    if let Ok(mut guard) = pos_clone.lock() {
                        *guard = Some((reply.root_x, reply.root_y));
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(33));
        }
    });

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    let _ = pos_clone;
    pos
}

/// On KDE Plasma Wayland, `winit`'s `WindowLevel::AlwaysOnTop` is a no-op because
/// `xdg-shell` does not expose always-on-top to clients. Sync a KWin window rule
/// asynchronously so KWin keeps Rusty-Desk above other windows when requested.
pub fn apply_platform_always_on_top(enabled: bool) {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = enabled;
        return;
    }
    #[cfg(target_os = "linux")]
    {
        let is_wayland = std::env::var("XDG_SESSION_TYPE")
            .map(|v| v.eq_ignore_ascii_case("wayland"))
            .unwrap_or(false)
            || std::env::var_os("WAYLAND_DISPLAY").is_some();
        let is_kde = std::env::var("XDG_CURRENT_DESKTOP")
            .map(|v| v.to_ascii_uppercase().contains("KDE"))
            .unwrap_or(false);

        if !is_wayland || !is_kde {
            return;
        }

        std::thread::spawn(move || {
            use std::process::Command;

            const RULE_ID: &str = "rusty-desk-always-on-top";
            let rules_path = directories::BaseDirs::new()
                .map(|b| b.config_dir().join("kwinrulesrc"))
                .unwrap_or_else(|| std::path::PathBuf::from("kwinrulesrc"));

            let existing_rules = std::fs::read_to_string(&rules_path)
                .ok()
                .and_then(|raw| {
                    raw.lines()
                        .find_map(|line| line.strip_prefix("rules=").map(str::to_string))
                })
                .unwrap_or_default();
            let merged_rules = merge_kwin_rules_list(&existing_rules, RULE_ID);
            let above_val = if enabled { "true" } else { "false" };

            let writes = [
                ("General", "rules", merged_rules.as_str()),
                (RULE_ID, "Description", "Rusty-Desk Always On Top"),
                (RULE_ID, "title", "Rusty-Desk"),
                (RULE_ID, "titlematch", "1"),
                (RULE_ID, "above", above_val),
                (RULE_ID, "aboverule", "2"),
            ];

            for (group, key, val) in writes {
                let status = Command::new("kwriteconfig6")
                    .args(["--file", "kwinrulesrc", "--group", group, "--key", key, val])
                    .status();
                if status.is_err() {
                    return;
                }
            }

            let _ = Command::new("qdbus6")
                .args(["org.kde.KWin", "/KWin", "reconfigure"])
                .status();
        });
    }
}

pub fn merge_kwin_rules_list(existing: &str, rule_id: &str) -> String {
    let mut items: Vec<String> = existing
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect();
    if !items.iter().any(|item| item == rule_id) {
        items.push(rule_id.to_string());
    }
    items.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_label_matches_target() {
        #[cfg(target_os = "windows")]
        assert_eq!(host_os_label(), "Windows");
        #[cfg(target_os = "linux")]
        assert_eq!(host_os_label(), "Linux");
    }

    #[test]
    fn resolve_monitor_rect_respects_hidpi_and_multi_monitor() {
        // Two 1920x1080 monitors at 2x HiDPI => virtual 7680x2160 physical => 3840x1080 logical.
        let rect = resolve_monitor_rect(
            Some([1920.0, 1080.0]),
            Some([2500.0, 400.0]),
            Some((0.0, 0.0, 7680.0, 2160.0)),
            2.0,
        );
        assert_eq!(
            rect,
            MonitorRect {
                x: 1920.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
            }
        );
    }

    #[test]
    fn merge_kwin_rules_preserves_existing_and_avoids_duplicates() {
        assert_eq!(
            merge_kwin_rules_list("", "rusty-desk-always-on-top"),
            "rusty-desk-always-on-top"
        );
        assert_eq!(
            merge_kwin_rules_list("rule1, rule2", "rusty-desk-always-on-top"),
            "rule1,rule2,rusty-desk-always-on-top"
        );
        assert_eq!(
            merge_kwin_rules_list("rule1,rusty-desk-always-on-top", "rusty-desk-always-on-top"),
            "rule1,rusty-desk-always-on-top"
        );
    }
}
