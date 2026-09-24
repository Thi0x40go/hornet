use crate::models::NoteItem;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

pub fn default_notes_dir() -> PathBuf {
    dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("hornet")
        .join("notes")
}

pub fn legacy_notes_dir() -> PathBuf {
    dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("nvim")
        .join("dbee")
        .join("notes")
}

pub fn list_notes() -> Result<Vec<NoteItem>, String> {
    let mut seen = HashSet::new();
    let mut results = Vec::new();

    let primary_dir = default_notes_dir().join("global");
    let _ = fs::create_dir_all(&primary_dir);

    // 1. Read Hornet notes
    if let Ok(entries) = fs::read_dir(&primary_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().map_or(false, |ext| ext == "sql") {
                if let Some(file_name) = path.file_stem().and_then(|s| s.to_str()) {
                    seen.insert(file_name.to_string());
                    results.push(NoteItem {
                        id: file_name.to_string(),
                        name: file_name.to_string(),
                        file_path: path.to_string_lossy().to_string(),
                        namespace: "global".to_string(),
                    });
                }
            }
        }
    }

    // 2. Read legacy nvim-dbee notes
    let legacy_dir = legacy_notes_dir().join("global");
    if let Ok(entries) = fs::read_dir(&legacy_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().map_or(false, |ext| ext == "sql") {
                if let Some(file_name) = path.file_stem().and_then(|s| s.to_str()) {
                    if seen.insert(file_name.to_string()) {
                        results.push(NoteItem {
                            id: file_name.to_string(),
                            name: file_name.to_string(),
                            file_path: path.to_string_lossy().to_string(),
                            namespace: "global".to_string(),
                        });
                    }
                }
            }
        }
    }

    Ok(results)
}

pub fn read_note(path: &str) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("Failed to read note at {}: {}", path, e))
}

pub fn save_note(name: &str, content: &str) -> Result<String, String> {
    let mut file_name = name.to_string();
    if !file_name.ends_with(".sql") {
        file_name.push_str(".sql");
    }

    let dir = default_notes_dir().join("global");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let file_path = dir.join(&file_name);
    fs::write(&file_path, content).map_err(|e| e.to_string())?;

    Ok(file_path.to_string_lossy().to_string())
}
