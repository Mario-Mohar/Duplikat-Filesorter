use rayon::prelude::*;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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

/// Ob ein Fehler bedeutet: "das ist eine andere Platte".
///
/// EXDEV ist 18 unter Linux und macOS; Windows meldet fuer dieselbe Lage
/// ERROR_NOT_SAME_DEVICE (17). Nur dieser eine Fall darf auf Kopieren
/// ausweichen -- eine fehlende Berechtigung soll weiterhin als Fehler
/// dastehen und nicht als zweiter, ebenso aussichtsloser Versuch.
fn is_cross_device(err: &std::io::Error) -> bool {
    match err.raw_os_error() {
        Some(18) => cfg!(unix),
        Some(17) => cfg!(windows),
        _ => false,
    }
}

/// Kopiert in Bloecken und prueft dabei das Abbruch-Flag.
///
/// `fs::copy` waere kuerzer, aber bei einer 4-GB-Datei laege die letzte
/// Abbruchpruefung dann Minuten zurueck. Und bricht die Kopie ab -- durch
/// einen Fehler oder durch den Nutzer --, muss die halbe Zieldatei weg: sonst
/// steht im Duplikate-Ordner eine beschaedigte Datei, die aussieht wie ein
/// gerettetes Duplikat.
fn copy_with_stop(src: &Path, dest: &Path) -> Result<(), String> {
    let result = (|| -> std::io::Result<()> {
        let mut reader = BufReader::new(File::open(src)?);
        let mut writer = BufWriter::new(File::create(dest)?);
        let mut buffer = vec![0u8; 65536];

        loop {
            if STOP_FLAG.load(Ordering::SeqCst) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "Abgebrochen",
                ));
            }
            let bytes_read = reader.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            writer.write_all(&buffer[..bytes_read])?;
        }
        writer.flush()?;

        // Das Aenderungsdatum traegt hier Bedeutung: behalten wird die
        // aelteste Datei jeder Gruppe. `fs::copy` uebernimmt den Zeitstempel
        // nicht, eine Kopie saehe also juenger aus als das Original.
        if let Ok(mtime) = fs::metadata(src).and_then(|m| m.modified()) {
            let _ = writer.get_ref().set_modified(mtime);
        }
        Ok(())
    })();

    if let Err(e) = result {
        let _ = fs::remove_file(dest);
        return Err(e.to_string());
    }
    Ok(())
}

fn scan_directory(
    dir: &Path,
    dup_path: &Path,
    size_groups: &mut HashMap<u64, Vec<PathBuf>>,
    file_count: &mut usize,
    visited: &mut HashSet<PathBuf>,
    loops_cut: &mut Vec<PathBuf>,
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
            // Ordner an ihrer Identitaet wiedererkennen, nicht am Pfad:
            // `is_dir()` folgt Symlinks, und derselbe Ordner ist ueber
            // mehrere Wege erreichbar. Ohne diese Liste schickt ein
            // Verweis auf einen Vorfahren die Rekursion in eine Schleife,
            // bis der Stack reisst -- und ein doppelt erreichbarer Ordner
            // erschiene als Duplikat seiner selbst.
            //
            // `canonicalize` loest dafuer jeden Symlink auf. Das ist
            // plattformunabhaengig, wo (dev, ino) es nicht waere, und
            // fuer Verzeichnisse genauso eindeutig -- Hardlinks auf
            // Ordner gibt es nicht.
            let key = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            if !visited.insert(key) {
                loops_cut.push(path.clone());
                continue;
            }
            let _ = scan_directory(&path, dup_path, size_groups, file_count, visited, loops_cut);
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

fn get_file_mtime(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        })
        .unwrap_or(0)
}

#[tauri::command]
async fn find_duplicates(
    window: Window,
    source_dir: String,
    duplicates_dir: Option<String>,
    dry_run: bool,
    hash_threads: Option<usize>,
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
        emit_log(
            &window,
            &format!("Duplikate-Ordner erstellt: {}", dup_path.display()),
            "success",
        );
    } else if dup_path.exists() {
        emit_log(
            &window,
            &format!("Duplikate-Ordner: {}", dup_path.display()),
            "info",
        );
    }

    // Step 1: Scan files and group by size
    emit_progress(
        &window,
        ProgressPayload {
            status: Some("Scanne Dateien...".to_string()),
            progress: Some(0.0),
            files_scanned: None,
            duplicates_found: None,
            space_saved: None,
        },
    );

    let mut size_groups: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    let mut file_count = 0;
    let mut visited: HashSet<PathBuf> = HashSet::new();
    let mut loops_cut: Vec<PathBuf> = Vec::new();

    // Der Quellordner selbst gehoert vor dem ersten Abstieg auf die Liste,
    // sonst laeuft ein Symlink, der auf ihn zurueckzeigt, eine Ebene zu weit.
    visited.insert(fs::canonicalize(&source_path).unwrap_or_else(|_| source_path.clone()));
    scan_directory(
        &source_path,
        &dup_path,
        &mut size_groups,
        &mut file_count,
        &mut visited,
        &mut loops_cut,
    )?;

    // Sichtbar melden: sonst wundert man sich still ueber fehlende Treffer.
    if !loops_cut.is_empty() {
        emit_log(
            &window,
            &format!(
                "{} Ordner uebersprungen, weil sie schon besucht waren (Symlink-Schleife)",
                loops_cut.len()
            ),
            "warning",
        );
        for path in loops_cut.iter().take(5) {
            emit_log(
                &window,
                &format!("  uebersprungen: {}", path.display()),
                "info",
            );
        }
    }

    emit_log(
        &window,
        &format!("Gefunden: {} Dateien", file_count),
        "info",
    );
    emit_progress(
        &window,
        ProgressPayload {
            status: None,
            progress: None,
            files_scanned: Some(file_count),
            duplicates_found: None,
            space_saved: None,
        },
    );

    // Step 2: Calculate hashes for potential duplicates
    let files_to_hash: Vec<PathBuf> = size_groups
        .values()
        .filter(|files| files.len() > 1)
        .flatten()
        .cloned()
        .collect();

    let total_to_hash = files_to_hash.len();
    emit_log(
        &window,
        &format!(
            "Berechne Hashes für {} potentielle Duplikate...",
            total_to_hash
        ),
        "info",
    );

    // Auf einer SSD ist paralleles Lesen deutlich schneller, auf einer
    // Festplatte langsamer -- deshalb abschaltbar (hash_threads = 1) statt
    // fest verdrahtet. Gedeckelt, weil jenseits einer Handvoll Faeden nicht
    // mehr die CPU der Engpass ist, sondern das Laufwerk.
    let threads = match hash_threads {
        Some(n) if n >= 1 => n,
        _ => std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .min(8),
    };
    if threads > 1 {
        emit_log(&window, &format!("Hashe mit {} Faeden", threads), "info");
    }

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .map_err(|e| format!("Konnte Hash-Faeden nicht starten: {}", e))?;

    // Der Fortschritt kam aus dem Schleifenindex; parallel braucht es einen
    // atomaren Zaehler, sonst springt die Anzeige hin und her.
    let hashed_count = AtomicUsize::new(0);
    let window_ref = &window;

    let mut hashed: Vec<(usize, String, PathBuf)> = pool.install(|| {
        files_to_hash
            .par_iter()
            .enumerate()
            // Vor dem Hashen pruefen reicht fuer einen zuegigen Abbruch: eine
            // einzelne Datei ist schnell durch.
            .filter(|_| !STOP_FLAG.load(Ordering::SeqCst))
            .filter_map(|(i, path)| {
                let hash = calculate_md5(path);
                let done = hashed_count.fetch_add(1, Ordering::Relaxed) + 1;
                if done.is_multiple_of(50) || done == total_to_hash {
                    emit_progress(
                        window_ref,
                        ProgressPayload {
                            status: Some(format!("Hash: {}/{}", done, total_to_hash)),
                            progress: Some((done as f64 / total_to_hash as f64) * 50.0),
                            files_scanned: None,
                            duplicates_found: None,
                            space_saved: None,
                        },
                    );
                }
                hash.map(|h| (i, h, path.clone()))
            })
            .collect()
    });

    if STOP_FLAG.load(Ordering::SeqCst) {
        return Err("Abgebrochen".to_string());
    }

    // Nach dem urspruenglichen Index sortiert, bevor gruppiert wird: welche
    // Datei behalten wird, entscheidet spaeter ohnehin die Sortierung nach
    // mtime -- aber zwei Laeufe ueber denselben Ordner sollen dasselbe Log
    // ergeben, und die Reihenfolge, in der Faeden fertig werden, ist beliebig.
    hashed.sort_by_key(|entry| entry.0);

    let mut hash_groups: HashMap<String, Vec<PathBuf>> = HashMap::new();
    for (_, hash, path) in hashed {
        hash_groups.entry(hash).or_default().push(path);
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
    emit_log(
        &window,
        &format!(
            "Gefunden: {} Gruppen mit {} Duplikaten",
            duplicates.len(),
            total_duplicates
        ),
        "highlight",
    );

    // Step 4: Create log file and move duplicates
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let log_file_path = source_path.join(format!("duplikate_log_{}.txt", timestamp));

    let mut log_file = File::create(&log_file_path)
        .map_err(|e| format!("Konnte Logdatei nicht erstellen: {}", e))?;

    writeln!(
        log_file,
        "Duplikat-Finder Log - {}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    )
    .ok();
    writeln!(log_file, "{}", "=".repeat(80)).ok();
    writeln!(log_file).ok();

    if dry_run {
        writeln!(
            log_file,
            "*** TROCKENLAUF - Keine Dateien wurden verschoben ***"
        )
        .ok();
        writeln!(log_file).ok();
    }

    // Drei getrennte Zaehler statt einem: ein einzelner moved_count musste
    // gleichzeitig "wuerde verschieben", "verschoben" und "gefunden" bedeuten
    // und war dadurch in zwei von drei Faellen falsch.
    let mut moved_count = 0; // nur erfolgreiche echte Verschiebungen
    let mut planned_count = 0; // Kandidaten im Trockenlauf
    let mut failed_count = 0; // fehlgeschlagene Verschiebungen
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

        emit_log(
            &window,
            &format!(
                "Gruppe {}/{}: {}",
                idx + 1,
                total_groups,
                original.file_name().unwrap_or_default().to_string_lossy()
            ),
            "info",
        );

        for dup_file in dups_to_move {
            // Auch innerhalb einer Gruppe pruefen: eine Kopie ueber
            // Plattengrenzen kann lange dauern, und ein Abbruch soll nicht
            // erst beim naechsten Gruppenwechsel greifen.
            if STOP_FLAG.load(Ordering::SeqCst) {
                break;
            }

            let file_size = fs::metadata(dup_file).map(|m| m.len()).unwrap_or(0);

            // Create destination path
            let rel_path = dup_file.strip_prefix(&source_path).unwrap_or(dup_file);
            let mut dest_path = dup_path.join(rel_path);

            // Handle name conflicts
            if dest_path.exists() {
                let stem = dest_path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let ext = dest_path
                    .extension()
                    .map(|e| format!(".{}", e.to_string_lossy()))
                    .unwrap_or_default();
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
                emit_log(
                    &window,
                    &format!(
                        "  -> Würde verschieben: {}",
                        dup_file.file_name().unwrap_or_default().to_string_lossy()
                    ),
                    "warning",
                );
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
                        emit_log(
                            &window,
                            &format!(
                                "  -> Verschoben: {}",
                                dup_file.file_name().unwrap_or_default().to_string_lossy()
                            ),
                            "success",
                        );
                        moved_count += 1;
                        space_saved += file_size;
                    }
                    // Der Duplikate-Ordner darf auf einer anderen Platte
                    // liegen -- gerade dort ist ueblicherweise Platz, und der
                    // Ordner ist der einzige Grund, dem Werkzeug zu trauen.
                    // `rename` kann das nicht, also kopieren und erst nach
                    // erfolgreicher Kopie loeschen. Nie andersherum.
                    Err(e) if is_cross_device(&e) => {
                        match copy_with_stop(dup_file, &dest_path) {
                            Ok(_) => match fs::remove_file(dup_file) {
                                Ok(_) => {
                                    writeln!(
                                        log_file,
                                        "  Kopiert und entfernt (andere Platte): {}",
                                        dup_file.display()
                                    )
                                    .ok();
                                    writeln!(log_file, "    -> {}", dest_path.display()).ok();
                                    emit_log(
                                        &window,
                                        &format!(
                                            "  -> Kopiert (andere Platte): {}",
                                            dup_file
                                                .file_name()
                                                .unwrap_or_default()
                                                .to_string_lossy()
                                        ),
                                        "success",
                                    );
                                    moved_count += 1;
                                    space_saved += file_size;
                                }
                                Err(e) => {
                                    // Die Kopie steht, das Original liegt noch
                                    // da: die Kopie zuruecknehmen, sonst
                                    // existiert die Datei doppelt und der Lauf
                                    // haette still nichts gespart.
                                    let _ = fs::remove_file(&dest_path);
                                    writeln!(
                                        log_file,
                                        "  FEHLER: {} - kopiert, aber nicht loeschbar: {}",
                                        dup_file.display(),
                                        e
                                    )
                                    .ok();
                                    emit_log(&window, &format!("  FEHLER: {}", e), "error");
                                    failed_count += 1;
                                    continue;
                                }
                            },
                            Err(msg) => {
                                writeln!(log_file, "  FEHLER: {} - {}", dup_file.display(), msg)
                                    .ok();
                                emit_log(&window, &format!("  FEHLER: {}", msg), "error");
                                failed_count += 1;
                                continue;
                            }
                        }
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
        emit_progress(
            &window,
            ProgressPayload {
                status: None,
                progress: Some(progress),
                files_scanned: None,
                duplicates_found: Some(moved_count + planned_count),
                space_saved: Some(space_saved),
            },
        );
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
    writeln!(
        log_file,
        "Speicherplatz: {:.2} MB",
        space_saved as f64 / (1024.0 * 1024.0)
    )
    .ok();

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    /// Ein eigenes Verzeichnis je Test, ohne Zusatzabhaengigkeit.
    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("dupfinder-test-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("Testordner");
        dir
    }

    fn scan(root: &Path) -> (usize, usize) {
        let mut groups: HashMap<u64, Vec<PathBuf>> = HashMap::new();
        let mut count = 0;
        let mut visited: HashSet<PathBuf> = HashSet::new();
        let mut cut: Vec<PathBuf> = Vec::new();
        let nowhere = root.join("__keine_duplikate__");
        visited.insert(fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf()));
        scan_directory(
            root,
            &nowhere,
            &mut groups,
            &mut count,
            &mut visited,
            &mut cut,
        )
        .expect("Scan");
        (count, cut.len())
    }

    #[test]
    #[cfg(unix)]
    fn symlink_auf_den_vorfahren_beendet_den_scan() {
        use std::os::unix::fs::symlink;
        let root = scratch("loop");
        fs::create_dir(root.join("unten")).unwrap();
        fs::write(root.join("unten/a.txt"), b"hallo").unwrap();
        // Der klassische Fall: ein Ordner, der auf seinen eigenen Vorfahren zeigt.
        symlink(&root, root.join("unten/zurueck")).unwrap();

        let (files, cut) = scan(&root);
        assert_eq!(files, 1, "die eine echte Datei, nicht mehrfach");
        assert!(
            cut >= 1,
            "die Schleife muss als abgeschnitten gemeldet werden"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    #[cfg(unix)]
    fn zweiter_weg_auf_denselben_ordner_zaehlt_nicht_doppelt() {
        use std::os::unix::fs::symlink;
        let root = scratch("zweiwege");
        fs::create_dir(root.join("echt")).unwrap();
        fs::write(root.join("echt/a.txt"), b"inhalt").unwrap();
        symlink(root.join("echt"), root.join("auchecht")).unwrap();

        let (files, cut) = scan(&root);
        // Ohne Merkliste stuende die Datei zweimal drin und waere ihr eigenes Duplikat.
        assert_eq!(files, 1);
        assert_eq!(cut, 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn kopie_uebernimmt_inhalt_und_zeitstempel() {
        let root = scratch("kopie");
        let src = root.join("quelle.bin");
        let dest = root.join("ziel.bin");
        // Groesser als der 64-KB-Puffer, damit die Schleife mehrfach laeuft.
        let payload: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        fs::write(&src, &payload).unwrap();

        let alt = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        File::options()
            .write(true)
            .open(&src)
            .unwrap()
            .set_modified(alt)
            .unwrap();

        copy_with_stop(&src, &dest).expect("Kopie");
        assert_eq!(fs::read(&dest).unwrap(), payload);

        let src_m = fs::metadata(&src).unwrap().modified().unwrap();
        let dest_m = fs::metadata(&dest).unwrap().modified().unwrap();
        assert_eq!(
            src_m, dest_m,
            "die aelteste Datei zu behalten haengt am mtime"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn abbruch_waehrend_der_kopie_laesst_keine_halbe_datei_zurueck() {
        let root = scratch("abbruch");
        let src = root.join("quelle.bin");
        let dest = root.join("ziel.bin");
        fs::write(&src, vec![7u8; 500_000]).unwrap();

        STOP_FLAG.store(true, Ordering::SeqCst);
        let result = copy_with_stop(&src, &dest);
        STOP_FLAG.store(false, Ordering::SeqCst);

        assert!(result.is_err(), "ein Abbruch muss als Fehler zurueckkommen");
        assert!(!dest.exists(), "die halbe Zieldatei muss weg sein");
        assert!(
            src.exists(),
            "das Original wird nie vor der Kopie angefasst"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn nur_exdev_weicht_auf_kopieren_aus() {
        let exdev = std::io::Error::from_raw_os_error(if cfg!(windows) { 17 } else { 18 });
        assert!(is_cross_device(&exdev));
        // Eine fehlende Berechtigung soll ein Fehler bleiben, kein zweiter Versuch.
        assert!(!is_cross_device(&std::io::Error::from_raw_os_error(13)));
        assert!(!is_cross_device(&std::io::Error::other("kein OS-Fehler")));
    }
}
