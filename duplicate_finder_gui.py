#!/usr/bin/env python3
"""
Duplicate File Finder GUI
Grafische Oberfläche zum Finden und Verschieben von Duplikaten.
Funktioniert auf Windows und Linux.
"""

import os
import hashlib
import shutil
import threading
import tkinter as tk
from tkinter import ttk, filedialog, messagebox, scrolledtext
from datetime import datetime
from pathlib import Path
from collections import defaultdict


class DuplicateFinderGUI:
    def __init__(self, root):
        self.root = root
        self.root.title("Duplikat-Finder")
        self.root.geometry("700x600")
        self.root.minsize(600, 500)

        # Variablen
        self.source_dir = tk.StringVar()
        self.duplicates_dir = tk.StringVar()
        self.dry_run = tk.BooleanVar(value=True)
        self.is_running = False

        self.setup_ui()

    def setup_ui(self):
        """Erstellt die Benutzeroberfläche."""
        # Hauptframe mit Padding
        main_frame = ttk.Frame(self.root, padding="10")
        main_frame.grid(row=0, column=0, sticky="nsew")

        self.root.columnconfigure(0, weight=1)
        self.root.rowconfigure(0, weight=1)
        main_frame.columnconfigure(1, weight=1)

        # === Quellordner ===
        ttk.Label(main_frame, text="OneDrive-Ordner:", font=("", 10, "bold")).grid(
            row=0, column=0, sticky="w", pady=(0, 5)
        )

        source_frame = ttk.Frame(main_frame)
        source_frame.grid(row=1, column=0, columnspan=3, sticky="ew", pady=(0, 15))
        source_frame.columnconfigure(0, weight=1)

        self.source_entry = ttk.Entry(source_frame, textvariable=self.source_dir)
        self.source_entry.grid(row=0, column=0, sticky="ew", padx=(0, 5))

        ttk.Button(source_frame, text="Durchsuchen...", command=self.browse_source).grid(
            row=0, column=1
        )

        # === Duplikate-Ordner ===
        ttk.Label(main_frame, text="Duplikate-Ordner (optional):", font=("", 10, "bold")).grid(
            row=2, column=0, sticky="w", pady=(0, 5)
        )
        ttk.Label(main_frame, text="Leer lassen = 'Duplikate' im Quellordner", font=("", 8)).grid(
            row=3, column=0, sticky="w", pady=(0, 5)
        )

        dup_frame = ttk.Frame(main_frame)
        dup_frame.grid(row=4, column=0, columnspan=3, sticky="ew", pady=(0, 15))
        dup_frame.columnconfigure(0, weight=1)

        self.dup_entry = ttk.Entry(dup_frame, textvariable=self.duplicates_dir)
        self.dup_entry.grid(row=0, column=0, sticky="ew", padx=(0, 5))

        ttk.Button(dup_frame, text="Durchsuchen...", command=self.browse_duplicates).grid(
            row=0, column=1
        )

        # === Optionen ===
        options_frame = ttk.LabelFrame(main_frame, text="Optionen", padding="10")
        options_frame.grid(row=5, column=0, columnspan=3, sticky="ew", pady=(0, 15))

        self.dry_run_check = ttk.Checkbutton(
            options_frame,
            text="Trockenlauf (zeigt nur was passieren würde, verschiebt nichts)",
            variable=self.dry_run
        )
        self.dry_run_check.grid(row=0, column=0, sticky="w")

        # === Buttons ===
        button_frame = ttk.Frame(main_frame)
        button_frame.grid(row=6, column=0, columnspan=3, pady=(0, 15))

        self.start_button = ttk.Button(
            button_frame,
            text="Duplikate suchen",
            command=self.start_search,
            style="Accent.TButton"
        )
        self.start_button.grid(row=0, column=0, padx=5)

        self.stop_button = ttk.Button(
            button_frame,
            text="Abbrechen",
            command=self.stop_search,
            state="disabled"
        )
        self.stop_button.grid(row=0, column=1, padx=5)

        # === Fortschrittsbalken ===
        self.progress_var = tk.DoubleVar()
        self.progress_bar = ttk.Progressbar(
            main_frame,
            variable=self.progress_var,
            maximum=100
        )
        self.progress_bar.grid(row=7, column=0, columnspan=3, sticky="ew", pady=(0, 5))

        self.status_label = ttk.Label(main_frame, text="Bereit")
        self.status_label.grid(row=8, column=0, columnspan=3, sticky="w", pady=(0, 10))

        # === Log-Ausgabe ===
        ttk.Label(main_frame, text="Log:", font=("", 10, "bold")).grid(
            row=9, column=0, sticky="w", pady=(0, 5)
        )

        self.log_text = scrolledtext.ScrolledText(
            main_frame,
            height=15,
            wrap=tk.WORD,
            font=("Consolas", 9) if os.name == "nt" else ("Monospace", 9)
        )
        self.log_text.grid(row=10, column=0, columnspan=3, sticky="nsew", pady=(0, 10))
        main_frame.rowconfigure(10, weight=1)

        # === Statistik ===
        self.stats_label = ttk.Label(main_frame, text="", font=("", 9))
        self.stats_label.grid(row=11, column=0, columnspan=3, sticky="w")

    def browse_source(self):
        """Öffnet Dialog zur Auswahl des Quellordners."""
        directory = filedialog.askdirectory(title="OneDrive-Ordner auswählen")
        if directory:
            self.source_dir.set(directory)

    def browse_duplicates(self):
        """Öffnet Dialog zur Auswahl des Duplikate-Ordners."""
        directory = filedialog.askdirectory(title="Duplikate-Ordner auswählen")
        if directory:
            self.duplicates_dir.set(directory)

    def log(self, message: str):
        """Fügt eine Nachricht zum Log hinzu."""
        self.log_text.insert(tk.END, message + "\n")
        self.log_text.see(tk.END)
        self.root.update_idletasks()

    def set_status(self, message: str):
        """Aktualisiert die Statusanzeige."""
        self.status_label.config(text=message)
        self.root.update_idletasks()

    def set_progress(self, value: float):
        """Aktualisiert den Fortschrittsbalken."""
        self.progress_var.set(value)
        self.root.update_idletasks()

    def start_search(self):
        """Startet die Duplikatsuche in einem separaten Thread."""
        source = self.source_dir.get().strip()

        if not source:
            messagebox.showerror("Fehler", "Bitte wähle einen OneDrive-Ordner aus.")
            return

        if not os.path.isdir(source):
            messagebox.showerror("Fehler", f"Ordner nicht gefunden:\n{source}")
            return

        # UI vorbereiten
        self.is_running = True
        self.start_button.config(state="disabled")
        self.stop_button.config(state="normal")
        self.log_text.delete(1.0, tk.END)
        self.set_progress(0)
        self.stats_label.config(text="")

        # In separatem Thread ausführen
        thread = threading.Thread(target=self.run_search, daemon=True)
        thread.start()

    def stop_search(self):
        """Bricht die Suche ab."""
        self.is_running = False
        self.set_status("Abbruch angefordert...")

    def run_search(self):
        """Führt die eigentliche Duplikatsuche durch."""
        try:
            source = self.source_dir.get().strip()
            dup_dir = self.duplicates_dir.get().strip()
            dry_run = self.dry_run.get()

            # Duplikate-Ordner bestimmen
            if dup_dir:
                duplicates_dir = dup_dir
            else:
                duplicates_dir = os.path.join(source, "Duplikate")

            # Duplikate-Ordner erstellen
            if not os.path.exists(duplicates_dir):
                if not dry_run:
                    os.makedirs(duplicates_dir)
                    self.log(f"Duplikate-Ordner erstellt: {duplicates_dir}")
                else:
                    self.log(f"[TROCKENLAUF] Würde Ordner erstellen: {duplicates_dir}")
            else:
                self.log(f"Duplikate-Ordner existiert: {duplicates_dir}")

            # Dateien scannen
            self.set_status("Scanne Dateien...")
            self.log("\nScanne Verzeichnis nach Dateien...")

            size_groups = defaultdict(list)
            file_count = 0

            for root, dirs, files in os.walk(source):
                if not self.is_running:
                    self.log("\n*** Abgebrochen ***")
                    return

                # Überspringe Duplikate-Ordner
                if os.path.abspath(root).startswith(os.path.abspath(duplicates_dir)):
                    continue

                for filename in files:
                    filepath = os.path.join(root, filename)
                    try:
                        file_size = os.path.getsize(filepath)
                        if file_size > 0:
                            size_groups[file_size].append(filepath)
                            file_count += 1
                    except OSError:
                        pass

            self.log(f"Gefunden: {file_count} Dateien")

            # Hash berechnen
            self.set_status("Berechne Hashes...")
            hash_groups = defaultdict(list)

            files_to_hash = [f for files in size_groups.values() if len(files) > 1 for f in files]
            total_to_hash = len(files_to_hash)

            self.log(f"\nBerechne Hashes für {total_to_hash} potentielle Duplikate...")

            for i, filepath in enumerate(files_to_hash):
                if not self.is_running:
                    self.log("\n*** Abgebrochen ***")
                    return

                file_hash = self.calculate_md5(filepath)
                if file_hash:
                    hash_groups[file_hash].append(filepath)

                progress = ((i + 1) / total_to_hash) * 50
                self.set_progress(progress)

                if (i + 1) % 50 == 0:
                    self.set_status(f"Hash: {i + 1}/{total_to_hash}")

            # Duplikate filtern
            duplicates = {h: files for h, files in hash_groups.items() if len(files) > 1}

            if not duplicates:
                self.log("\nKeine Duplikate gefunden!")
                self.set_status("Fertig - keine Duplikate")
                self.finish_search()
                return

            total_duplicates = sum(len(files) - 1 for files in duplicates.values())
            self.log(f"\nGefunden: {len(duplicates)} Gruppen mit {total_duplicates} Duplikaten")

            # Logdatei erstellen
            log_file = os.path.join(source, f"duplikate_log_{datetime.now().strftime('%Y%m%d_%H%M%S')}.txt")

            # Duplikate verschieben
            self.set_status("Verschiebe Duplikate...")
            moved_count, size_saved = self.move_duplicates(
                duplicates, duplicates_dir, log_file, dry_run
            )

            # Ergebnis anzeigen
            self.set_progress(100)
            mode = "[TROCKENLAUF] " if dry_run else ""
            self.log(f"\n{'='*50}")
            self.log(f"{mode}FERTIG!")
            self.log(f"Duplikate verschoben: {moved_count}")
            self.log(f"Speicherplatz: {size_saved / (1024*1024):.2f} MB")
            self.log(f"Logdatei: {log_file}")

            self.stats_label.config(
                text=f"{mode}Verschoben: {moved_count} Dateien | Gespart: {size_saved / (1024*1024):.2f} MB"
            )
            self.set_status("Fertig!")

        except Exception as e:
            self.log(f"\nFEHLER: {e}")
            self.set_status(f"Fehler: {e}")

        finally:
            self.finish_search()

    def finish_search(self):
        """Setzt die UI nach der Suche zurück."""
        self.is_running = False
        self.start_button.config(state="normal")
        self.stop_button.config(state="disabled")

    def calculate_md5(self, filepath: str) -> str:
        """Berechnet den MD5-Hash einer Datei."""
        md5_hash = hashlib.md5()
        try:
            with open(filepath, "rb") as f:
                for chunk in iter(lambda: f.read(8192), b""):
                    md5_hash.update(chunk)
            return md5_hash.hexdigest()
        except (IOError, OSError):
            return None

    def move_duplicates(self, duplicates: dict, duplicates_dir: str, log_file: str, dry_run: bool):
        """Verschiebt Duplikate und erstellt Logdatei."""
        moved_count = 0
        total_size_saved = 0
        total_groups = len(duplicates)

        with open(log_file, "w", encoding="utf-8") as log:
            log.write(f"Duplikat-Finder Log - {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}\n")
            log.write("=" * 80 + "\n\n")

            if dry_run:
                log.write("*** TROCKENLAUF - Keine Dateien wurden verschoben ***\n\n")

            for idx, (file_hash, files) in enumerate(duplicates.items()):
                if not self.is_running:
                    break

                # Sortiere nach Änderungszeit
                files_with_mtime = [(f, os.path.getmtime(f)) for f in files]
                files_with_mtime.sort(key=lambda x: x[1])

                original = files_with_mtime[0][0]
                dups_to_move = [f[0] for f in files_with_mtime[1:]]

                log.write(f"Hash: {file_hash}\n")
                log.write(f"  Original: {original}\n")

                self.log(f"\nGruppe {idx + 1}/{total_groups}: {os.path.basename(original)}")

                for dup_file in dups_to_move:
                    try:
                        file_size = os.path.getsize(dup_file)
                    except OSError:
                        file_size = 0

                    # Zielordner-Struktur
                    rel_path = os.path.relpath(dup_file, os.path.dirname(duplicates_dir))
                    dest_path = os.path.join(duplicates_dir, rel_path)

                    # Dateiname-Konflikt lösen
                    if os.path.exists(dest_path):
                        base, ext = os.path.splitext(dest_path)
                        counter = 1
                        while os.path.exists(f"{base}_{counter}{ext}"):
                            counter += 1
                        dest_path = f"{base}_{counter}{ext}"

                    if dry_run:
                        log.write(f"  [WÜRDE VERSCHIEBEN] {dup_file}\n")
                        self.log(f"  -> Würde verschieben: {os.path.basename(dup_file)}")
                        moved_count += 1
                        total_size_saved += file_size
                    else:
                        try:
                            os.makedirs(os.path.dirname(dest_path), exist_ok=True)
                            shutil.move(dup_file, dest_path)
                            log.write(f"  Verschoben: {dup_file}\n")
                            log.write(f"    -> {dest_path}\n")
                            self.log(f"  -> Verschoben: {os.path.basename(dup_file)}")
                            moved_count += 1
                            total_size_saved += file_size
                        except (IOError, OSError) as e:
                            log.write(f"  FEHLER: {dup_file}: {e}\n")
                            self.log(f"  FEHLER: {e}")

                log.write("\n")

                # Progress aktualisieren
                progress = 50 + ((idx + 1) / total_groups) * 50
                self.set_progress(progress)

            # Zusammenfassung
            log.write("=" * 80 + "\n")
            log.write("ZUSAMMENFASSUNG\n")
            log.write(f"Duplikat-Gruppen: {total_groups}\n")
            log.write(f"Dateien verschoben: {moved_count}\n")
            log.write(f"Speicherplatz: {total_size_saved / (1024*1024):.2f} MB\n")

        return moved_count, total_size_saved


def main():
    root = tk.Tk()

    # Styling für besseres Aussehen
    style = ttk.Style()

    # Versuche ein modernes Theme zu verwenden
    available_themes = style.theme_names()
    if "clam" in available_themes:
        style.theme_use("clam")
    elif "vista" in available_themes:
        style.theme_use("vista")

    app = DuplicateFinderGUI(root)
    root.mainloop()


if __name__ == "__main__":
    main()
