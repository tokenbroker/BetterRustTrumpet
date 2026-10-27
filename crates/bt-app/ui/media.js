// Media popup: polls the current media session while visible.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);
const PLAY = '<svg viewBox="0 0 24 24"><path d="M7 4v16l13-8z"/></svg>';
const PAUSE = '<svg viewBox="0 0 24 24"><path d="M6 4h4v16H6zM14 4h4v16h-4z"/></svg>';
let state = null;
let timer = null;
let lastArt = null;

function time(ms) {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}

function render(now) {
  state = now;
  if (!now) {
    $("title").textContent = "Nothing playing";
    $("artist").textContent = "";
    return;
  }
  $("title").textContent = now.title || now.sourceApp || "Unknown";
  $("artist").textContent = [now.artist, now.album].filter(Boolean).join(" — ");
  if (now.art !== lastArt) {
    lastArt = now.art;
    $("art").hidden = !now.art;
    $("art-placeholder").hidden = !!now.art;
    if (now.art) $("art").src = now.art;
  }
  $("position").textContent = time(now.positionMs);
  $("duration").textContent = time(now.durationMs);
  const fraction = now.durationMs > 0 ? Math.min(1, now.positionMs / now.durationMs) : 0;
  $("progress-fill").style.width = `${fraction * 100}%`;
  $("progress").classList.toggle("disabled", !now.canSeek);
  $("play").innerHTML = now.playing ? PAUSE : PLAY;
  $("play").disabled = !now.canPlayPause;
  $("next").disabled = !now.canNext;
  $("previous").disabled = !now.canPrevious;
  $("shuffle").disabled = now.shuffle === null;
  $("shuffle").classList.toggle("active", now.shuffle === true);
  $("repeat").disabled = now.repeat === null;
  $("repeat").classList.toggle("active", now.repeat === "list" || now.repeat === "track");
  $("repeat-one").hidden = now.repeat !== "track";
}

async function refresh() {
  try {
    render(await invoke("now_playing"));
  } catch {
    render(null);
  }
}

function command(name, extra = {}) {
  invoke("media_command", { command: name, ...extra }).then(refresh).catch(() => {});
}
$("play").addEventListener("click", () => command("playPause"));
$("next").addEventListener("click", () => command("next"));
$("previous").addEventListener("click", () => command("previous"));
$("shuffle").addEventListener("click", () => command("shuffle"));
$("repeat").addEventListener("click", () => command("repeat"));
$("progress").addEventListener("click", (event) => {
  if (!state || !state.canSeek || state.durationMs <= 0) return;
  const rect = event.currentTarget.getBoundingClientRect();
  const fraction = Math.min(1, Math.max(0, (event.clientX - rect.left) / rect.width));
  command("seek", { positionMs: Math.round(fraction * state.durationMs) });
});

document.addEventListener("mouseenter", () => invoke("media_hover", { hovering: true }));
document.addEventListener("mouseleave", () => invoke("media_hover", { hovering: false }));

function start() {
  refresh();
  if (!timer) timer = setInterval(refresh, 500);
}
function stop() {
  clearInterval(timer);
  timer = null;
}
// Restarts the card's enter ("in") or exit ("out") animation.
function animate(name) {
  const card = $("media");
  card.classList.remove("in", "out");
  void card.offsetWidth;
  card.classList.add(name);
}
listen("media-shown", () => {
  animate("in");
  start();
});
listen("media-hiding", () => animate("out"));
listen("media-hidden", stop);
