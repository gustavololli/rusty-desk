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
const TAB_THICKNESS: f32 = 10.0;
const TAB_LENGTH: f32 = 88.0;

pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
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
    /// Linear animation progress in `0.0..=1.0` for Hiding/Showing.
    anim_t: f32,
    last_applied: Option<([f32; 2], Option<[f32; 2]>)>,
}

impl Default for IslandStateMachine {
    fn default() -> Self {
        Self {
            visibility: IslandVisibility::Visible,
            hide_timer_start: None,
            anim_t: 1.0,
            last_applied: None,
        }
    }
}

impl IslandStateMachine {
    pub fn reset_visible(&mut self) {
        self.visibility = IslandVisibility::Visible;
        self.hide_timer_start = None;
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

    fn stay_visible_reason(input: &IslandInput) -> bool {
        !input.auto_hide
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
        let keep_visible = Self::stay_visible_reason(input);
        let delay_secs = f64::from(input.hide_delay_secs.max(1));
        let dt = input.dt.clamp(0.0, 0.1);

        match self.visibility {
            IslandVisibility::Visible => {
                if keep_visible {
                    self.hide_timer_start = None;
                } else {
                    self.visibility = IslandVisibility::WaitingToHide;
                    self.hide_timer_start = Some(input.now);
                }
            }
            IslandVisibility::WaitingToHide => {
                if keep_visible {
                    self.visibility = IslandVisibility::Visible;
                    self.hide_timer_start = None;
                } else {
                    let started = *self.hide_timer_start.get_or_insert(input.now);
                    if input.now - started >= delay_secs {
                        self.visibility = IslandVisibility::Hiding;
                        self.anim_t = 0.0;
                        self.hide_timer_start = None;
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
                    || input.is_recording
                    || (input.music_active && input.keep_visible_during_playback)
                    || (input.ai_busy && input.keep_visible_during_ai);
                let wake_for_hover = input.show_on_hover && input.pointer_in_tab;
                if wake_for_activity || wake_for_hover {
                    self.visibility = IslandVisibility::Showing;
                    self.anim_t = 0.0;
                }
            }
            IslandVisibility::Showing => {
                self.anim_t = (self.anim_t + dt / SHOW_ANIM_SECS).min(1.0);
                if self.anim_t >= 1.0 {
                    self.visibility = IslandVisibility::Visible;
                    self.hide_timer_start = None;
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

        // Mouse leaves -> WaitingToHide
        let out = sm.step(&base_input(1.0), IslandPosition::Top, mon, 12.0, None, size);
        assert_eq!(out.visibility, IslandVisibility::WaitingToHide);

        // Before 2s -> still WaitingToHide
        let out = sm.step(&base_input(2.5), IslandPosition::Top, mon, 12.0, None, size);
        assert_eq!(out.visibility, IslandVisibility::WaitingToHide);

        // After 2s -> Hiding
        let out = sm.step(&base_input(3.1), IslandPosition::Top, mon, 12.0, None, size);
        assert_eq!(out.visibility, IslandVisibility::Hiding);
        assert!(out.is_animating);

        // Advance past 250ms -> Hidden tab
        let mut t = 3.15;
        let mut hidden_out = out;
        for _ in 0..8 {
            hidden_out = sm.step(&base_input(t), IslandPosition::Top, mon, 12.0, None, size);
            t += 0.05;
        }
        assert_eq!(hidden_out.visibility, IslandVisibility::Hidden);
        assert_eq!(hidden_out.size, [88.0, 10.0]);
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

        // Start waiting
        sm.step(
            &base_input(1.0),
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
            let mut t = 3.1;
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

        // Dragged near top -> contracts vertically to horizontal pill [88, 10]
        let top_out = collapse([780.0, 10.0]);
        assert_eq!(top_out.size, [TAB_LENGTH, TAB_THICKNESS]);
        assert_eq!(top_out.pos, Some([780.0 + (360.0 - TAB_LENGTH) * 0.5, 10.0]));

        // Dragged near bottom -> contracts vertically to horizontal pill at bottom of rect
        let bot_out = collapse([780.0, 1000.0]);
        assert_eq!(bot_out.size, [TAB_LENGTH, TAB_THICKNESS]);
        assert_eq!(
            bot_out.pos,
            Some([780.0 + (360.0 - TAB_LENGTH) * 0.5, 1000.0 + 66.0 - TAB_THICKNESS])
        );

        // Dragged near left -> contracts horizontally to vertical pill [10, 88]
        let left_out = collapse([8.0, 500.0]);
        assert_eq!(left_out.size, [TAB_THICKNESS, TAB_LENGTH]);
        assert_eq!(
            left_out.pos,
            Some([8.0, 500.0 + (66.0 - TAB_LENGTH) * 0.5])
        );

        // Dragged near right -> contracts horizontally to vertical pill [10, 88] at right edge
        let right_out = collapse([1550.0, 500.0]);
        assert_eq!(right_out.size, [TAB_THICKNESS, TAB_LENGTH]);
        assert_eq!(
            right_out.pos,
            Some([1550.0 + 360.0 - TAB_THICKNESS, 500.0 + (66.0 - TAB_LENGTH) * 0.5])
        );
    }
}
