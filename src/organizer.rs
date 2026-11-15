use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MovePlan {
    pub file_name: String,
    pub category: String,
    pub from: PathBuf,
    pub to: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoBatch {
    pub moves: Vec<(PathBuf, PathBuf)>,
    pub created_dirs: Vec<PathBuf>,
}

#[derive(Debug, Default)]
pub struct FileOrganizer {
    pub last_preview: Vec<MovePlan>,
    pub undo_stack: Vec<UndoBatch>,
}

impl FileOrganizer {
    /// Categorizes a filename by its extension.
    pub fn categorize(path: &Path) -> &'static str {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();

        match ext.as_str() {
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "ico" => "Imágenes",
            "pdf" | "doc" | "docx" | "txt" | "md" | "odt" | "rtf" | "xls" | "xlsx" | "csv"
            | "ppt" | "pptx" => "Documentos",
            "mp4" | "mkv" | "avi" | "mov" | "webm" => "Videos",
            "mp3" | "wav" | "ogg" | "flac" | "m4a" => "Audio",
            "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" => "Comprimidos",
            "rs" | "py" | "js" | "ts" | "html" | "css" | "json" | "toml" | "yaml" | "yml"
            | "sh" | "c" | "cpp" => "Código",
            "deb" | "appimage" | "exe" | "msi" | "dmg" => "Instaladores",
            _ => "Otros",
        }
    }

    /// Scans `target_dir` for loose files (ignoring directories and hidden files)
    /// and generates a non-destructive move plan without modifying disk.
    pub fn preview(&mut self, target_dir: &Path) -> anyhow::Result<Vec<MovePlan>> {
        if !target_dir.exists() || !target_dir.is_dir() {
            anyhow::bail!("La carpeta '{}' no existe.", target_dir.display());
        }

        let mut plans = Vec::new();
        for entry in fs::read_dir(target_dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name_os) = path.file_name() else {
                continue;
            };
            let file_name = name_os.to_string_lossy().to_string();
            if file_name.starts_with('.') {
                continue;
            }

            let category = Self::categorize(&path).to_string();
            let dest_dir = target_dir.join(&category);
            let dest_path = unique_dest_path(&dest_dir, &file_name);

            plans.push(MovePlan {
                file_name,
                category,
                from: path,
                to: dest_path,
            });
        }

        plans.sort_by(|a, b| a.category.cmp(&b.category).then(a.file_name.cmp(&b.file_name)));
        self.last_preview = plans.clone();
        Ok(plans)
    }

    /// Applies the given move plans (or `last_preview` if empty) and records an UndoBatch.
    pub fn apply(&mut self, target_dir: &Path) -> anyhow::Result<usize> {
        if self.last_preview.is_empty() {
            self.preview(target_dir)?;
        }
        if self.last_preview.is_empty() {
            return Ok(0);
        }

        let mut batch = UndoBatch {
            moves: Vec::new(),
            created_dirs: Vec::new(),
        };

        for plan in &self.last_preview {
            if !plan.from.exists() {
                continue;
            }
            if let Some(parent) = plan.to.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent)?;
                    batch.created_dirs.push(parent.to_path_buf());
                }
            }
            let final_dest = unique_dest_path(
                plan.to.parent().unwrap_or(target_dir),
                &plan.file_name,
            );
            fs::rename(&plan.from, &final_dest)?;
            batch.moves.push((plan.from.clone(), final_dest));
        }

        let count = batch.moves.len();
        if count > 0 {
            self.undo_stack.push(batch);
        }
        self.last_preview.clear();
        Ok(count)
    }

    /// Reverts the last applied batch of file moves.
    pub fn undo_last(&mut self) -> anyhow::Result<usize> {
        let Some(batch) = self.undo_stack.pop() else {
            anyhow::bail!("No hay cambios recientes para deshacer.");
        };

        let mut restored = 0;
        for (orig, moved) in batch.moves.into_iter().rev() {
            if moved.exists() && !orig.exists() {
                fs::rename(&moved, &orig)?;
                restored += 1;
            }
        }

        for dir in batch.created_dirs {
            if dir.is_dir() {
                let _ = fs::remove_dir(&dir); // Only removes if empty
            }
        }
        Ok(restored)
    }
}

fn unique_dest_path(dest_dir: &Path, file_name: &str) -> PathBuf {
    let candidate = dest_dir.join(file_name);
    if !candidate.exists() {
        return candidate;
    }

    let p = Path::new(file_name);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or(file_name);
    let ext = p.extension().and_then(|e| e.to_str());

    for i in 1..=999 {
        let new_name = match ext {
            Some(e) => format!("{stem}_{i}.{e}"),
            None => format!("{stem}_{i}"),
        };
        let next = dest_dir.join(new_name);
        if !next.exists() {
            return next;
        }
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn preview_apply_and_undo() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("rusty_desk_org_test_{nanos}"));
        fs::create_dir_all(&temp_dir).unwrap();

        let img = temp_dir.join("foto.png");
        let doc = temp_dir.join("notas.pdf");
        fs::write(&img, b"png").unwrap();
        fs::write(&doc, b"pdf").unwrap();

        let mut org = FileOrganizer::default();
        let plans = org.preview(&temp_dir).unwrap();
        assert_eq!(plans.len(), 2);

        let moved = org.apply(&temp_dir).unwrap();
        assert_eq!(moved, 2);
        assert!(!img.exists());
        assert!(temp_dir.join("Imágenes").join("foto.png").exists());
        assert!(temp_dir.join("Documentos").join("notas.pdf").exists());

        let restored = org.undo_last().unwrap();
        assert_eq!(restored, 2);
        assert!(img.exists());
        assert!(doc.exists());

        let _ = fs::remove_dir_all(&temp_dir);
    }
}

