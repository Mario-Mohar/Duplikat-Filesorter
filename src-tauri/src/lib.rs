use serde::Serialize;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{Emitter, Window};

// Global stop flag
static STOP_FLAG: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Serialize)]
struct ProgressPayload {
    status: Option<String>,
    progress: Option<f64>,
    files_scanned: Option<usize>,
    duplicates_found: Option<usize>,
    space_saved: Option<u64>,
}

#[derive(Clone, Serialize)]
struct LogPayload {
    message: String,
    level: String,
}

#[derive(Serialize)]
struct SearchResult {
    duplicates_found: usize,
    space_saved: u64,
    log_file: Option<String>,
}

fn emit_progress(window: &Window, payload: ProgressPayload) {
    let _ = window.emit("scan-progress", payload);
}

fn emit_log(window: &Window, message: &str, level: &str) {
    let _ = window.emit(
        "log-message",
        LogPayload {
            message: message.to_string(),
            level: level.to_string(),
        },
    );
}

fn calculate_md5(path: &Path) -> Option<String> {
    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let mut context = md5::Context::new();
    let mut buffer = [0u8; 8192];

    loop {
        let bytes_read = reader.read(&mut buffer).ok()?;
        if bytes_read == 0 {
            break;
        }
        context.consume(&buffer[..bytes_read]);
    }

    Some(format!("{:x}", context.compute()))
}

fn get_file_mtime(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs())
        .unwrap_or(0)
}

#[tauri::command]
async fn find_duplicates(
    window: Window,
    source_dir: String,
    duplicates_dir: Option<String>,
    dry_run: bool,
) -> Result<SearchResult, String> {
    STOP_FLAG.store(false, Ordering::SeqCst);

    let source_path = PathBuf::from(&source_dir);
    if !source_path.is_dir() {
        return Err(format!("Ordner nicht gefunden: {}", source_dir));
    }

    // Determine duplicates directory
    let dup_path = match duplicates_dir {
        Some(ref dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => source_path.join("Duplikate"),
    };

    // Create duplicates directory if needed
    if !dry_run && !dup_path.exists() {
        fs::create_dir_all(&dup_path)
            .map_err(|e| format!("Konnte Duplikate-Ordner nicht erstellen: {}", e))?;
        emit_log(&window, &format!("Duplikate-Ordner erstellt: {}", dup_path.display()), "success");
    } else if dup_path.exists() {
        emit_log(&window, &format!("Duplikate-Ordner: {}", dup_path.display()), "info");
    }

    // Step 1: Scan files and group by size
    emit_progress(&window, ProgressPayload {
        status: Some("Scanne Dateien...".to_string()),
        progress: Some(0.0),
        files_scanned: None,
        duplicates_found: None,
        space_saved: None,
    });

    let mut size_groups: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    let mut file_count = 0;

    fn scan_directory(
        dir: &Path,
        dup_path: &Path,
        size_groups: &mut HashMap<u64, Vec<PathBuf>>,
        file_count: &mut usize,
    ) -> Result<(), String> {
        let entries = fs::read_dir(dir).map_err(|e| e.to_string())?;

        for entry in entries.flatten() {
            if STOP_FLAG.load(Ordering::SeqCst) {
                return Err("Abgebrochen".to_string());
            }

            let path = entry.path();

            // Skip duplicates directory
            if path.starts_with(dup_path) {
                continue;
            }

            if path.is_dir() {
                let _ = scan_directory(&path, dup_path, size_groups, file_count);
            } else if path.is_file() {
                if let Ok(metadata) = fs::metadata(&path) {
                    let size = metadata.len();
                    if size > 0 {
                        size_groups.entry(size).or_default().push(path);
                        *file_count += 1;
                    }
                }
            }
        }
        Ok(())
    }

    scan_directory(&source_path, &dup_path, &mut size_groups, &mut file_count)?;

    emit_log(&window, &format!("Gefunden: {} Dateien", file_count), "info");
    emit_progress(&window, ProgressPayload {
        status: None,
        progress: None,
        files_scanned: Some(file_count),
        duplicates_found: None,
        space_saved: None,
    });

    // Step 2: Calculate hashes for potential duplicates
    let files_to_hash: Vec<PathBuf> = size_groups
        .values()
        .filter(|files| files.len() > 1)
        .flatten()
        .cloned()
        .collect();

    let total_to_hash = files_to_hash.len();
    emit_log(&window, &format!("Berechne Hashes für {} potentielle Duplikate...", total_to_hash), "info");

    let mut hash_groups: HashMap<String, Vec<PathBuf>> = HashMap::new();

    for (i, path) in files_to_hash.iter().enumerate() {
        if STOP_FLAG.load(Ordering::SeqCst) {
            return Err("Abgebrochen".to_string());
        }

        if let Some(hash) = calculate_md5(path) {
            hash_groups.entry(hash).or_default().push(path.clone());
        }

        let progress = ((i + 1) as f64 / total_to_hash as f64) * 50.0;
        if i % 50 == 0 {
            emit_progress(&window, ProgressPayload {
                status: Some(format!("Hash: {}/{}", i + 1, total_to_hash)),
                progress: Some(progress),
                files_scanned: None,
                duplicates_found: None,
                space_saved: None,
            });
        }
    }

    // Step 3: Filter to only actual duplicates
    let duplicates: HashMap<String, Vec<PathBuf>> = hash_groups
        .into_iter()
        .filter(|(_, files)| files.len() > 1)
        .collect();

    if duplicates.is_empty() {
        emit_log(&window, "Keine Duplikate gefunden!", "success");
        return Ok(SearchResult {
            duplicates_found: 0,
            space_saved: 0,
            log_file: None,
        });
    }

    let total_duplicates: usize = duplicates.values().map(|files| files.len() - 1).sum();
    emit_log(&window, &format!("Gefunden: {} Gruppen mit {} Duplikaten", duplicates.len(), total_duplicates), "highlight");

    // Step 4: Create log file and move duplicates
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let log_file_path = source_path.join(format!("duplikate_log_{}.txt", timestamp));

    let mut log_file = File::create(&log_file_path)
        .map_err(|e| format!("Konnte Logdatei nicht erstellen: {}", e))?;

    writeln!(log_file, "Duplikat-Finder Log - {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")).ok();
    writeln!(log_file, "{}", "=".repeat(80)).ok();
    writeln!(log_file).ok();

    if dry_run {
        writeln!(log_file, "*** TROCKENLAUF - Keine Dateien wurden verschoben ***").ok();
        writeln!(log_file).ok();
    }

    // Drei getrennte Zaehler statt einem: ein einzelner moved_count musste
    // gleichzeitig "wuerde verschieben", "verschoben" und "gefunden" bedeuten
    // und war dadurch in zwei von drei Faellen falsch.
    let mut moved_count = 0;    // nur erfolgreiche echte Verschiebungen
    let mut planned_count = 0;  // Kandidaten im Trockenlauf
    let mut failed_count = 0;   // fehlgeschlagene Verschiebungen
    let mut space_saved: u64 = 0;
    let total_groups = duplicates.len();

    for (idx, (hash, mut files)) in duplicates.into_iter().enumerate() {
        if STOP_FLAG.load(Ordering::SeqCst) {
            emit_log(&window, "Abgebrochen!", "warning");
            break;
        }

        // Sort by modification time (keep oldest)
        files.sort_by_key(|f| get_file_mtime(f));

        let original = &files[0];
        let dups_to_move = &files[1..];

        writeln!(log_file, "Hash: {}", hash).ok();
        writeln!(log_file, "  Original: {}", original.display()).ok();

        emit_log(&window, &format!("Gruppe {}/{}: {}", idx + 1, total_groups,
            original.file_name().unwrap_or_default().to_string_lossy()), "info");

        for dup_file in dups_to_move {
            let file_size = fs::metadata(dup_file).map(|m| m.len()).unwrap_or(0);

            // Create destination path
            let rel_path = dup_file.strip_prefix(&source_path).unwrap_or(dup_file);
            let mut dest_path = dup_path.join(rel_path);

            // Handle name conflicts
            if dest_path.exists() {
                let stem = dest_path.file_stem().unwrap_or_default().to_string_lossy().to_string();
                let ext = dest_path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
                let parent = dest_path.parent().unwrap_or(&dup_path).to_path_buf();

                let mut counter = 1;
                loop {
                    dest_path = parent.join(format!("{}_{}{}", stem, counter, ext));
                    if !dest_path.exists() {
                        break;
                    }
                    counter += 1;
                }
            }

            if dry_run {
                writeln!(log_file, "  [WÜRDE VERSCHIEBEN] {}", dup_file.display()).ok();
                emit_log(&window, &format!("  -> Würde verschieben: {}",
                    dup_file.file_name().unwrap_or_default().to_string_lossy()), "warning");
                planned_count += 1;
                space_saved += file_size;
            } else {
                // Create parent directory
                if let Some(parent) = dest_path.parent() {
                    let _ = fs::create_dir_all(parent);
                }

                match fs::rename(dup_file, &dest_path) {
                    Ok(_) => {
                        writeln!(log_file, "  Verschoben: {}", dup_file.display()).ok();
                        writeln!(log_file, "    -> {}", dest_path.display()).ok();
                        emit_log(&window, &format!("  -> Verschoben: {}",
                            dup_file.file_name().unwrap_or_default().to_string_lossy()), "success");
                        moved_count += 1;
                        space_saved += file_size;
                    }
                    Err(e) => {
                        writeln!(log_file, "  FEHLER: {} - {}", dup_file.display(), e).ok();
                        emit_log(&window, &format!("  FEHLER: {}", e), "error");
                        failed_count += 1;
                        continue;
                    }
                }
            }
        }

        writeln!(log_file).ok();

        let progress = 50.0 + ((idx + 1) as f64 / total_groups as f64) * 50.0;
        emit_progress(&window, ProgressPayload {
            status: None,
            progress: Some(progress),
            files_scanned: None,
            duplicates_found: Some(moved_count + planned_count),
            space_saved: Some(space_saved),
        });
    }

    // Write summary
    writeln!(log_file, "{}", "=".repeat(80)).ok();
    writeln!(log_file, "ZUSAMMENFASSUNG").ok();
    writeln!(log_file, "Duplikat-Gruppen: {}", total_groups).ok();
    if dry_run {
        writeln!(log_file, "Würde verschieben: {}", planned_count).ok();
    } else {
        writeln!(log_file, "Dateien verschoben: {}", moved_count).ok();
        if failed_count > 0 {
            writeln!(log_file, "Fehlgeschlagen: {}", failed_count).ok();
        }
    }
    writeln!(log_file, "Speicherplatz: {:.2} MB", space_saved as f64 / (1024.0 * 1024.0)).ok();

    Ok(SearchResult {
        // Was gefunden wurde, nicht was bewegt wurde -- die Oberflaeche
        // beschriftet das Feld mit "Duplikate gefunden".
        duplicates_found: total_duplicates,
        space_saved,
        log_file: Some(log_file_path.to_string_lossy().to_string()),
    })
}

#[tauri::command]
fn stop_search() {
    STOP_FLAG.store(true, Ordering::SeqCst);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![find_duplicates, stop_search])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
