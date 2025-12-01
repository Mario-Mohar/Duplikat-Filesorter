#!/usr/bin/env python3
"""
Duplicate File Finder for OneDrive
Findet doppelte Dateien basierend auf MD5-Hash und verschiebt Duplikate
in einen separaten Ordner. Erstellt ein Logfile mit allen Aktionen.
"""

import os
import hashlib
import shutil
import argparse
from datetime import datetime
from pathlib import Path
from collections import defaultdict


def calculate_md5(filepath: str, chunk_size: int = 8192) -> str:
    """Berechnet den MD5-Hash einer Datei."""
    md5_hash = hashlib.md5()
    try:
        with open(filepath, "rb") as f:
            for chunk in iter(lambda: f.read(chunk_size), b""):
                md5_hash.update(chunk)
        return md5_hash.hexdigest()
    except (IOError, OSError) as e:
        print(f"Fehler beim Lesen von {filepath}: {e}")
        return None


def get_file_size(filepath: str) -> int:
    """Gibt die Dateigröße zurück."""
    try:
        return os.path.getsize(filepath)
    except OSError:
        return 0


def find_duplicates(source_dir: str, duplicates_dir: str) -> dict:
    """
    Findet alle Duplikate im Quellverzeichnis.
    Gruppiert Dateien zuerst nach Größe, dann nach MD5-Hash.
    """
    # Schritt 1: Dateien nach Größe gruppieren (schnelle Vorfilterung)
    size_groups = defaultdict(list)

    print("Scanne Verzeichnis nach Dateien...")
    file_count = 0

    for root, dirs, files in os.walk(source_dir):
        # Überspringe den Duplikate-Ordner
        if os.path.abspath(root).startswith(os.path.abspath(duplicates_dir)):
            continue

        for filename in files:
            filepath = os.path.join(root, filename)
            file_size = get_file_size(filepath)
            if file_size > 0:  # Ignoriere leere Dateien
                size_groups[file_size].append(filepath)
                file_count += 1

    print(f"Gefunden: {file_count} Dateien")

    # Schritt 2: Nur Dateien mit gleicher Größe per Hash prüfen
    hash_groups = defaultdict(list)
    files_to_hash = sum(len(files) for files in size_groups.values() if len(files) > 1)

    print(f"Berechne Hashes für {files_to_hash} potentielle Duplikate...")
    hashed = 0

    for size, files in size_groups.items():
        if len(files) > 1:  # Nur wenn mehrere Dateien gleiche Größe haben
            for filepath in files:
                file_hash = calculate_md5(filepath)
                if file_hash:
                    hash_groups[file_hash].append(filepath)
                    hashed += 1
                    if hashed % 100 == 0:
                        print(f"  Fortschritt: {hashed}/{files_to_hash}")

    # Schritt 3: Nur Gruppen mit echten Duplikaten behalten
    duplicates = {h: files for h, files in hash_groups.items() if len(files) > 1}

    return duplicates


def setup_directories(source_dir: str) -> str:
    """Erstellt den Duplikate-Ordner falls nicht vorhanden."""
    duplicates_dir = os.path.join(source_dir, "Duplikate")

    if os.path.exists(duplicates_dir):
        print(f"Duplikate-Ordner existiert bereits: {duplicates_dir}")
    else:
        os.makedirs(duplicates_dir)
        print(f"Duplikate-Ordner erstellt: {duplicates_dir}")

    return duplicates_dir


def move_duplicates(duplicates: dict, duplicates_dir: str, log_file: str, dry_run: bool = False):
    """
    Verschiebt Duplikate in den Duplikate-Ordner.
    Behält jeweils die erste Datei (älteste Änderungszeit) am Originalort.
    """
    moved_count = 0
    total_size_saved = 0

    with open(log_file, "w", encoding="utf-8") as log:
        log.write(f"Duplikat-Finder Log - {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}\n")
        log.write("=" * 80 + "\n\n")

        if dry_run:
            log.write("*** TROCKENLAUF - Keine Dateien wurden verschoben ***\n\n")

        for file_hash, files in duplicates.items():
            # Sortiere nach Änderungszeit (älteste zuerst behalten)
            files_with_mtime = [(f, os.path.getmtime(f)) for f in files]
            files_with_mtime.sort(key=lambda x: x[1])

            original = files_with_mtime[0][0]
            duplicates_to_move = [f[0] for f in files_with_mtime[1:]]

            log.write(f"Hash: {file_hash}\n")
            log.write(f"  Original behalten: {original}\n")

            for dup_file in duplicates_to_move:
                file_size = get_file_size(dup_file)

                # Erstelle Zielordner-Struktur im Duplikate-Ordner
                rel_path = os.path.relpath(dup_file, os.path.dirname(duplicates_dir))
                dest_path = os.path.join(duplicates_dir, rel_path)
                dest_dir = os.path.dirname(dest_path)

                # Falls Dateiname bereits existiert, füge Nummer hinzu
                if os.path.exists(dest_path):
                    base, ext = os.path.splitext(dest_path)
                    counter = 1
                    while os.path.exists(f"{base}_{counter}{ext}"):
                        counter += 1
                    dest_path = f"{base}_{counter}{ext}"

                if dry_run:
                    log.write(f"  [WÜRDE VERSCHIEBEN] {dup_file}\n")
                    log.write(f"    -> {dest_path}\n")
                else:
                    try:
                        os.makedirs(dest_dir, exist_ok=True)
                        shutil.move(dup_file, dest_path)
                        log.write(f"  Verschoben: {dup_file}\n")
                        log.write(f"    -> {dest_path}\n")
                        moved_count += 1
                        total_size_saved += file_size
                    except (IOError, OSError) as e:
                        log.write(f"  FEHLER beim Verschieben von {dup_file}: {e}\n")

            log.write("\n")

        # Zusammenfassung
        log.write("=" * 80 + "\n")
        log.write("ZUSAMMENFASSUNG\n")
        log.write(f"Duplikat-Gruppen gefunden: {len(duplicates)}\n")
        log.write(f"Dateien verschoben: {moved_count}\n")
        log.write(f"Speicherplatz freigegeben: {total_size_saved / (1024*1024):.2f} MB\n")

    return moved_count, total_size_saved


def main():
    parser = argparse.ArgumentParser(
        description="Findet und verschiebt doppelte Dateien in einen Duplikate-Ordner."
    )
    parser.add_argument(
        "source_dir",
        help="Der zu durchsuchende Ordner (z.B. OneDrive-Pfad)"
    )
    parser.add_argument(
        "--dry-run", "-n",
        action="store_true",
        help="Trockenlauf - zeigt nur was passieren würde, ohne Dateien zu verschieben"
    )
    parser.add_argument(
        "--log-file", "-l",
        help="Pfad zur Logdatei (Standard: duplikate_log.txt im Quellordner)"
    )

    args = parser.parse_args()

    # Validiere Quellverzeichnis
    source_dir = os.path.abspath(args.source_dir)
    if not os.path.isdir(source_dir):
        print(f"Fehler: Verzeichnis nicht gefunden: {source_dir}")
        return 1

    print(f"\nDuplikat-Finder für: {source_dir}")
    print("=" * 60)

    if args.dry_run:
        print("*** TROCKENLAUF MODUS - Keine Dateien werden verschoben ***\n")

    # Setup
    duplicates_dir = setup_directories(source_dir)
    log_file = args.log_file or os.path.join(source_dir, "duplikate_log.txt")

    # Duplikate finden
    print()
    duplicates = find_duplicates(source_dir, duplicates_dir)

    if not duplicates:
        print("\nKeine Duplikate gefunden!")
        return 0

    total_duplicates = sum(len(files) - 1 for files in duplicates.values())
    print(f"\nGefunden: {len(duplicates)} Gruppen mit insgesamt {total_duplicates} Duplikaten")

    # Duplikate verschieben
    print("\nVerschiebe Duplikate...")
    moved, size_saved = move_duplicates(duplicates, duplicates_dir, log_file, args.dry_run)

    # Ergebnis
    print("\n" + "=" * 60)
    print("FERTIG!")
    print(f"Duplikate verschoben: {moved}")
    print(f"Speicherplatz freigegeben: {size_saved / (1024*1024):.2f} MB")
    print(f"Logdatei: {log_file}")

    return 0


if __name__ == "__main__":
    exit(main())
