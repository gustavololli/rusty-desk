# Windows platform support (experimental branch)

Branch: `feature/rusty-windows-platform` — based on Dreaming v1 merged into `main`.

Rusty chooses native integrations at compile time (`cfg(windows)` / `cfg(target_os = "linux")`),
not by guessing from environment variables. The OS is shown in **Settings → AI and Audio**.

| Feature | Linux | Windows |
| --- | --- | --- |
| File selection / chat attachments | zenity / kdialog (existing) | Native dialog via `rfd` |
| M3U playlist selection | zenity / kdialog | Native dialog via `rfd` |
| Music volume / mute | PulseAudio/PipeWire via `pactl` | Windows Core Audio session volume per ffplay PID |
| Cursor tracking (pet eyes) | X11 | Win32 `GetCursorPos` |
| Sound effects | pw-play/paplay/aplay/ffplay | Win32 `PlaySoundW` |
| Local text-to-speech | espeak / spd-say | Windows System.Speech via PowerShell |
| KDE always-on-top workaround | Wayland / KWin only | Not invoked; uses eframe window level |

## Music caveats

Audio playback itself still relies on an installed `ffplay` executable (or existing Linux
fallbacks). On Windows install FFmpeg/ffplay and ensure it is on PATH. The Windows Core Audio
backend adjusts **only the spawned player's** audio session, not system/master volume.

Pause/resume previously invoked `kill -STOP/-CONT` (Linux-specific). Those commands are now
excluded on Windows; **Windows pause/resume is not implemented in this first platform pass**.
The UI should not imply playback has paused when audio continues.

## Validation needed on Windows

1. `cargo check` and `cargo test` with Windows stable Rust.
2. `cargo run --release`; open chat file picker and M3U picker, including cancellation.
3. Start an online radio with ffplay; change volume and mute, check only Rusty's session changes.
4. Play notification SFX and test text-to-speech.
5. Check pet eye tracking and window always-on-top.

No deployment or changes to Nueva Farma projects.
