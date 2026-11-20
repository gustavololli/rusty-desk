use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::io::Cursor;
use std::sync::{Arc, Mutex};

/// Holds an active microphone recording session.
pub struct ActiveRecording {
    samples: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    channels: u16,
    _stream: cpal::Stream,
}

impl ActiveRecording {
    /// Starts capturing audio from the default input device.
    pub fn start() -> anyhow::Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| anyhow::anyhow!("No se encontró ningún micrófono predeterminado."))?;

        let config = device.default_input_config()?;
        let sample_rate = config.sample_rate().0;
        let channels = config.channels();
        let samples = Arc::new(Mutex::new(Vec::<f32>::new()));
        let samples_clone = Arc::clone(&samples);

        let err_fn = |err| eprintln!("Error en stream de audio: {err}");

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &config.into(),
                move |data: &[f32], _| {
                    if let Ok(mut buf) = samples_clone.lock() {
                        buf.extend_from_slice(data);
                    }
                },
                err_fn,
                None,
            )?,
            cpal::SampleFormat::I16 => device.build_input_stream(
                &config.into(),
                move |data: &[i16], _| {
                    if let Ok(mut buf) = samples_clone.lock() {
                        buf.extend(data.iter().map(|&s| s as f32 / i16::MAX as f32));
                    }
                },
                err_fn,
                None,
            )?,
            cpal::SampleFormat::U16 => device.build_input_stream(
                &config.into(),
                move |data: &[u16], _| {
                    if let Ok(mut buf) = samples_clone.lock() {
                        buf.extend(data.iter().map(|&s| {
                            (s as f32 - u16::MAX as f32 / 2.0) / (u16::MAX as f32 / 2.0)
                        }));
                    }
                },
                err_fn,
                None,
            )?,
            fmt => anyhow::bail!("Formato de audio no soportado: {fmt:?}"),
        };

        stream.play()?;

        Ok(Self {
            samples,
            sample_rate,
            channels,
            _stream: stream,
        })
    }

    /// Stops recording and encodes captured audio into WAV bytes (16-bit PCM).
    pub fn stop_to_wav(self) -> anyhow::Result<Vec<u8>> {
        drop(self._stream);

        let samples = self
            .samples
            .lock()
            .map_err(|_| anyhow::anyhow!("Error al leer el buffer de audio"))?;

        if samples.is_empty() {
            anyhow::bail!("No se capturó audio del micrófono.");
        }

        let spec = hound::WavSpec {
            channels: self.channels,
            sample_rate: self.sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };

        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = hound::WavWriter::new(&mut cursor, spec)?;
            for &s in samples.iter() {
                let clamped = s.clamp(-1.0, 1.0);
                writer.write_sample((clamped * i16::MAX as f32) as i16)?;
            }
            writer.finalize()?;
        }

        Ok(cursor.into_inner())
    }
}

/// Best-effort local TTS playback using system speech synthesizer if available.
pub fn speak_local_async(text: String) {
    std::thread::spawn(move || {
        #[cfg(windows)]
        {
            use std::io::Write;
            use std::process::{Command, Stdio};
            // The spoken text goes through stdin, never interpolated into PowerShell code.
            let script = "Add-Type -AssemblyName System.Speech; $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; $s.Speak([Console]::In.ReadToEnd())";
            if let Ok(mut child) = Command::new("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-Command", script])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                if let Some(mut input) = child.stdin.take() {
                    let _ = input.write_all(text.as_bytes());
                }
                let _ = child.wait();
            }
        }
        #[cfg(not(windows))]
        {
            for bin in ["espeak-ng", "espeak", "spd-say"] {
                let args: Vec<&str> = if bin == "spd-say" {
                    vec![&text]
                } else {
                    vec!["-v", "es", &text]
                };
                if let Ok(mut child) = std::process::Command::new(bin).args(&args).spawn() {
                    let _ = child.wait();
                    break;
                }
            }
        }
    });
}
