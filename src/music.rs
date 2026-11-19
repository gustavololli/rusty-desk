use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};

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

pub fn is_remote_or_uri(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("rtsp://")
        || lower.starts_with("file://")
}

pub fn normalize_audio_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Ingresá una URL válida.".to_string());
    }
    if trimmed.chars().any(char::is_whitespace) {
        return Err("La URL no puede contener espacios.".to_string());
    }
    if !is_remote_or_uri(trimmed) {
        return Err("La URL debe comenzar con http://, https://, rtsp:// o file://".to_string());
    }
    Ok(trimmed.to_string())
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

pub fn infer_title_from_uri(uri: &str) -> String {
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

/// How `/play` chooses a track. A number is 1-based and only applies when it
/// fits the list that is already loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaySelector {
    Current,
    Index(usize),
    Text(String),
}

pub fn classify_play_query(query: Option<&str>, current_len: usize) -> PlaySelector {
    let Some(raw) = query.map(str::trim).filter(|q| !q.is_empty()) else {
        return PlaySelector::Current;
    };
    if let Ok(num) = raw.parse::<usize>() {
        if num >= 1 && num <= current_len {
            return PlaySelector::Index(num - 1);
        }
    }
    PlaySelector::Text(raw.to_string())
}

/// Case-insensitive match against a track title or URI. `tracks` is `(title, uri)`.
pub fn find_track_index(tracks: &[(&str, &str)], needle: &str) -> Option<usize> {
    let needle = needle.to_lowercase();
    tracks.iter().position(|(title, uri)| {
        title.to_lowercase().contains(&needle) || uri.to_lowercase().contains(&needle)
    })
}

/// Local players tried in order. ffplay is first because its volume is 0..=100.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerLaunch {
    pub program: &'static str,
    pub args: Vec<String>,
}

pub fn player_launch_plans(uri: &str) -> Vec<PlayerLaunch> {
    vec![
        PlayerLaunch {
            program: "ffplay",
            args: vec![
                "-nodisp".into(),
                "-autoexit".into(),
                "-loglevel".into(),
                "quiet".into(),
                "-volume".into(),
                "100".into(),
                uri.into(),
            ],
        },
        PlayerLaunch {
            program: "gst-play-1.0",
            args: vec![
                "--quiet".into(),
                "--volume".into(),
                "1.00".into(),
                uri.into(),
            ],
        },
        PlayerLaunch {
            program: "mpg123",
            args: vec!["-q".into(), uri.into()],
        },
    ]
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
    #[cfg(windows)]
    {
        return Ok(rfd::FileDialog::new()
            .set_title("Seleccionar playlist")
            .add_filter("Playlists", &["m3u", "m3u8"])
            .pick_file());
    }
    #[cfg(not(windows))]
    {
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
}

/// Abstraction for applying live volume and mute state to an active player process.
pub trait VolumeBackend: Send + Sync {
    fn apply_pid_volume(&self, pid: u32, volume_pct: u8, muted: bool) -> bool;
}

/// PulseAudio / PipeWire `pactl` backend that adjusts only rusty-desk's sink-input.
#[derive(Debug, Default)]
pub struct PactlVolumeBackend;

impl VolumeBackend for PactlVolumeBackend {
    fn apply_pid_volume(&self, pid: u32, volume_pct: u8, muted: bool) -> bool {
        let Ok(out) = Command::new("pactl").args(["list", "sink-inputs"]).output() else {
            return false;
        };
        if !out.status.success() {
            return false;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let mut ids = parse_sink_input_ids_for_pid(&text, pid);
        if ids.is_empty() {
            if let Ok(clients_out) = Command::new("pactl").args(["list", "clients"]).output() {
                if clients_out.status.success() {
                    let clients_text = String::from_utf8_lossy(&clients_out.stdout);
                    let client_ids = parse_client_ids_for_pid(&clients_text, pid);
                    if !client_ids.is_empty() {
                        ids = parse_sink_input_ids_for_clients(&text, &client_ids);
                    }
                }
            }
        }
        if ids.is_empty() {
            return false;
        }

        let vol_arg = format!("{}%", volume_pct.min(100));
        let mute_arg = if muted { "1" } else { "0" };
        let mut applied_any = false;
        for id in &ids {
            let vol_ok = Command::new("pactl")
                .args(["set-sink-input-volume", id, &vol_arg])
                .status()
                .is_ok_and(|s| s.success());
            let mute_ok = Command::new("pactl")
                .args(["set-sink-input-mute", id, mute_arg])
                .status()
                .is_ok_and(|s| s.success());
            applied_any |= vol_ok && mute_ok;
        }
        applied_any
    }
}

/// Adjusts only the external player's Windows audio session, not master volume.
#[cfg(windows)]
#[derive(Debug, Default)]
pub struct WindowsVolumeBackend;

#[cfg(windows)]
impl VolumeBackend for WindowsVolumeBackend {
    fn apply_pid_volume(&self, pid: u32, volume_pct: u8, muted: bool) -> bool {
        use windows::core::Interface;
        use windows::Win32::Media::Audio::{
            eMultimedia, eRender, IAudioSessionControl2, IAudioSessionManager2,
            IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator,
        };
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
        };

        // All COM interfaces remain inside this worker thread.
        unsafe {
            if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
                return false;
            }
            struct UninitCom;
            impl Drop for UninitCom {
                fn drop(&mut self) {
                    unsafe {
                        CoUninitialize();
                    }
                }
            }
            let _com = UninitCom;
            let apply = || -> windows::core::Result<bool> {
                let enumerator: IMMDeviceEnumerator =
                    CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
                let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
                let manager: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None)?;
                let sessions = manager.GetSessionEnumerator()?;
                let mut matched = false;
                for i in 0..sessions.GetCount()? {
                    let session = sessions.GetSession(i)?;
                    let info: IAudioSessionControl2 = session.cast()?;
                    if info.GetProcessId()? != pid {
                        continue;
                    }
                    let control: ISimpleAudioVolume = session.cast()?;
                    control
                        .SetMasterVolume(volume_pct.min(100) as f32 / 100.0, std::ptr::null())?;
                    control.SetMute(muted, std::ptr::null())?;
                    matched = true;
                }
                Ok(matched)
            };
            apply().unwrap_or(false)
        }
    }
}

/// Parses `pactl list` block output and returns all block IDs whose `field_prefix` matches `predicate`.
fn parse_block_ids_matching(
    text: &str,
    header_prefix: &str,
    field_prefix: &str,
    mut predicate: impl FnMut(&str) -> bool,
) -> Vec<String> {
    let mut current_id: Option<String> = None;
    let mut matches = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(header_prefix) {
            current_id = Some(rest.trim().to_string());
        } else if let Some(val) = trimmed.strip_prefix(field_prefix) {
            if predicate(val.trim()) {
                if let Some(id) = current_id.take() {
                    matches.push(id);
                }
            }
        }
    }
    matches
}

/// Parses `pactl list sink-inputs` output and returns the first sink-input ID matching `pid`.
#[allow(dead_code)]
pub fn parse_sink_input_id_for_pid(text: &str, pid: u32) -> Option<String> {
    parse_sink_input_ids_for_pid(text, pid).into_iter().next()
}

/// Parses `pactl list sink-inputs` output and returns all sink-input IDs matching `pid`.
pub fn parse_sink_input_ids_for_pid(text: &str, pid: u32) -> Vec<String> {
    let exact_pid = format!("\"{pid}\"");
    parse_block_ids_matching(text, "Sink Input #", "application.process.id =", |v| {
        v == exact_pid
    })
}

/// Parses `pactl list clients` output and returns all client IDs belonging to `pid`.
///
/// Native PipeWire clients (e.g. `ffplay` via SDL's PipeWire backend) expose
/// `application.process.id` on the client object rather than on the sink-input node.
pub fn parse_client_ids_for_pid(clients_text: &str, pid: u32) -> Vec<String> {
    let exact_pid = format!("\"{pid}\"");
    parse_block_ids_matching(clients_text, "Client #", "application.process.id =", |v| {
        v == exact_pid
    })
}

/// Parses `pactl list sink-inputs` output and returns all sink-input IDs owned by `client_ids`.
pub fn parse_sink_input_ids_for_clients(
    sink_inputs_text: &str,
    client_ids: &[String],
) -> Vec<String> {
    parse_block_ids_matching(sink_inputs_text, "Sink Input #", "Client:", |v| {
        client_ids.iter().any(|c| c == v)
    })
}

#[derive(Debug, Clone, Copy)]
struct VolumeTarget {
    pid: u32,
    volume: u8,
    muted: bool,
    retries_left: u8,
    gen: u64,
}

/// Lightweight local audio player backed by `ffplay` (with fallbacks to `gst-play-1.0` / `mpg123`).
pub struct MusicPlayer {
    child: Option<Child>,
    volume: u8,
    muted: bool,
    paused: bool,
    gen: u64,
    worker_state: Arc<(Mutex<Option<VolumeTarget>>, Condvar)>,
}

impl Default for MusicPlayer {
    fn default() -> Self {
        #[cfg(windows)]
        {
            return Self::with_backend(Arc::new(WindowsVolumeBackend));
        }
        #[cfg(not(windows))]
        Self::with_backend(Arc::new(PactlVolumeBackend))
    }
}

impl Drop for MusicPlayer {
    fn drop(&mut self) {
        self.stop();
    }
}

impl MusicPlayer {
    pub fn with_backend(backend: Arc<dyn VolumeBackend>) -> Self {
        let worker_state: Arc<(Mutex<Option<VolumeTarget>>, Condvar)> =
            Arc::new((Mutex::new(None), Condvar::new()));
        let state_clone = Arc::clone(&worker_state);

        std::thread::spawn(move || {
            let mut last_applied_gen: Option<u64> = None;
            loop {
                let target_opt = {
                    let (lock, cvar) = &*state_clone;
                    let mut guard = match lock.lock() {
                        Ok(g) => g,
                        Err(_) => return,
                    };
                    while guard.is_none()
                        || guard
                            .as_ref()
                            .is_some_and(|t| last_applied_gen == Some(t.gen) && t.retries_left == 0)
                    {
                        guard = match cvar.wait(guard) {
                            Ok(g) => g,
                            Err(_) => return,
                        };
                    }
                    *guard
                };

                let Some(target) = target_opt else {
                    continue;
                };

                let applied = backend.apply_pid_volume(target.pid, target.volume, target.muted);
                let mut should_sleep = false;
                if let Ok(mut guard) = state_clone.0.lock() {
                    if let Some(current) = guard.as_mut() {
                        if current.gen == target.gen {
                            last_applied_gen = Some(target.gen);
                            if applied {
                                current.retries_left = 0;
                            } else if current.retries_left > 0 {
                                current.retries_left -= 1;
                                should_sleep = true;
                            }
                        }
                    }
                }
                if should_sleep {
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
            }
        });

        Self {
            child: None,
            volume: 80,
            muted: false,
            paused: false,
            gen: 0,
            worker_state,
        }
    }

    #[allow(dead_code)]
    pub fn volume(&self) -> u8 {
        self.volume
    }

    #[allow(dead_code)]
    pub fn is_muted(&self) -> bool {
        self.muted
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Returns `true` if a track is loaded (playing OR paused).
    pub fn is_active(&mut self) -> bool {
        let Some(child) = self.child.as_mut() else {
            self.paused = false;
            return false;
        };
        match child.try_wait() {
            Ok(None) => true,
            _ => {
                self.child = None;
                self.paused = false;
                false
            }
        }
    }

    /// Returns `true` only when actively playing (not paused).
    pub fn is_playing(&mut self) -> bool {
        self.is_active() && !self.paused
    }

    /// Returns `true` once if the active track finished playing on its own.
    pub fn poll_finished(&mut self) -> bool {
        let Some(child) = self.child.as_mut() else {
            return false;
        };
        match child.try_wait() {
            Ok(Some(_)) => {
                self.child = None;
                self.paused = false;
                self.clear_worker_target();
                true
            }
            Ok(None) => false,
            Err(_) => {
                self.child = None;
                self.paused = false;
                self.clear_worker_target();
                false
            }
        }
    }

    pub fn play(&mut self, uri: &str) -> Result<(), String> {
        self.stop();
        self.paused = false;

        // Spawn at 100% internal gain so `pactl` can scale smoothly across 0..=100% and mute/unmute.
        for plan in player_launch_plans(uri) {
            if let Ok(child) = Command::new(plan.program)
                .args(&plan.args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                let pid = child.id();
                self.child = Some(child);
                self.push_worker_target(pid, 40);
                return Ok(());
            }
        }

        #[cfg(windows)]
        {
            Err("No se encontró ffplay. Instalá FFmpeg y agregá ffplay.exe al PATH para reproducir música en Windows.".to_string())
        }
        #[cfg(not(windows))]
        {
            Err(
                "No se encontró un reproductor compatible (ffplay, gst-play-1.0 o mpg123)."
                    .to_string(),
            )
        }
    }

    pub fn pause(&mut self) {
        if self.paused {
            return;
        }
        #[cfg(windows)]
        {
            // ffplay's Linux SIGSTOP pause mechanism does not exist on Windows.
            // Never report a paused player while audio is still running.
            return;
        }
        #[cfg(not(windows))]
        {
            if let Some(pid) = self.child.as_ref().map(Child::id) {
                let _ = Command::new("kill")
                    .args(["-STOP", &pid.to_string()])
                    .status();
                self.paused = true;
            }
        }
    }

    pub fn resume(&mut self) {
        if !self.paused {
            return;
        }
        #[cfg(windows)]
        {
            return;
        }
        #[cfg(not(windows))]
        {
            if let Some(pid) = self.child.as_ref().map(Child::id) {
                let _ = Command::new("kill")
                    .args(["-CONT", &pid.to_string()])
                    .status();
                self.paused = false;
                self.push_worker_target(pid, 4);
            }
        }
    }

    pub fn toggle_pause(&mut self) {
        if self.paused {
            self.resume();
        } else {
            self.pause();
        }
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            #[cfg(not(windows))]
            if self.paused {
                let _ = Command::new("kill")
                    .args(["-CONT", &child.id().to_string()])
                    .status();
            }
            let _ = child.kill();
            let _ = child.wait();
        }
        self.paused = false;
        self.clear_worker_target();
    }

    pub fn set_volume(&mut self, new_volume: u8) {
        let clamped = new_volume.min(100);
        if self.volume == clamped {
            return;
        }
        self.volume = clamped;
        if let Some(pid) = self.child.as_ref().map(Child::id) {
            self.push_worker_target(pid, 4);
        }
    }

    pub fn set_muted(&mut self, muted: bool) {
        if self.muted == muted {
            return;
        }
        self.muted = muted;
        if let Some(pid) = self.child.as_ref().map(Child::id) {
            self.push_worker_target(pid, 4);
        }
    }

    pub fn toggle_mute(&mut self) -> bool {
        let next = !self.muted;
        self.set_muted(next);
        next
    }

    fn push_worker_target(&mut self, pid: u32, retries_left: u8) {
        self.gen = self.gen.wrapping_add(1);
        let target = VolumeTarget {
            pid,
            volume: self.volume,
            muted: self.muted,
            retries_left,
            gen: self.gen,
        };
        let (lock, cvar) = &*self.worker_state;
        if let Ok(mut guard) = lock.lock() {
            *guard = Some(target);
            cvar.notify_all();
        }
    }

    fn clear_worker_target(&self) {
        let (lock, cvar) = &*self.worker_state;
        if let Ok(mut guard) = lock.lock() {
            *guard = None;
            cvar.notify_all();
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

    #[test]
    fn normalizes_and_validates_audio_urls() {
        assert_eq!(
            normalize_audio_url("  https://radio.example.com/live.mp3  ").unwrap(),
            "https://radio.example.com/live.mp3"
        );
        assert_eq!(
            normalize_audio_url("http://stream.local:8000/radio").unwrap(),
            "http://stream.local:8000/radio"
        );
        assert!(normalize_audio_url("").is_err());
        assert!(normalize_audio_url("https://bad url.com/a.mp3").is_err());
        assert!(normalize_audio_url("/local/file.mp3").is_err());
    }

    #[test]
    fn parses_exact_sink_input_pid_without_prefix_collisions() {
        let pactl_out = "\
Sink Input #41
\tDriver: PipeWire
\tapplication.process.id = \"1234\"
Sink Input #42
\tDriver: PipeWire
\tapplication.process.id = \"123\"
";
        assert_eq!(
            parse_sink_input_id_for_pid(pactl_out, 123).as_deref(),
            Some("42")
        );
        assert_eq!(
            parse_sink_input_id_for_pid(pactl_out, 1234).as_deref(),
            Some("41")
        );
        assert_eq!(parse_sink_input_id_for_pid(pactl_out, 999), None);
    }

    #[test]
    fn resolves_pipewire_sink_input_via_client_pid_when_node_lacks_process_id() {
        let sink_inputs = "\
Sink Input #113
\tDriver: PipeWire
\tClient: 112
\tProperties:
\t\tapplication.name = \"ffplay\"
Sink Input #176
\tDriver: PipeWire
\tClient: 175
\tProperties:
\t\tapplication.name = \"ffplay\"
";
        let clients = "\
Client #174
\tProperties:
\t\tapplication.name = \"ffplay\"
\t\tapplication.process.id = \"3659\"
Client #175
\tProperties:
\t\tapplication.name = \"ffplay\"
\t\tapplication.process.id = \"3659\"
Client #112
\tProperties:
\t\tapplication.name = \"ffplay\"
\t\tapplication.process.id = \"2151\"
";
        assert!(parse_sink_input_ids_for_pid(sink_inputs, 3659).is_empty());
        let client_ids = parse_client_ids_for_pid(clients, 3659);
        assert_eq!(client_ids, vec!["174", "175"]);
        assert_eq!(
            parse_sink_input_ids_for_clients(sink_inputs, &client_ids),
            vec!["176"]
        );
        // Prefix collisions on PID must not match, and empty client list yields no sink-inputs.
        assert!(parse_client_ids_for_pid(clients, 365).is_empty());
        assert!(parse_sink_input_ids_for_clients(sink_inputs, &[]).is_empty());
    }

    struct RecordingVolumeBackend {
        calls: Mutex<Vec<(u32, u8, bool)>>,
    }

    impl RecordingVolumeBackend {
        fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl VolumeBackend for RecordingVolumeBackend {
        fn apply_pid_volume(&self, pid: u32, volume_pct: u8, muted: bool) -> bool {
            if let Ok(mut guard) = self.calls.lock() {
                guard.push((pid, volume_pct, muted));
            }
            true
        }
    }

    #[test]
    fn slider_volume_and_mute_reach_audio_backend_and_preserve_previous_volume() {
        let backend = Arc::new(RecordingVolumeBackend::new());
        let mut player = MusicPlayer::with_backend(backend.clone());

        // Spawn a lightweight silent sleeper process as a stand-in child PID.
        let child = Command::new("sleep")
            .arg("2")
            .spawn()
            .expect("sleep should spawn");
        let pid = child.id();
        player.child = Some(child);

        for target_vol in [0u8, 25, 50, 100, 65] {
            player.set_volume(target_vol);
            assert_eq!(player.volume(), target_vol);
        }

        // Mute keeps previous volume and sends muted=true.
        player.set_muted(true);
        assert!(player.is_muted());
        assert_eq!(player.volume(), 65);

        // Unmute restores previous volume and sends muted=false.
        player.set_muted(false);
        assert!(!player.is_muted());
        assert_eq!(player.volume(), 65);

        // Pause / resume does not reset volume or mute.
        player.pause();
        assert!(player.is_paused());
        player.resume();
        assert!(!player.is_paused());
        assert_eq!(player.volume(), 65);

        // Wait briefly for worker thread to flush.
        std::thread::sleep(std::time::Duration::from_millis(60));
        player.stop();

        let calls = backend.calls.lock().unwrap();
        assert!(
            !calls.is_empty(),
            "volume backend must receive updates from slider/mute"
        );
        assert!(calls.iter().any(|&(p, v, m)| p == pid && v == 65 && !m));
    }

    #[test]
    fn player_launch_plans_prefer_ffplay_and_keep_the_uri_as_data() {
        let plans = player_launch_plans("https://radio.example.com/live.mp3");
        assert_eq!(
            plans.iter().map(|plan| plan.program).collect::<Vec<_>>(),
            vec!["ffplay", "gst-play-1.0", "mpg123"]
        );
        assert_eq!(
            plans[0].args,
            [
                "-nodisp",
                "-autoexit",
                "-loglevel",
                "quiet",
                "-volume",
                "100",
                "https://radio.example.com/live.mp3",
            ]
        );
        assert_eq!(plans[1].args[1], "--volume");
        assert_eq!(plans[1].args[2], "1.00");
        assert_eq!(plans[2].args, ["-q", "https://radio.example.com/live.mp3"]);
        for plan in &plans {
            assert_eq!(
                plan.args.last().map(String::as_str),
                Some("https://radio.example.com/live.mp3")
            );
            assert!(plan.args.iter().all(|arg| !arg.contains(' ')));
        }
    }

    #[test]
    fn playlist_parser_skips_comments_and_titles_come_from_the_uri() {
        let m3u = "\u{feff}# comment\n#EXTINF:not-a-number,  \nsongs\\nested\\track.mp3\n\nonly-a-file.ogg\n";
        let tracks = parse_m3u_content(m3u, Some(Path::new("/music")));
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].duration_secs, None);
        assert_eq!(tracks[0].title, "track");
        assert_eq!(tracks[0].uri, "/music/songs/nested/track.mp3");
        assert_eq!(tracks[1].title, "only-a-file");
        assert_eq!(
            infer_title_from_uri("https://cdn.example/a/b.mp3?x=1"),
            "b.mp3?x=1"
        );
        assert!(parse_m3u_content("# nothing\n", None).is_empty());
        assert!(normalize_audio_url("rtsp://cam.local/stream").is_ok());
        assert!(normalize_audio_url("file:///tmp/a.mp3").is_ok());
        assert!(normalize_audio_url("ftp://nope").is_err());
    }

    #[test]
    fn play_query_uses_a_one_based_index_then_title_or_uri() {
        assert_eq!(classify_play_query(None, 3), PlaySelector::Current);
        assert_eq!(classify_play_query(Some("  "), 3), PlaySelector::Current);
        assert_eq!(classify_play_query(Some("2"), 3), PlaySelector::Index(1));
        assert_eq!(
            classify_play_query(Some("0"), 3),
            PlaySelector::Text("0".into())
        );
        assert_eq!(
            classify_play_query(Some("4"), 3),
            PlaySelector::Text("4".into())
        );
        let tracks = [
            ("LoFi Radio", "https://example/lofi"),
            ("Morning", "/tmp/morning.mp3"),
        ];
        assert_eq!(find_track_index(&tracks, "LOFI"), Some(0));
        assert_eq!(find_track_index(&tracks, "morning.mp3"), Some(1));
        assert_eq!(find_track_index(&tracks, "missing"), None);
    }
}
