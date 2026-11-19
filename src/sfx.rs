use directories::ProjectDirs;
use std::f32::consts::TAU;
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(not(windows))]
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
        // Keep old 8-bit-style cached files untouched; a new folder forces
        // regeneration without overriding custom user-supplied WAVs.
        proj.cache_dir().join("sfx-16bit-v1")
    } else {
        std::env::temp_dir().join("rusty-desk-sfx-16bit-v1")
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
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::Media::Audio::{PlaySoundW, SND_FILENAME, SND_NODEFAULT, SND_SYNC};
        let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        wide.push(0);
        let _ = unsafe {
            PlaySoundW(
                PCWSTR(wide.as_ptr()),
                None,
                SND_FILENAME | SND_NODEFAULT | SND_SYNC,
            )
        };
        return;
    }
    #[cfg(not(windows))]
    {
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
}

/// Generates layered, 16-bit-console-inspired stereo effects using `hound`.
fn write_synth_wav(path: &Path, event: SfxEvent) -> anyhow::Result<()> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for [left, right] in synthesize(event) {
        writer.write_sample((left * i16::MAX as f32) as i16)?;
        writer.write_sample((right * i16::MAX as f32) as i16)?;
    }
    writer.finalize()?;
    Ok(())
}

const SAMPLE_RATE: u32 = 44_100;

#[derive(Clone, Copy)]
enum Timbre {
    Warm,
    Bell,
    Pluck,
    Bass,
}

#[derive(Clone, Copy)]
struct Voice {
    at: f32,
    hz: f32,
    secs: f32,
    gain: f32,
    pan: f32,
    timbre: Timbre,
}

fn score(event: SfxEvent) -> Vec<Voice> {
    use Timbre::*;
    let note = |at, hz, secs, gain, pan, timbre| Voice {
        at,
        hz,
        secs,
        gain,
        pan,
        timbre,
    };
    match event {
        SfxEvent::Startup => vec![
            note(0.00, 392.00, 0.26, 0.22, -0.3, Warm),
            note(0.08, 493.88, 0.27, 0.20, 0.2, Bell),
            note(0.16, 587.33, 0.37, 0.21, 0.0, Bell),
            note(0.25, 783.99, 0.32, 0.14, 0.25, Warm),
        ],
        SfxEvent::Click => vec![note(0.0, 1046.50, 0.045, 0.12, 0.0, Pluck)],
        SfxEvent::MessageSent => vec![
            note(0.00, 783.99, 0.085, 0.17, -0.2, Pluck),
            note(0.06, 1174.66, 0.11, 0.16, 0.2, Bell),
        ],
        SfxEvent::MessageReceived => vec![
            note(0.00, 987.77, 0.13, 0.19, -0.2, Bell),
            note(0.10, 1318.51, 0.19, 0.19, 0.3, Bell),
        ],
        SfxEvent::TaskAdded => vec![
            note(0.00, 523.25, 0.12, 0.19, -0.1, Pluck),
            note(0.09, 783.99, 0.16, 0.19, 0.2, Warm),
        ],
        SfxEvent::TaskCompleted => vec![
            note(0.00, 523.25, 0.16, 0.19, -0.25, Pluck),
            note(0.10, 659.25, 0.19, 0.19, 0.25, Pluck),
            note(0.20, 1046.50, 0.30, 0.21, 0.0, Bell),
            note(0.23, 783.99, 0.27, 0.09, -0.3, Warm),
        ],
        SfxEvent::HabitLogged => vec![
            note(0.00, 659.25, 0.13, 0.17, -0.25, Pluck),
            note(0.08, 987.77, 0.20, 0.18, 0.25, Bell),
        ],
        SfxEvent::PomodoroStart => vec![
            note(0.00, 392.00, 0.25, 0.20, -0.25, Warm),
            note(0.07, 587.33, 0.32, 0.19, 0.25, Warm),
            note(0.14, 783.99, 0.34, 0.16, 0.0, Bell),
        ],
        SfxEvent::PomodoroDone => vec![
            note(0.00, 440.00, 0.15, 0.18, -0.3, Pluck),
            note(0.11, 554.37, 0.17, 0.19, 0.2, Pluck),
            note(0.22, 659.25, 0.18, 0.20, -0.15, Pluck),
            note(0.34, 880.00, 0.34, 0.20, 0.25, Bell),
            note(0.36, 659.25, 0.31, 0.10, -0.25, Warm),
        ],
        SfxEvent::OrganizeDone => (0..4)
            .map(|i| {
                note(
                    i as f32 * 0.052,
                    [587.33, 698.46, 880.00, 1174.66][i],
                    0.12,
                    0.15,
                    if i % 2 == 0 { -0.25 } else { 0.25 },
                    Pluck,
                )
            })
            .collect(),
        SfxEvent::Error => vec![
            note(0.00, 196.00, 0.17, 0.23, -0.15, Bass),
            note(0.15, 164.81, 0.22, 0.22, 0.15, Bass),
        ],
    }
}

fn synthesize(event: SfxEvent) -> Vec<[f32; 2]> {
    let voices = score(event);
    let duration = voices.iter().map(|v| v.at + v.secs).fold(0.0_f32, f32::max) + 0.16;
    let mut stereo = vec![[0.0_f32; 2]; (duration * SAMPLE_RATE as f32).ceil() as usize];

    for voice in voices {
        let from = (voice.at * SAMPLE_RATE as f32) as usize;
        let frames = (voice.secs * SAMPLE_RATE as f32) as usize;
        for i in 0..frames {
            let t = i as f32 / SAMPLE_RATE as f32;
            let progress = t / voice.secs;
            let attack = (t / 0.008).min(1.0);
            let release = ((voice.secs - t) / 0.045).clamp(0.0, 1.0);
            let decay = (1.0 - progress * 0.40).max(0.0);
            let extra_decay = if matches!(voice.timbre, Timbre::Pluck | Timbre::Bell) {
                (-2.4 * progress).exp()
            } else {
                1.0
            };
            let phase = TAU * voice.hz * t + 0.013 * (TAU * 5.2 * t).sin();
            let raw = match voice.timbre {
                Timbre::Warm => {
                    0.72 * phase.sin() + 0.20 * (phase * 2.0).sin() + 0.08 * (phase * 3.0).sin()
                }
                Timbre::Bell => {
                    0.67 * (phase + 0.36 * (phase * 2.01).sin()).sin() + 0.33 * (phase * 2.4).sin()
                }
                Timbre::Pluck => {
                    0.62 * phase.sin() + 0.25 * (phase * 2.0).sin() + 0.13 * (phase * 3.0).sin()
                }
                Timbre::Bass => 0.82 * phase.sin() + 0.18 * (phase * 2.0).sin(),
            };
            let sample = raw * voice.gain * attack * release * decay * extra_decay;
            let left = ((1.0 - voice.pan) * 0.5).sqrt();
            let right = ((1.0 + voice.pan) * 0.5).sqrt();
            stereo[from + i][0] += sample * left;
            stereo[from + i][1] += sample * right;
        }
    }

    // Subtle cross-channel echoes: a touch of 16-bit-era depth, no full reverb engine.
    for (delay_ms, feedback) in [(72, 0.12), (124, 0.065)] {
        let delay = (SAMPLE_RATE as usize * delay_ms) / 1000;
        for i in delay..stereo.len() {
            let earlier = stereo[i - delay];
            stereo[i][0] += earlier[1] * feedback;
            stereo[i][1] += earlier[0] * feedback;
        }
    }
    let peak = stereo
        .iter()
        .flat_map(|lr| lr.iter())
        .fold(0.0_f32, |m, s| m.max(s.abs()));
    let scale = if peak > 0.78 { 0.78 / peak } else { 1.0 };
    for frame in &mut stereo {
        frame[0] *= scale;
        frame[1] *= scale;
    }
    stereo
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effects_are_stereo_bounded_and_short() {
        let all = [
            SfxEvent::Startup,
            SfxEvent::Click,
            SfxEvent::MessageSent,
            SfxEvent::MessageReceived,
            SfxEvent::TaskAdded,
            SfxEvent::TaskCompleted,
            SfxEvent::HabitLogged,
            SfxEvent::PomodoroStart,
            SfxEvent::PomodoroDone,
            SfxEvent::OrganizeDone,
            SfxEvent::Error,
        ];
        for event in all {
            let frames = synthesize(event);
            assert!(frames.len() > 100 && frames.len() < SAMPLE_RATE as usize);
            assert!(frames
                .iter()
                .flatten()
                .all(|v| v.is_finite() && v.abs() <= 0.781));
            assert!(frames.iter().flatten().any(|v| v.abs() > 0.001));
        }
    }

    #[test]
    fn generated_wav_is_16_bit_stereo_44100() {
        let path = std::env::temp_dir().join(format!("rusty_sfx_{}.wav", uuid::Uuid::new_v4()));
        write_synth_wav(&path, SfxEvent::MessageReceived).expect("write test wav");
        let reader = hound::WavReader::open(&path).expect("read test wav");
        let spec = reader.spec();
        assert_eq!(spec.channels, 2);
        assert_eq!(spec.bits_per_sample, 16);
        assert_eq!(spec.sample_rate, SAMPLE_RATE);
        drop(reader);
        let _ = fs::remove_file(path);
    }
}
