use crate::config::{clamp_window_position, compute_island_position, IslandPosition};
use crate::platform::MonitorRect;

/// Explicit visibility state machine for Smart Island auto-hide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IslandVisibility {
    #[default]
    Visible,
    WaitingToHide,
    Hiding,
    Hidden,
    Showing,
}

/// Per-frame inputs evaluated by `IslandStateMachine`.
#[derive(Debug, Clone, Copy)]
pub struct IslandInput {
    pub now: f64,
    pub dt: f32,
    pub auto_hide: bool,
    pub hide_delay_secs: u32,
    pub show_on_hover: bool,
    pub keep_visible_during_playback: bool,
    pub keep_visible_during_ai: bool,
    pub pointer_in_island: bool,
    pub pointer_in_tab: bool,
    pub is_dragging: bool,
    pub is_interacting: bool,
    pub has_open_menu: bool,
    pub is_recording: bool,
    pub music_active: bool,
    pub ai_busy: bool,
}

/// Output geometry and state for the current frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IslandFrameOutput {
    pub visibility: IslandVisibility,
    pub is_animating: bool,
    pub size: [f32; 2],
    pub pos: Option<[f32; 2]>,
}

const HIDE_ANIM_SECS: f32 = 0.25;
const SHOW_ANIM_SECS: f32 = 0.22;
pub const TAB_THICKNESS: f32 = 24.0;
pub const TAB_LENGTH: f32 = 88.0;
pub const SNAP_THRESHOLD_PX: f32 = 24.0;
const HOVER_HYSTERESIS_SECS: f64 = 0.25;
const REHIDE_AFTER_HOVER_SECS: f64 = 1.0;

pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Snaps a dragged island window to the nearest monitor edge if within `SNAP_THRESHOLD_PX` (24 px).
/// Returns `(IslandPosition::Custom, clamped_pos)` when outside the threshold, or the snapped edge
/// and margin-aligned position when within the threshold.
pub fn snap_island_to_edge(
    pos: [f32; 2],
    size: [f32; 2],
    monitor: MonitorRect,
    margin: f32,
) -> (IslandPosition, [f32; 2]) {
    let margin = if margin.is_finite() {
        margin.clamp(0.0, 128.0)
    } else {
        12.0
    };
    let clamped = clamp_window_position(pos, size, Some(monitor.as_tuple()));

    let dist_top = (clamped[1] - monitor.y).abs();
    let dist_bottom = ((monitor.y + monitor.height) - (clamped[1] + size[1])).abs();
    let dist_left = (clamped[0] - monitor.x).abs();
    let dist_right = ((monitor.x + monitor.width) - (clamped[0] + size[0])).abs();

    let mut best = (dist_top, IslandPosition::Top);
    for candidate in [
        (dist_left, IslandPosition::Left),
        (dist_right, IslandPosition::Right),
        (dist_bottom, IslandPosition::Bottom),
    ] {
        if candidate.0 < best.0 {
            best = candidate;
        }
    }

    if best.0 > SNAP_THRESHOLD_PX {
        return (IslandPosition::Custom, clamped);
    }

    let snapped = match best.1 {
        IslandPosition::Top => [clamped[0], monitor.y + margin],
        IslandPosition::Bottom => [clamped[0], monitor.y + monitor.height - size[1] - margin],
        IslandPosition::Left => [monitor.x + margin, clamped[1]],
        IslandPosition::Right => [monitor.x + monitor.width - size[0] - margin, clamped[1]],
        IslandPosition::Custom => clamped,
    };
    (
        best.1,
        clamp_window_position(snapped, size, Some(monitor.as_tuple())),
    )
}

/// Returns `true` when the island is docked to a compatible auto-hide edge (`Top`, `Left`, `Right`).
/// `Bottom` and floating `Custom` positions do not auto-hide.
pub fn supports_auto_hide(
    anchor: IslandPosition,
    custom_pos: Option<[f32; 2]>,
    island_size: [f32; 2],
    monitor: MonitorRect,
    margin: f32,
) -> bool {
    match anchor {
        IslandPosition::Top | IslandPosition::Left | IslandPosition::Right => true,
        IslandPosition::Bottom => false,
        IslandPosition::Custom => {
            let Some(pos) = custom_pos else {
                return false;
            };
            let tol = margin.clamp(0.0, 128.0) + SNAP_THRESHOLD_PX;
            let dist_top = (pos[1] - monitor.y).abs();
            let dist_left = (pos[0] - monitor.x).abs();
            let dist_right = ((monitor.x + monitor.width) - (pos[0] + island_size[0])).abs();

            let edge = infer_nearest_edge(pos, island_size, monitor);
            match edge {
                IslandPosition::Top => dist_top <= tol,
                IslandPosition::Left => dist_left <= tol,
                IslandPosition::Right => dist_right <= tol,
                IslandPosition::Bottom | IslandPosition::Custom => false,
            }
        }
    }
}

/// Infers the nearest monitor edge (`Top`, `Bottom`, `Left`, `Right`) for a window rect.
pub fn infer_nearest_edge(pos: [f32; 2], size: [f32; 2], monitor: MonitorRect) -> IslandPosition {
    let dist_top = (pos[1] - monitor.y).abs();
    let dist_bottom = ((monitor.y + monitor.height) - (pos[1] + size[1])).abs();
    let dist_left = (pos[0] - monitor.x).abs();
    let dist_right = ((monitor.x + monitor.width) - (pos[0] + size[0])).abs();

    let mut best = (dist_top, IslandPosition::Top);
    for candidate in [
        (dist_bottom, IslandPosition::Bottom),
        (dist_left, IslandPosition::Left),
        (dist_right, IslandPosition::Right),
    ] {
        if candidate.0 < best.0 {
            best = candidate;
        }
    }
    best.1
}

pub fn effective_tab_edge(
    anchor: IslandPosition,
    monitor: MonitorRect,
    custom_pos: Option<[f32; 2]>,
    island_size: [f32; 2],
) -> IslandPosition {
    match anchor {
        IslandPosition::Custom => custom_pos
            .map(|pos| infer_nearest_edge(pos, island_size, monitor))
            .unwrap_or(IslandPosition::Top),
        other => other,
    }
}

pub fn tab_size_for(anchor: IslandPosition) -> [f32; 2] {
    match anchor {
        IslandPosition::Left | IslandPosition::Right => [TAB_THICKNESS, TAB_LENGTH],
        IslandPosition::Top | IslandPosition::Bottom | IslandPosition::Custom => {
            [TAB_LENGTH, TAB_THICKNESS]
        }
    }
}

pub fn tab_position_for(
    anchor: IslandPosition,
    monitor: MonitorRect,
    custom_pos: Option<[f32; 2]>,
    island_size: [f32; 2],
) -> [f32; 2] {
    let edge = effective_tab_edge(anchor, monitor, custom_pos, island_size);
    let tab = tab_size_for(edge);
    let raw = match anchor {
        IslandPosition::Top => [monitor.x + (monitor.width - tab[0]) * 0.5, monitor.y],
        IslandPosition::Bottom => [
            monitor.x + (monitor.width - tab[0]) * 0.5,
            monitor.y + monitor.height - tab[1],
        ],
        IslandPosition::Left => [monitor.x, monitor.y + (monitor.height - tab[1]) * 0.5],
        IslandPosition::Right => [
            monitor.x + monitor.width - tab[0],
            monitor.y + (monitor.height - tab[1]) * 0.5,
        ],
        IslandPosition::Custom => {
            if let Some([cx, cy]) = custom_pos {
                match edge {
                    IslandPosition::Top | IslandPosition::Custom => {
                        [cx + (island_size[0] - tab[0]) * 0.5, cy]
                    }
                    IslandPosition::Bottom => [
                        cx + (island_size[0] - tab[0]) * 0.5,
                        cy + island_size[1] - tab[1],
                    ],
                    IslandPosition::Left => [cx, cy + (island_size[1] - tab[1]) * 0.5],
                    IslandPosition::Right => [
                        cx + island_size[0] - tab[0],
                        cy + (island_size[1] - tab[1]) * 0.5,
                    ],
                }
            } else {
                [monitor.x + (monitor.width - tab[0]) * 0.5, monitor.y]
            }
        }
    };
    clamp_window_position(raw, tab, Some(monitor.as_tuple()))
}

#[derive(Debug, Clone)]
pub struct IslandStateMachine {
    pub visibility: IslandVisibility,
    hide_timer_start: Option<f64>,
    hover_leave_time: Option<f64>,
    restored_by_hover: bool,
    /// Linear animation progress in `0.0..=1.0` for Hiding/Showing.
    anim_t: f32,
    last_applied: Option<([f32; 2], Option<[f32; 2]>)>,
}

impl Default for IslandStateMachine {
    fn default() -> Self {
        Self {
            visibility: IslandVisibility::Visible,
            hide_timer_start: None,
            hover_leave_time: None,
            restored_by_hover: false,
            anim_t: 1.0,
            last_applied: None,
        }
    }
}

impl IslandStateMachine {
    pub fn reset_visible(&mut self) {
        self.visibility = IslandVisibility::Visible;
        self.hide_timer_start = None;
        self.hover_leave_time = None;
        self.restored_by_hover = false;
        self.anim_t = 1.0;
        self.last_applied = None;
    }

    pub fn invalidate_geometry(&mut self) {
        self.last_applied = None;
    }

    pub fn should_apply_geometry(&mut self, size: [f32; 2], pos: Option<[f32; 2]>) -> bool {
        let rounded_size = [size[0].round(), size[1].round()];
        let rounded_pos = pos.map(|[x, y]| [x.round(), y.round()]);
        let key = (rounded_size, rounded_pos);
        if self.last_applied == Some(key) {
            false
        } else {
            self.last_applied = Some(key);
            true
        }
    }

    fn stay_visible_reason(input: &IslandInput, can_auto_hide: bool) -> bool {
        !input.auto_hide
            || !can_auto_hide
            || input.is_dragging
            || input.is_interacting
            || input.has_open_menu
            || input.is_recording
            || input.pointer_in_island
            || (input.music_active && input.keep_visible_during_playback)
            || (input.ai_busy && input.keep_visible_during_ai)
    }

    pub fn step(
        &mut self,
        input: &IslandInput,
        anchor: IslandPosition,
        monitor: MonitorRect,
        margin: f32,
        custom_pos: Option<[f32; 2]>,
        expanded_size: [f32; 2],
    ) -> IslandFrameOutput {
        let can_auto_hide = supports_auto_hide(anchor, custom_pos, expanded_size, monitor, margin);
        let keep_visible = Self::stay_visible_reason(input, can_auto_hide);
        let base_delay = f64::from(input.hide_delay_secs.max(1));
        let effective_delay = if self.restored_by_hover {
            base_delay.min(REHIDE_AFTER_HOVER_SECS)
        } else {
            base_delay
        };
        let dt = input.dt.clamp(0.0, 0.1);

        match self.visibility {
            IslandVisibility::Visible => {
                if keep_visible {
                    self.hide_timer_start = None;
                    self.hover_leave_time = None;
                    if input.pointer_in_island {
                        // Pointer is clearly inside the restored island.
                    }
                } else {
                    let leave_t = *self.hover_leave_time.get_or_insert(input.now);
                    if input.now - leave_t >= HOVER_HYSTERESIS_SECS {
                        self.visibility = IslandVisibility::WaitingToHide;
                        self.hide_timer_start = Some(input.now);
                        self.hover_leave_time = None;
                    }
                }
            }
            IslandVisibility::WaitingToHide => {
                if keep_visible {
                    self.visibility = IslandVisibility::Visible;
                    self.hide_timer_start = None;
                    self.hover_leave_time = None;
                } else {
                    let started = *self.hide_timer_start.get_or_insert(input.now);
                    if input.now - started >= effective_delay {
                        self.visibility = IslandVisibility::Hiding;
                        self.anim_t = 0.0;
                        self.hide_timer_start = None;
                        self.restored_by_hover = false;
                    }
                }
            }
            IslandVisibility::Hiding => {
                if keep_visible {
                    let remaining = (1.0 - self.anim_t).clamp(0.0, 1.0);
                    self.visibility = IslandVisibility::Showing;
                    self.anim_t = remaining;
                } else {
                    self.anim_t = (self.anim_t + dt / HIDE_ANIM_SECS).min(1.0);
                    if self.anim_t >= 1.0 {
                        self.visibility = IslandVisibility::Hidden;
                    }
                }
            }
            IslandVisibility::Hidden => {
                let wake_for_activity = !input.auto_hide
                    || !can_auto_hide
                    || input.is_recording
                    || (input.music_active && input.keep_visible_during_playback)
                    || (input.ai_busy && input.keep_visible_during_ai);
                let wake_for_hover = input.show_on_hover && input.pointer_in_tab;
                if wake_for_activity || wake_for_hover {
                    self.restored_by_hover = wake_for_hover && !wake_for_activity;
                    self.visibility = IslandVisibility::Showing;
                    self.anim_t = 0.0;
                }
            }
            IslandVisibility::Showing => {
                self.anim_t = (self.anim_t + dt / SHOW_ANIM_SECS).min(1.0);
                if self.anim_t >= 1.0 {
                    self.visibility = IslandVisibility::Visible;
                    self.hide_timer_start = None;
                    self.hover_leave_time = None;
                }
            }
        }

        let visible_pos = compute_island_position(
            anchor,
            expanded_size,
            Some(monitor.as_tuple()),
            margin,
            custom_pos,
        );
        let effective_edge = effective_tab_edge(anchor, monitor, custom_pos, expanded_size);
        let tab_size = tab_size_for(effective_edge);
        let tab_pos = tab_position_for(anchor, monitor, custom_pos, expanded_size);

        let (size, pos, is_animating) = match self.visibility {
            IslandVisibility::Visible | IslandVisibility::WaitingToHide => {
                (expanded_size, visible_pos, false)
            }
            IslandVisibility::Hidden => (tab_size, Some(tab_pos), false),
            IslandVisibility::Hiding => {
                let k = 1.0 - ease_out_cubic(self.anim_t);
                let size = lerp2(tab_size, expanded_size, k);
                let pos = visible_pos.map(|vp| lerp2(tab_pos, vp, k));
                (size, pos, true)
            }
            IslandVisibility::Showing => {
                let k = ease_out_cubic(self.anim_t);
                let size = lerp2(tab_size, expanded_size, k);
                let pos = visible_pos.map(|vp| lerp2(tab_pos, vp, k));
                (size, pos, true)
            }
        };

        IslandFrameOutput {
            visibility: self.visibility,
            is_animating,
            size,
            pos,
        }
    }
}

fn lerp2(a: [f32; 2], b: [f32; 2], t: f32) -> [f32; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_input(now: f64) -> IslandInput {
        IslandInput {
            now,
            dt: 0.05,
            auto_hide: true,
            hide_delay_secs: 2,
            show_on_hover: true,
            keep_visible_during_playback: true,
            keep_visible_during_ai: true,
            pointer_in_island: false,
            pointer_in_tab: false,
            is_dragging: false,
            is_interacting: false,
            has_open_menu: false,
            is_recording: false,
            music_active: false,
            ai_busy: false,
        }
    }

    #[test]
    fn auto_hide_transitions_through_timer_animation_and_hover_restore() {
        let mut sm = IslandStateMachine::default();
        let mon = MonitorRect::default();
        let size = [360.0, 66.0];

        // Mouse leaves -> hysteresis (0.25s) -> WaitingToHide
        let out = sm.step(&base_input(1.0), IslandPosition::Top, mon, 12.0, None, size);
        assert_eq!(out.visibility, IslandVisibility::Visible);
        let out = sm.step(&base_input(1.3), IslandPosition::Top, mon, 12.0, None, size);
        assert_eq!(out.visibility, IslandVisibility::WaitingToHide);

        // Before 2s -> still WaitingToHide
        let out = sm.step(&base_input(2.8), IslandPosition::Top, mon, 12.0, None, size);
        assert_eq!(out.visibility, IslandVisibility::WaitingToHide);

        // After 2s -> Hiding
        let out = sm.step(&base_input(3.4), IslandPosition::Top, mon, 12.0, None, size);
        assert_eq!(out.visibility, IslandVisibility::Hiding);
        assert!(out.is_animating);

        // Advance past 250ms -> Hidden tab (24px thick)
        let mut t = 3.45;
        let mut hidden_out = out;
        for _ in 0..8 {
            hidden_out = sm.step(&base_input(t), IslandPosition::Top, mon, 12.0, None, size);
            t += 0.05;
        }
        assert_eq!(hidden_out.visibility, IslandVisibility::Hidden);
        assert_eq!(hidden_out.size, [TAB_LENGTH, TAB_THICKNESS]);
        assert!(!hidden_out.is_animating);

        // Mouse enters tab -> Showing -> Visible (mouse is now inside island)
        let mut hover_in = base_input(t);
        hover_in.pointer_in_tab = true;
        let out = sm.step(&hover_in, IslandPosition::Top, mon, 12.0, None, size);
        assert_eq!(out.visibility, IslandVisibility::Showing);

        hover_in.pointer_in_tab = false;
        hover_in.pointer_in_island = true;
        for _ in 0..8 {
            t += 0.05;
            hover_in.now = t;
            let _ = sm.step(&hover_in, IslandPosition::Top, mon, 12.0, None, size);
        }
        assert_eq!(sm.visibility, IslandVisibility::Visible);
    }

    #[test]
    fn blockers_prevent_hiding_and_cancel_waiting_timer() {
        let mut sm = IslandStateMachine::default();
        let mon = MonitorRect::default();
        let size = [360.0, 66.0];

        // Start waiting (after 0.25s hysteresis)
        sm.step(
            &base_input(1.0),
            IslandPosition::Right,
            mon,
            12.0,
            None,
            size,
        );
        sm.step(
            &base_input(1.3),
            IslandPosition::Right,
            mon,
            12.0,
            None,
            size,
        );
        assert_eq!(sm.visibility, IslandVisibility::WaitingToHide);

        // Dragging cancels waiting
        let mut drag = base_input(2.0);
        drag.is_dragging = true;
        sm.step(&drag, IslandPosition::Right, mon, 12.0, None, size);
        assert_eq!(sm.visibility, IslandVisibility::Visible);

        // Music playback keeps visible when configured
        let mut music = base_input(10.0);
        music.music_active = true;
        sm.step(&music, IslandPosition::Right, mon, 12.0, None, size);
        assert_eq!(sm.visibility, IslandVisibility::Visible);

        // Open menu / interacting button keeps visible
        let mut menu = base_input(11.0);
        menu.has_open_menu = true;
        sm.step(&menu, IslandPosition::Right, mon, 12.0, None, size);
        assert_eq!(sm.visibility, IslandVisibility::Visible);
    }

    #[test]
    fn tab_orientation_contracts_vertically_at_top_bottom_and_horizontally_at_sides() {
        let top = tab_size_for(IslandPosition::Top);
        let bottom = tab_size_for(IslandPosition::Bottom);
        let left = tab_size_for(IslandPosition::Left);
        let right = tab_size_for(IslandPosition::Right);

        // Top & Bottom: wide horizontal pill (height = TAB_THICKNESS, width = TAB_LENGTH)
        assert_eq!(top, [TAB_LENGTH, TAB_THICKNESS]);
        assert_eq!(bottom, [TAB_LENGTH, TAB_THICKNESS]);
        assert!(top[0] > top[1]);

        // Left & Right: tall vertical pill (width = TAB_THICKNESS, height = TAB_LENGTH)
        assert_eq!(left, [TAB_THICKNESS, TAB_LENGTH]);
        assert_eq!(right, [TAB_THICKNESS, TAB_LENGTH]);
        assert!(left[1] > left[0]);
    }

    #[test]
    fn infer_nearest_edge_detects_all_four_edges_including_multi_monitor() {
        let mon = MonitorRect {
            x: 1920.0,
            y: 100.0,
            width: 1920.0,
            height: 1080.0,
        };
        let size = [360.0, 66.0];

        // Moved to top center
        assert_eq!(
            infer_nearest_edge([2700.0, 112.0], size, mon),
            IslandPosition::Top
        );
        // Moved to bottom center
        assert_eq!(
            infer_nearest_edge([2700.0, 1100.0], size, mon),
            IslandPosition::Bottom
        );
        // Moved to left middle
        assert_eq!(
            infer_nearest_edge([1930.0, 600.0], size, mon),
            IslandPosition::Left
        );
        // Moved to right middle
        assert_eq!(
            infer_nearest_edge([3470.0, 600.0], size, mon),
            IslandPosition::Right
        );
    }

    #[test]
    fn custom_island_contracts_according_to_nearest_edge() {
        let mon = MonitorRect::default();
        let size = [360.0, 66.0];

        let collapse = |custom_pos: [f32; 2]| {
            let mut sm = IslandStateMachine::default();
            sm.step(
                &base_input(1.0),
                IslandPosition::Custom,
                mon,
                12.0,
                Some(custom_pos),
                size,
            );
            sm.step(
                &base_input(1.3),
                IslandPosition::Custom,
                mon,
                12.0,
                Some(custom_pos),
                size,
            );
            let mut t = 3.4;
            let mut out = sm.step(
                &base_input(t),
                IslandPosition::Custom,
                mon,
                12.0,
                Some(custom_pos),
                size,
            );
            for _ in 0..8 {
                t += 0.05;
                out = sm.step(
                    &base_input(t),
                    IslandPosition::Custom,
                    mon,
                    12.0,
                    Some(custom_pos),
                    size,
                );
            }
            assert_eq!(out.visibility, IslandVisibility::Hidden);
            out
        };

        // Dragged near top -> contracts vertically to horizontal pill [88, 24]
        let top_out = collapse([780.0, 10.0]);
        assert_eq!(top_out.size, [TAB_LENGTH, TAB_THICKNESS]);
        assert_eq!(
            top_out.pos,
            Some([780.0 + (360.0 - TAB_LENGTH) * 0.5, 10.0])
        );

        // Dragged near left -> contracts horizontally to vertical pill [24, 88]
        let left_out = collapse([8.0, 500.0]);
        assert_eq!(left_out.size, [TAB_THICKNESS, TAB_LENGTH]);
        assert_eq!(left_out.pos, Some([8.0, 500.0 + (66.0 - TAB_LENGTH) * 0.5]));

        // Dragged near right -> contracts horizontally to vertical pill [24, 88] at right edge
        let right_out = collapse([1550.0, 500.0]);
        assert_eq!(right_out.size, [TAB_THICKNESS, TAB_LENGTH]);
        assert_eq!(
            right_out.pos,
            Some([
                1550.0 + 360.0 - TAB_THICKNESS,
                500.0 + (66.0 - TAB_LENGTH) * 0.5
            ])
        );
    }

    #[test]
    fn snap_island_to_edge_respects_24px_threshold_and_bounds() {
        let mon = MonitorRect::default();
        let size = [360.0, 66.0];
        let margin = 12.0;

        // Within 24px of Left edge -> snaps to Left
        let (edge, pos) = snap_island_to_edge([18.0, 300.0], size, mon, margin);
        assert_eq!(edge, IslandPosition::Left);
        assert_eq!(pos, [12.0, 300.0]);

        // Within 24px of Right edge -> snaps to Right
        let (edge, pos) = snap_island_to_edge([1920.0 - 360.0 - 20.0, 300.0], size, mon, margin);
        assert_eq!(edge, IslandPosition::Right);
        assert_eq!(pos, [1920.0 - 360.0 - 12.0, 300.0]);

        // Within 24px of Top edge -> snaps to Top
        let (edge, pos) = snap_island_to_edge([700.0, 22.0], size, mon, margin);
        assert_eq!(edge, IslandPosition::Top);
        assert_eq!(pos, [700.0, 12.0]);

        // Outside 24px of all edges -> stays Custom without snapping
        let (edge, pos) = snap_island_to_edge([400.0, 300.0], size, mon, margin);
        assert_eq!(edge, IslandPosition::Custom);
        assert_eq!(pos, [400.0, 300.0]);

        // Off-screen / disconnected monitor -> clamped safely inside monitor
        let (edge, pos) = snap_island_to_edge([2600.0, 1400.0], size, mon, margin);
        assert_eq!(edge, IslandPosition::Right);
        assert_eq!(pos[0], 1920.0 - 360.0 - 12.0);
        assert!(pos[1] <= 1080.0 - 66.0);
    }

    #[test]
    fn bottom_and_floating_custom_do_not_auto_hide() {
        let mon = MonitorRect::default();
        let size = [360.0, 66.0];

        assert!(!supports_auto_hide(
            IslandPosition::Bottom,
            None,
            size,
            mon,
            12.0
        ));
        assert!(!supports_auto_hide(
            IslandPosition::Custom,
            Some([500.0, 400.0]),
            size,
            mon,
            12.0
        ));
        assert!(supports_auto_hide(
            IslandPosition::Top,
            None,
            size,
            mon,
            12.0
        ));

        let mut sm = IslandStateMachine::default();
        for t in [1.0, 1.5, 5.0, 10.0] {
            let out = sm.step(
                &base_input(t),
                IslandPosition::Bottom,
                mon,
                12.0,
                None,
                size,
            );
            assert_eq!(out.visibility, IslandVisibility::Visible);
        }
    }

    #[test]
    fn hover_hysteresis_prevents_oscillation_and_re_hides_after_1s() {
        let mut sm = IslandStateMachine::default();
        let mon = MonitorRect::default();
        let size = [360.0, 66.0];

        // Brief pointer leave (< 0.25s) then re-enter must NOT start WaitingToHide
        sm.step(&base_input(1.0), IslandPosition::Top, mon, 12.0, None, size);
        let mut back_in = base_input(1.15);
        back_in.pointer_in_island = true;
        let out = sm.step(&back_in, IslandPosition::Top, mon, 12.0, None, size);
        assert_eq!(out.visibility, IslandVisibility::Visible);

        // Now leave and hide completely
        sm.step(&base_input(2.0), IslandPosition::Top, mon, 12.0, None, size);
        sm.step(&base_input(2.3), IslandPosition::Top, mon, 12.0, None, size);
        sm.step(&base_input(4.4), IslandPosition::Top, mon, 12.0, None, size);
        let mut t = 4.45;
        for _ in 0..8 {
            sm.step(&base_input(t), IslandPosition::Top, mon, 12.0, None, size);
            t += 0.05;
        }
        assert_eq!(sm.visibility, IslandVisibility::Hidden);

        // Restore via hover on tab
        let mut hover_tab = base_input(t);
        hover_tab.pointer_in_tab = true;
        sm.step(&hover_tab, IslandPosition::Top, mon, 12.0, None, size);
        for _ in 0..8 {
            t += 0.05;
            let mut inside = base_input(t);
            inside.pointer_in_island = true;
            sm.step(&inside, IslandPosition::Top, mon, 12.0, None, size);
        }
        assert_eq!(sm.visibility, IslandVisibility::Visible);

        // Leave after hover-restore: 0.25s hysteresis -> WaitingToHide -> 1.0s re-hide!
        sm.step(
            &base_input(t + 0.1),
            IslandPosition::Top,
            mon,
            12.0,
            None,
            size,
        );
        sm.step(
            &base_input(t + 0.4),
            IslandPosition::Top,
            mon,
            12.0,
            None,
            size,
        );
        assert_eq!(sm.visibility, IslandVisibility::WaitingToHide);
        let out = sm.step(
            &base_input(t + 1.5),
            IslandPosition::Top,
            mon,
            12.0,
            None,
            size,
        );
        assert_eq!(out.visibility, IslandVisibility::Hiding);
    }
}
