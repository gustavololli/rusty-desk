use directories::ProjectDirs;
use std::f32::consts::TAU;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy)]
pub enum SfxEvent {
    Startup,
    Click,
    MessageSent,
    MessageReceived,
    TaskAdded,
    TaskCompleted,
    HabitLogged,
    PomodoroStart,
    PomodoroDone,
    OrganizeDone,
    Error,
}

impl SfxEvent {
    pub fn file_name(self) -> &'static str {
        match self {
            Self::Startup => "startup.wav",
            Self::Click => "click.wav",
            Self::MessageSent => "message_sent.wav",
            Self::MessageReceived => "message_received.wav",
            Self::TaskAdded => "task_added.wav",
            Self::TaskCompleted => "task_completed.wav",
            Self::HabitLogged => "habit_logged.wav",
            Self::PomodoroStart => "pomodoro_start.wav",
            Self::PomodoroDone => "pomodoro_done.wav",
            Self::OrganizeDone => "organize_done.wav",
            Self::Error => "error.wav",
        }
    }
}

/// Directory where users can place custom Zero Hour `.wav` files to override built-in sounds.
pub fn custom_sounds_dir() -> PathBuf {
    if let Some(proj) = ProjectDirs::from("com", "rustydesk", "rusty-desk") {
        proj.config_dir().join("sounds")
    } else {
        PathBuf::from("sounds")
    }
}

fn cache_sounds_dir() -> PathBuf {
    if let Some(proj) = ProjectDirs::from("com", "rustydesk", "rusty-desk") {
        proj.cache_dir().join("sfx")
    } else {
        std::env::temp_dir().join("rusty-desk-sfx")
    }
}

/// Plays a sound effect asynchronously if `enabled` is true.
pub fn play(event: SfxEvent, enabled: bool) {
    if !enabled {
        return;
    }
    std::thread::spawn(move || {
        let custom_path = custom_sounds_dir().join(event.file_name());
        if custom_path.exists() {
            play_wav_file(&custom_path);
            return;
        }

        let cache_dir = cache_sounds_dir();
        let _ = fs::create_dir_all(&cache_dir);
        let cached_path = cache_dir.join(event.file_name());
        let needs_regen = fs::metadata(&cached_path)
            .map(|m| m.len() == 0)
            .unwrap_or(true);
        if needs_regen {
            if let Err(e) = write_synth_wav(&cached_path, event) {
                eprintln!("Error generando SFX {:?}: {e}", event);
                return;
            }
        }
        play_wav_file(&cached_path);
    });
}

fn play_wav_file(path: &Path) {
    let players: &[(&str, &[&str])] = &[
        ("pw-play", &[]),
        ("paplay", &[]),
        ("aplay", &["-q"]),
        ("ffplay", &["-nodisp", "-autoexit", "-loglevel", "quiet"]),
    ];

    for (bin, extra_args) in players {
        if let Ok(mut child) = Command::new(bin).args(*extra_args).arg(path).spawn() {
            if let Ok(status) = child.wait() {
                if status.success() {
                    return;
                }
            }
        }
    }
}

/// Generates crisp 8-bit tactical / RTS-inspired sound effects using `hound`.
fn write_synth_wav(path: &Path, event: SfxEvent) -> anyhow::Result<()> {
    const SR: u32 = 22050;
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SR,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut samples: Vec<f32> = Vec::new();
    match event {
        SfxEvent::Startup => {
            // "Battle control online" rising tactical chord
            append_tone(&mut samples, SR, 330.0, 0.08, Wave::Square, 0.22);
            append_tone(&mut samples, SR, 440.0, 0.08, Wave::Square, 0.24);
            append_tone(&mut samples, SR, 660.0, 0.16, Wave::Square, 0.26);
        }
        SfxEvent::Click => {
            // Tactical HUD button blip
            append_tone(&mut samples, SR, 1200.0, 0.025, Wave::Square, 0.16);
        }
        SfxEvent::MessageSent => {
            // Radio squelch blip
            append_tone(&mut samples, SR, 880.0, 0.035, Wave::Square, 0.18);
            append_tone(&mut samples, SR, 1320.0, 0.035, Wave::Square, 0.16);
        }
        SfxEvent::MessageReceived => {
            // Radar comm ping
            append_tone(&mut samples, SR, 987.77, 0.06, Wave::Triangle, 0.28);
            append_silence(&mut samples, SR, 0.02);
            append_tone(&mut samples, SR, 1318.5, 0.09, Wave::Triangle, 0.26);
        }
        SfxEvent::TaskAdded => {
            // "Unit / construction queued"
            append_tone(&mut samples, SR, 523.25, 0.06, Wave::Square, 0.22);
            append_tone(&mut samples, SR, 783.99, 0.09, Wave::Square, 0.24);
        }
        SfxEvent::TaskCompleted => {
            // "Building complete" 3-note fanfare
            append_tone(&mut samples, SR, 523.25, 0.07, Wave::Square, 0.22);
            append_tone(&mut samples, SR, 659.25, 0.07, Wave::Square, 0.24);
            append_tone(&mut samples, SR, 1046.5, 0.18, Wave::Square, 0.26);
        }
        SfxEvent::HabitLogged => {
            // Supply collected
            append_tone(&mut samples, SR, 659.25, 0.05, Wave::Triangle, 0.26);
            append_tone(&mut samples, SR, 987.77, 0.08, Wave::Triangle, 0.26);
        }
        SfxEvent::PomodoroStart => {
            // Power-up sweep
            append_sweep(&mut samples, SR, 280.0, 880.0, 0.22, 0.24);
        }
        SfxEvent::PomodoroDone => {
            // "Mission accomplished" 4-note cadence
            append_tone(&mut samples, SR, 440.0, 0.08, Wave::Square, 0.22);
            append_tone(&mut samples, SR, 554.37, 0.08, Wave::Square, 0.24);
            append_tone(&mut samples, SR, 659.25, 0.08, Wave::Square, 0.25);
            append_tone(&mut samples, SR, 880.0, 0.24, Wave::Square, 0.28);
        }
        SfxEvent::OrganizeDone => {
            // Rapid telemetry burst
            for freq in [600.0, 800.0, 1000.0, 1200.0] {
                append_tone(&mut samples, SR, freq, 0.04, Wave::Square, 0.20);
            }
        }
        SfxEvent::Error => {
            // "Cannot deploy here" low double buzz
            append_tone(&mut samples, SR, 160.0, 0.09, Wave::Saw, 0.28);
            append_silence(&mut samples, SR, 0.04);
            append_tone(&mut samples, SR, 130.0, 0.14, Wave::Saw, 0.28);
        }
    }

    let mut writer = hound::WavWriter::create(path, spec)?;
    for s in samples {
        writer.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
    }
    writer.finalize()?;
    Ok(())
}

#[derive(Clone, Copy)]
enum Wave {
    Square,
    Triangle,
    Saw,
}

fn append_silence(buf: &mut Vec<f32>, sr: u32, secs: f32) {
    let n = (sr as f32 * secs) as usize;
    buf.extend(std::iter::repeat_n(0.0, n));
}

fn append_tone(buf: &mut Vec<f32>, sr: u32, freq: f32, secs: f32, wave: Wave, amp: f32) {
    let n = (sr as f32 * secs) as usize;
    for i in 0..n {
        let t = i as f32 / sr as f32;
        let phase = (t * freq) % 1.0;
        let raw = match wave {
            Wave::Square => {
                if (t * freq * TAU).sin() >= 0.0 {
                    1.0
                } else {
                    -1.0
                }
            }
            Wave::Triangle => 4.0 * (phase - 0.5).abs() - 1.0,
            Wave::Saw => 2.0 * phase - 1.0,
        };
        // Short fade-in/out envelope to avoid clicks
        let env = ((i as f32 / 80.0).min(1.0)) * (((n - 1 - i) as f32 / 120.0).min(1.0));
        buf.push(raw * amp * env);
    }
}

fn append_sweep(buf: &mut Vec<f32>, sr: u32, f_start: f32, f_end: f32, secs: f32, amp: f32) {
    let n = (sr as f32 * secs) as usize;
    let mut phase = 0.0_f32;
    for i in 0..n {
        let frac = i as f32 / n.max(1) as f32;
        let freq = f_start + (f_end - f_start) * frac;
        phase = (phase + freq / sr as f32) % 1.0;
        let raw = if phase < 0.5 { 1.0 } else { -1.0 };
        let env = ((i as f32 / 80.0).min(1.0)) * (((n - 1 - i) as f32 / 140.0).min(1.0));
        buf.push(raw * amp * env);
    }
}
