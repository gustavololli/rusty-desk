use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct M3uTrack {
    pub title: String,
    pub uri: String,
    pub duration_secs: Option<i64>,
}

/// Parses `.m3u` / `.m3u8` playlist text, resolving relative file paths against `base_dir`.
pub fn parse_m3u_content(content: &str, base_dir: Option<&Path>) -> Vec<M3uTrack> {
    let mut tracks = Vec::new();
    let mut pending_meta: Option<(Option<i64>, String)> = None;

    for raw_line in content.lines() {
        let line = raw_line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() {
            continue;
        }

        if let Some(extinf) = line.strip_prefix("#EXTINF:") {
            let (dur, title) = match extinf.split_once(',') {
                Some((dur_part, title_part)) => {
                    let parsed_dur = dur_part
                        .split_whitespace()
                        .next()
                        .and_then(|d| d.parse::<i64>().ok())
                        .filter(|&d| d >= 0);
                    (parsed_dur, title_part.trim().to_string())
                }
                None => (None, String::new()),
            };
            pending_meta = Some((dur, title));
            continue;
        }

        if line.starts_with('#') {
            continue;
        }

        let resolved_uri = resolve_track_uri(line, base_dir);
        let (duration_secs, meta_title) = pending_meta.take().unwrap_or((None, String::new()));
        let title = if !meta_title.is_empty() {
            meta_title
        } else {
            infer_title_from_uri(&resolved_uri)
        };

        tracks.push(M3uTrack {
            title,
            uri: resolved_uri,
            duration_secs,
        });
    }

    tracks
}

fn is_remote_or_uri(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("rtsp://")
        || lower.starts_with("file://")
}

fn resolve_track_uri(entry: &str, base_dir: Option<&Path>) -> String {
    if is_remote_or_uri(entry) {
        return entry.to_string();
    }
    let normalized = entry.replace('\\', "/");
    let p = Path::new(&normalized);
    if p.is_absolute() {
        return p.to_string_lossy().to_string();
    }
    if let Some(dir) = base_dir {
        dir.join(p).to_string_lossy().to_string()
    } else {
        normalized
    }
}

fn infer_title_from_uri(uri: &str) -> String {
    if is_remote_or_uri(uri) {
        return uri
            .rsplit('/')
            .find(|seg| !seg.is_empty())
            .unwrap_or(uri)
            .to_string();
    }
    Path::new(uri)
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(uri)
        .to_string()
}

/// Reads and parses a `.m3u` or `.m3u8` file from disk, returning `(playlist_name, tracks)`.
pub fn parse_m3u_file(path: &Path) -> Result<(String, Vec<M3uTrack>), String> {
    let bytes = fs::read(path).map_err(|e| format!("No se pudo leer '{}': {e}", path.display()))?;
    let content = String::from_utf8_lossy(&bytes);
    let tracks = parse_m3u_content(&content, path.parent());
    if tracks.is_empty() {
        return Err("El archivo .m3u no contiene pistas válidas.".to_string());
    }
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("Playlist M3U")
        .to_string();
    Ok((name, tracks))
}

/// Opens a native Linux file picker (`kdialog` or `zenity`) for `.m3u`/`.m3u8` files.
pub fn pick_m3u_file_dialog() -> Result<Option<PathBuf>, String> {
    if let Ok(out) = Command::new("kdialog")
        .args([
            "--getopenfilename",
            ".",
            "*.m3u *.m3u8|Playlists M3U (*.m3u, *.m3u8)",
        ])
        .output()
    {
        if out.status.success() {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            return Ok((!p.is_empty()).then(|| PathBuf::from(p)));
        }
        if out.status.code() == Some(1) {
            return Ok(None);
        }
    }

    if let Ok(out) = Command::new("zenity")
        .args([
            "--file-selection",
            "--title=Seleccionar playlist .m3u",
            "--file-filter=Playlists M3U (*.m3u *.m3u8) | *.m3u *.m3u8",
        ])
        .output()
    {
        if out.status.success() {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            return Ok((!p.is_empty()).then(|| PathBuf::from(p)));
        }
        if out.status.code() == Some(1) {
            return Ok(None);
        }
    }

    Err("No se encontró kdialog ni zenity para abrir el selector de archivos.".to_string())
}

/// Lightweight local audio player backed by `ffplay` (with fallbacks to `mpg123` / `gst-play-1.0`).
pub struct MusicPlayer {
    child: Option<Child>,
    volume: u8,
    last_live_vol_pid: Option<u32>,
}

impl Default for MusicPlayer {
    fn default() -> Self {
        Self {
            child: None,
            volume: 80,
            last_live_vol_pid: None,
        }
    }
}

impl Drop for MusicPlayer {
    fn drop(&mut self) {
        self.stop();
    }
}

impl MusicPlayer {
    pub fn is_playing(&mut self) -> bool {
        let Some(child) = self.child.as_mut() else {
            return false;
        };
        match child.try_wait() {
            Ok(None) => true,
            _ => {
                self.child = None;
                false
            }
        }
    }

    /// Returns `true` once if the active track finished playing on its own.
    pub fn poll_finished(&mut self) -> bool {
        let Some(child) = self.child.as_mut() else {
            return false;
        };
        match child.try_wait() {
            Ok(Some(_)) => {
                self.child = None;
                self.last_live_vol_pid = None;
                true
            }
            Ok(None) => false,
            Err(_) => {
                self.child = None;
                self.last_live_vol_pid = None;
                false
            }
        }
    }

    pub fn play(&mut self, uri: &str) -> Result<(), String> {
        self.stop();
        let vol_str = self.volume.min(100).to_string();

        if let Ok(child) = Command::new("ffplay")
            .args([
                "-nodisp",
                "-autoexit",
                "-loglevel",
                "quiet",
                "-volume",
                &vol_str,
                uri,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            self.last_live_vol_pid = Some(child.id());
            self.child = Some(child);
            return Ok(());
        }

        if let Ok(child) = Command::new("gst-play-1.0")
            .args([
                "--quiet",
                "--volume",
                &format!("{:.2}", f32::from(self.volume) / 100.0),
                uri,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            self.last_live_vol_pid = Some(child.id());
            self.child = Some(child);
            return Ok(());
        }

        if let Ok(child) = Command::new("mpg123")
            .args(["-q", uri])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            self.last_live_vol_pid = Some(child.id());
            self.child = Some(child);
            return Ok(());
        }

        Err("No se encontró un reproductor compatible (ffplay, gst-play-1.0 o mpg123).".to_string())
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.last_live_vol_pid = None;
    }

    pub fn set_volume(&mut self, new_volume: u8) {
        let clamped = new_volume.min(100);
        if self.volume == clamped {
            return;
        }
        self.volume = clamped;
        if let Some(pid) = self.child.as_ref().map(Child::id) {
            std::thread::spawn(move || {
                apply_pactl_volume_for_pid(pid, clamped);
            });
        }
    }
}

/// Best-effort live volume adjustment on PipeWire/PulseAudio without restarting playback.
fn apply_pactl_volume_for_pid(pid: u32, volume_pct: u8) {
    let Ok(out) = Command::new("pactl").args(["list", "sink-inputs"]).output() else {
        return;
    };
    if !out.status.success() {
        return;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let pid_str = pid.to_string();
    let mut current_id: Option<String> = None;

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Sink Input #") {
            current_id = Some(rest.trim().to_string());
        } else if trimmed.starts_with("application.process.id =") && trimmed.contains(&pid_str) {
            if let Some(id) = current_id.take() {
                let _ = Command::new("pactl")
                    .args(["set-sink-input-volume", &id, &format!("{volume_pct}%")])
                    .status();
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_extm3u_metadata_relative_paths_and_streams() {
        let m3u = "\
#EXTM3U
#EXTINF:185,Artist - Track One
songs/track1.mp3

#EXTINF:-1,LoFi Radio Stream
https://example.com/live.mp3
/abs/path/song2.flac
";
        let tracks = parse_m3u_content(m3u, Some(Path::new("/home/user/Music")));
        assert_eq!(tracks.len(), 3);
        assert_eq!(tracks[0].title, "Artist - Track One");
        assert_eq!(tracks[0].duration_secs, Some(185));
        assert_eq!(tracks[0].uri, "/home/user/Music/songs/track1.mp3");

        assert_eq!(tracks[1].title, "LoFi Radio Stream");
        assert_eq!(tracks[1].duration_secs, None);
        assert_eq!(tracks[1].uri, "https://example.com/live.mp3");

        assert_eq!(tracks[2].title, "song2");
        assert_eq!(tracks[2].uri, "/abs/path/song2.flac");
    }
}
