const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { open } = window.__TAURI__.dialog;

// DOM Elements
let sourceDirInput;
let duplicatesDirInput;
let dryRunCheckbox;
let startBtn;
let stopBtn;
let progressBar;
let statusText;
let statsContainer;
let filesScannedEl;
let duplicatesFoundEl;
let spaceSavedEl;
let logOutput;

// State
let isRunning = false;

window.addEventListener("DOMContentLoaded", () => {
  // Get DOM elements
  sourceDirInput = document.getElementById("source-dir");
  duplicatesDirInput = document.getElementById("duplicates-dir");
  dryRunCheckbox = document.getElementById("dry-run");
  startBtn = document.getElementById("start-btn");
  stopBtn = document.getElementById("stop-btn");
  progressBar = document.getElementById("progress");
  statusText = document.getElementById("status");
  statsContainer = document.getElementById("stats");
  filesScannedEl = document.getElementById("files-scanned");
  duplicatesFoundEl = document.getElementById("duplicates-found");
  spaceSavedEl = document.getElementById("space-saved");
  logOutput = document.getElementById("log");

  // Event listeners
  document.getElementById("browse-source").addEventListener("click", browseSource);
  document.getElementById("browse-duplicates").addEventListener("click", browseDuplicates);
  startBtn.addEventListener("click", startSearch);
  stopBtn.addEventListener("click", stopSearch);

  // Listen for progress events from Rust
  listen("scan-progress", (event) => {
    const { status, progress, files_scanned, duplicates_found, space_saved } = event.payload;

    if (status) {
      statusText.textContent = status;
    }

    if (progress !== undefined) {
      progressBar.style.width = `${progress}%`;
    }

    if (files_scanned !== undefined) {
      filesScannedEl.textContent = files_scanned;
      statsContainer.classList.remove("hidden");
    }

    if (duplicates_found !== undefined) {
      duplicatesFoundEl.textContent = duplicates_found;
    }

    if (space_saved !== undefined) {
      spaceSavedEl.textContent = formatBytes(space_saved);
    }
  });

  // Listen for log messages
  listen("log-message", (event) => {
    const { message, level } = event.payload;
    addLog(message, level);
  });
});

async function browseSource() {
  const selected = await open({
    directory: true,
    multiple: false,
    title: "OneDrive-Ordner auswählen"
  });

  if (selected) {
    sourceDirInput.value = selected;
  }
}

async function browseDuplicates() {
  const selected = await open({
    directory: true,
    multiple: false,
    title: "Duplikate-Ordner auswählen"
  });

  if (selected) {
    duplicatesDirInput.value = selected;
  }
}

async function startSearch() {
  const sourceDir = sourceDirInput.value;

  if (!sourceDir) {
    addLog("Bitte wähle einen OneDrive-Ordner aus.", "error");
    return;
  }

  // Reset UI
  isRunning = true;
  startBtn.disabled = true;
  stopBtn.disabled = false;
  progressBar.style.width = "0%";
  logOutput.innerHTML = "";
  statsContainer.classList.add("hidden");
  filesScannedEl.textContent = "0";
  duplicatesFoundEl.textContent = "0";
  spaceSavedEl.textContent = "0 MB";

  const dryRun = dryRunCheckbox.checked;
  const duplicatesDir = duplicatesDirInput.value || null;

  addLog(`Starte Suche in: ${sourceDir}`, "info");
  if (dryRun) {
    addLog("TROCKENLAUF - Keine Dateien werden verschoben", "warning");
  }

  try {
    const result = await invoke("find_duplicates", {
      sourceDir,
      duplicatesDir,
      dryRun
    });

    addLog("", "info");
    addLog("=".repeat(50), "info");
    addLog(`FERTIG! ${dryRun ? "(Trockenlauf)" : ""}`, "success");
    addLog(`Duplikate gefunden: ${result.duplicates_found}`, "success");
    addLog(`Speicherplatz: ${formatBytes(result.space_saved)}`, "success");
    if (result.log_file) {
      addLog(`Logdatei: ${result.log_file}`, "info");
    }

    statusText.textContent = "Fertig!";
    progressBar.style.width = "100%";
  } catch (error) {
    addLog(`Fehler: ${error}`, "error");
    statusText.textContent = "Fehler aufgetreten";
  } finally {
    isRunning = false;
    startBtn.disabled = false;
    stopBtn.disabled = true;
  }
}

async function stopSearch() {
  try {
    await invoke("stop_search");
    addLog("Abbruch angefordert...", "warning");
  } catch (error) {
    addLog(`Fehler beim Abbrechen: ${error}`, "error");
  }
}

function addLog(message, level = "info") {
  const entry = document.createElement("div");
  entry.className = `log-entry log-${level}`;
  entry.textContent = message;
  logOutput.appendChild(entry);
  logOutput.scrollTop = logOutput.scrollHeight;
}

function formatBytes(bytes) {
  if (bytes === 0) return "0 B";

  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  const value = bytes / Math.pow(1024, i);

  return `${value.toFixed(2)} ${units[i]}`;
}
