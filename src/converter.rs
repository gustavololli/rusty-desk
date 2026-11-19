use markitdown::model::ConversionOptions;
use markitdown::MarkItDown;
use std::path::{Path, PathBuf};
#[cfg(not(windows))]
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionMode {
    None,
    Moderate,
    LineByLine,
    Extreme,
}

impl CompactionMode {
    pub fn from_str_opt(s: Option<&str>) -> Self {
        match s.unwrap_or("").trim().to_lowercase().as_str() {
            "none" | "normal" | "ninguna" => Self::None,
            "line" | "linea" | "lines" => Self::LineByLine,
            "extreme" | "extrema" | "max" => Self::Extreme,
            // "moderate", "moderada", empty, and unknown values share this default.
            _ => Self::Moderate,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Normal",
            Self::Moderate => "Moderada (Markdown)",
            Self::LineByLine => "Línea por línea",
            Self::Extreme => "Extrema (1 línea)",
        }
    }
}

/// Normalizes and compacts Markdown/text without external dependencies to reduce LLM token usage.
pub fn compact_for_llm(input: &str, mode: CompactionMode) -> String {
    match mode {
        CompactionMode::None => input.to_string(),
        CompactionMode::Extreme => input.split_whitespace().collect::<Vec<_>>().join(" "),
        CompactionMode::LineByLine => input
            .lines()
            .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        CompactionMode::Moderate => {
            let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
            let mut out = String::with_capacity(normalized.len());
            let mut consecutive_empty = 0usize;

            for raw_line in normalized.lines() {
                let collapsed: String = raw_line
                    .split([' ', '\t'])
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");

                if collapsed.is_empty() {
                    consecutive_empty += 1;
                    // Allow at most 1 blank line between blocks (= "\n\n")
                    if consecutive_empty == 1 && !out.is_empty() {
                        out.push('\n');
                    }
                } else {
                    consecutive_empty = 0;
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(&collapsed);
                }
            }

            out.trim().to_string()
        }
    }
}

/// Formats character count and percentage savings between original and compacted text.
pub fn compaction_stats(original: &str, compacted: &str) -> (usize, usize, f32) {
    let orig_len = original.chars().count();
    let comp_len = compacted.chars().count();
    let saved_pct = if orig_len == 0 {
        0.0
    } else {
        ((orig_len.saturating_sub(comp_len)) as f32 / orig_len as f32) * 100.0
    };
    (orig_len, comp_len, saved_pct)
}

/// Converts a local file to Markdown using `markitdown-rs` without any LLM inference.
pub fn convert_file_to_markdown(path: &Path) -> Result<String, String> {
    if !path.exists() {
        return Err(format!("El archivo no existe: {}", path.display()));
    }
    if !path.is_file() {
        return Err(format!(
            "La ruta no es un archivo válido: {}",
            path.display()
        ));
    }

    let path_str = path
        .to_str()
        .ok_or_else(|| "La ruta contiene caracteres no válidos (UTF-8).".to_string())?;

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e.to_lowercase()));

    let md = MarkItDown::new();
    let options = ConversionOptions {
        file_extension: ext.clone(),
        url: None,
        llm_client: None,
        llm_model: None,
    };

    match md.convert(path_str, Some(options)) {
        Ok(Some(result)) if !result.text_content.trim().is_empty() => Ok(result.text_content),
        Ok(_) | Err(_) => {
            if ext.as_deref() == Some(".pdf") {
                return extract_pdf_with_pdftotext(path);
            }

            let ext_plain = ext.as_deref().unwrap_or("").trim_start_matches('.');
            let plain_exts = [
                "txt", "md", "markdown", "json", "toml", "yaml", "yml", "log", "rs", "py", "js",
                "ts", "sh", "ini", "cfg",
            ];
            if plain_exts.contains(&ext_plain) {
                std::fs::read_to_string(path)
                    .map_err(|e| format!("No se pudo leer el archivo como texto: {e}"))
            } else {
                Err(format!(
                    "No se pudo convertir '{}' (formato no soportado o archivo dañado).",
                    path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("archivo")
                ))
            }
        }
    }
}

fn extract_pdf_with_pdftotext(path: &Path) -> Result<String, String> {
    #[cfg(windows)]
    {
        let _ = path;
        return Err("No se pudo extraer el PDF con MarkItDown. El fallback 'pdftotext' todavía no está disponible en Windows.".to_string());
    }
    #[cfg(not(windows))]
    {
        let output = Command::new("pdftotext")
        .arg("-layout")
        .arg(path)
        .arg("-")
        .output()
        .map_err(|e| {
            format!(
                "MarkItDown (pdf-extract) no pudo leer este PDF y no se pudo ejecutar 'pdftotext': {e}"
            )
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "Error al extraer texto del PDF con pdftotext: {stderr}"
            ));
        }

        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() {
            return Err(
                "El PDF no contiene texto extraíble (posiblemente sea una imagen escaneada)."
                    .to_string(),
            );
        }

        let title = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Documento PDF");
        Ok(format!("# {title}\n\n{text}\n"))
    }
}

/// Saves converted Markdown next to the source file as `<stem>.md` (or `<stem>.converted.md` if source is already `.md`).
pub fn save_markdown_beside(source_path: &Path, markdown: &str) -> Result<PathBuf, String> {
    let parent = source_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let stem = source_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("documento");
    let is_md = source_path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"));

    let out_name = if is_md {
        format!("{stem}.converted.md")
    } else {
        format!("{stem}.md")
    };
    let out_path = parent.join(out_name);
    std::fs::write(&out_path, markdown)
        .map_err(|e| format!("No se pudo guardar '{}': {e}", out_path.display()))?;
    Ok(out_path)
}

/// Opens a native Linux file picker (`zenity` or `kdialog`) and returns selected file paths.
pub fn pick_files_dialog() -> Result<Vec<PathBuf>, String> {
    #[cfg(windows)]
    {
        return Ok(rfd::FileDialog::new()
            .set_title("Seleccionar archivos para Rusty")
            .pick_files()
            .unwrap_or_default());
    }

    #[cfg(not(windows))]
    {
        if let Ok(output) = Command::new("zenity")
            .args([
                "--file-selection",
                "--multiple",
                "--separator=\n",
                "--title=Seleccionar archivo para MarkItDown",
            ])
            .output()
        {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let paths: Vec<PathBuf> = stdout
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(PathBuf::from)
                    .collect();
                return Ok(paths);
            }
            // User cancelled dialog
            return Ok(Vec::new());
        }

        if let Ok(output) = Command::new("kdialog")
            .args(["--getopenfilename", ".", "--multiple", "--separate-output"])
            .output()
        {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let paths: Vec<PathBuf> = stdout
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(PathBuf::from)
                    .collect();
                return Ok(paths);
            }
            return Ok(Vec::new());
        }

        Err("No se encontró 'zenity' ni 'kdialog' para abrir el selector de archivos. Podés arrastrar y soltar el archivo directamente.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_modes_preserve_and_reduce_as_expected() {
        let sample = "  #   Título  \n\n\n\n-   Item   1\t\t\n\n  - Item   2  \n";
        let moderate = compact_for_llm(sample, CompactionMode::Moderate);
        assert_eq!(moderate, "# Título\n\n- Item 1\n\n- Item 2");

        let line_by_line = compact_for_llm(sample, CompactionMode::LineByLine);
        assert_eq!(line_by_line, "# Título\n- Item 1\n- Item 2");

        let extreme = compact_for_llm(sample, CompactionMode::Extreme);
        assert_eq!(extreme, "# Título - Item 1 - Item 2");
    }

    #[test]
    fn compaction_aliases_crlf_unicode_stats_and_missing_files() {
        assert_eq!(
            CompactionMode::from_str_opt(Some("NINGUNA")),
            CompactionMode::None
        );
        assert_eq!(
            CompactionMode::from_str_opt(Some("linea")),
            CompactionMode::LineByLine
        );
        assert_eq!(
            CompactionMode::from_str_opt(Some("MAX")),
            CompactionMode::Extreme
        );
        assert_eq!(CompactionMode::from_str_opt(None), CompactionMode::Moderate);
        assert_eq!(
            CompactionMode::from_str_opt(Some("no-such")).label(),
            "Moderada (Markdown)"
        );
        assert_eq!(compact_for_llm("hola", CompactionMode::None), "hola");
        assert_eq!(
            compact_for_llm("a  \r\nb\r\n\r\n\r\nc", CompactionMode::Moderate),
            "a\nb\n\nc"
        );

        let (orig, compact, saved) = compaction_stats("áááá", "á");
        assert_eq!((orig, compact), (4, 1));
        assert!((saved - 75.0).abs() < 0.01);
        assert_eq!(compaction_stats("", "").2, 0.0);

        let missing = convert_file_to_markdown(Path::new("/tmp/rusty-desk-missing-file.md"));
        assert!(missing.unwrap_err().contains("no existe"));
        assert!(convert_file_to_markdown(Path::new("."))
            .unwrap_err()
            .contains("no es un archivo"));
    }
}
