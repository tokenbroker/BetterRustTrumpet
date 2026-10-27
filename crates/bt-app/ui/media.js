// Media popup, after the original's MediaPopupWindow: refreshes the media
// session every second and the progress every 100 ms while visible.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const $ = (id) => document.getElementById(id);
// Phosphor (MIT) path data, as the original's PhosphorIconData.
const ICON = {
  play: "M234.49,111.07,90.41,22.94A20,20,0,0,0,60,39.87V216.13a20,20,0,0,0,30.41,16.93l144.08-88.13a19.82,19.82,0,0,0,0-33.86ZM84,208.85V47.15L216.16,128Z",
  pause: "M200,28H160a20,20,0,0,0-20,20V208a20,20,0,0,0,20,20h40a20,20,0,0,0,20-20V48A20,20,0,0,0,200,28Zm-4,176H164V52h32ZM96,28H56A20,20,0,0,0,36,48V208a20,20,0,0,0,20,20H96a20,20,0,0,0,20-20V48A20,20,0,0,0,96,28ZM92,204H60V52H92Z",
  speakerHigh: "M157.27,21.22a12,12,0,0,0-12.64,1.31L75.88,76H32A20,20,0,0,0,12,96v64a20,20,0,0,0,20,20H75.88l68.75,53.47A12,12,0,0,0,164,224V32A12,12,0,0,0,157.27,21.22ZM36,100H68v56H36Zm104,99.46L92,162.13V93.87l48-37.33ZM212,128a44,44,0,0,1-11,29.11,12,12,0,1,1-18-15.88,20,20,0,0,0,0-26.43,12,12,0,0,1,18-15.86A43.94,43.94,0,0,1,212,128Zm40,0a83.87,83.87,0,0,1-21.39,56,12,12,0,0,1-17.89-16,60,60,0,0,0,0-80,12,12,0,1,1,17.88-16A83.87,83.87,0,0,1,252,128Z",
  speakerLow: "M157.27,21.22a12,12,0,0,0-12.64,1.31L75.88,76H32A20,20,0,0,0,12,96v64a20,20,0,0,0,20,20H75.88l68.75,53.47A12,12,0,0,0,164,224V32A12,12,0,0,0,157.27,21.22ZM36,100H68v56H36Zm104,99.46L92,162.13V93.87l48-37.33ZM212,128a44,44,0,0,1-11,29.11,12,12,0,0,1-18-15.88,20,20,0,0,0,0-26.44,12,12,0,0,1,18-15.85A43.94,43.94,0,0,1,212,128Z",
  speakerNone: "M157.27,21.22a12,12,0,0,0-12.64,1.31L75.88,76H32A20,20,0,0,0,12,96v64a20,20,0,0,0,20,20H75.88l68.75,53.47A12,12,0,0,0,164,224V32A12,12,0,0,0,157.27,21.22ZM36,100H68v56H36Zm104,99.46L92,162.12V93.88l48-37.34Z",
  speakerSlash: "M56.88,31.93A12,12,0,1,0,39.12,48.07L64.51,76H32A20,20,0,0,0,12,96v64a20,20,0,0,0,20,20H75.88l68.75,53.47A12,12,0,0,0,164,224V185.44l35.12,38.63a12,12,0,0,0,17.76-16.14ZM36,100H68v56H36Zm104,99.46L92,162.13V106.24L140,159Zm-31-134a12,12,0,0,1,2.11-16.84l33.51-26.07A12,12,0,0,1,164,32V94.94a12,12,0,0,1-24,0V56.54l-14.15,11A12,12,0,0,1,109,65.44Zm74,49.35a12,12,0,0,1,18-15.85,44,44,0,0,1,5.55,50.21,12,12,0,0,1-21-11.55A19.67,19.67,0,0,0,188,128,20,20,0,0,0,183,114.79ZM252,128a84.18,84.18,0,0,1-19.11,53.35,12,12,0,1,1-18.53-15.25A60,60,0,0,0,212.73,88a12,12,0,1,1,17.88-16A83.87,83.87,0,0,1,252,128Z",
  repeat: "M20,128A76.08,76.08,0,0,1,96,52h99l-3.52-3.51a12,12,0,1,1,17-17l24,24a12,12,0,0,1,0,17l-24,24a12,12,0,0,1-17-17L195,76H96a52.06,52.06,0,0,0-52,52,12,12,0,0,1-24,0Zm204-12a12,12,0,0,0-12,12,52.06,52.06,0,0,1-52,52H61l3.52-3.51a12,12,0,1,0-17-17l-24,24a12,12,0,0,0,0,17l24,24a12,12,0,1,0,17-17L61,204h99a76.08,76.08,0,0,0,76-76A12,12,0,0,0,224,116Z",
  repeatOnce: "M20,128A76.08,76.08,0,0,1,96,52h99l-3.52-3.51a12,12,0,1,1,17-17l24,24a12,12,0,0,1,0,17l-24,24a12,12,0,0,1-17-17L195,76H96a52.06,52.06,0,0,0-52,52,12,12,0,0,1-24,0Zm204-12a12,12,0,0,0-12,12,52.06,52.06,0,0,1-52,52H61l3.52-3.51a12,12,0,1,0-17-17l-24,24a12,12,0,0,0,0,17l24,24a12,12,0,1,0,17-17L61,204h99a76.08,76.08,0,0,0,76-76A12,12,0,0,0,224,116Zm-88,48a12,12,0,0,0,12-12V104a12,12,0,0,0-17.36-10.74l-16,8a12,12,0,0,0,9.36,22V152A12,12,0,0,0,136,164Z",
};
const DEFAULT_ACCENT = [107, 77, 230];

let state = null;
let trackKey = null;
let artUrl = undefined;
let anchor = { at: 0, positionMs: 0 };
let seekPending = null;
let dragging = null;
let app = null;
let refreshTimer = null;
let progressTimer = null;
let marqueeTimer = null;
let marquee = { x: 0, width: 0 };
let expanded = false;

// WPF Stretch="Uniform" fits the shape's own bounds, not Phosphor's 256 box.
function setIcon(path, d) {
  if (path.getAttribute("d") === d) return;
  path.setAttribute("d", d);
  fit(path);
}
function fit(path) {
  const box = path.getBBox();
  if (box.width > 0 && box.height > 0) {
    path.ownerSVGElement.setAttribute("viewBox", `${box.x} ${box.y} ${box.width} ${box.height}`);
  }
}

function time(ms) {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}

function setTrack(track, fraction) {
  const f = Math.min(1, Math.max(0, fraction));
  track.querySelector(".fill").style.width = `${f * 100}%`;
  track.querySelector(".thumb").style.left = `${f * 100}%`;
}

// Progress between refreshes runs from the last report, as the original's
// interpolated timeline.
function currentPositionMs() {
  if (!state) return 0;
  let position = anchor.positionMs;
  if (state.playing) position += performance.now() - anchor.at;
  return Math.min(position, state.durationMs || position);
}
function renderProgress(positionMs = currentPositionMs()) {
  if (!state) return;
  $("position").textContent = time(positionMs);
  setTrack($("progress"), state.durationMs > 0 ? positionMs / state.durationMs : 0);
}

// "Artist - Title", like GetCurrentMediaInfo.
function titleText(now) {
  if (!now) return "No media playing";
  const title = now.title || now.sourceApp || "";
  return now.artist ? `${now.artist} - ${title}` : title || "No media playing";
}

function startMarquee() {
  const box = $("marquee").clientWidth;
  const text = $("title");
  marquee.width = text.scrollWidth;
  clearInterval(marqueeTimer);
  marqueeTimer = null;
  if (marquee.width > box) {
    marquee.x = 0;
    text.style.left = "0px";
    // 1 px every 50 ms, wrapping in from the right edge.
    marqueeTimer = setInterval(() => {
      marquee.x -= 1;
      if (marquee.x < -marquee.width) marquee.x = box;
      text.style.left = `${marquee.x}px`;
    }, 50);
  } else {
    text.style.left = `${(box - marquee.width) / 2}px`;
  }
}

function setTitle(text, animate) {
  const el = $("title");
  if (el.textContent === text) return;
  if (!animate) {
    el.textContent = text;
    startMarquee();
    return;
  }
  el.classList.remove("change-in");
  el.classList.add("change-out");
  setTimeout(() => {
    el.textContent = text;
    el.classList.remove("change-out");
    void el.offsetWidth;
    el.classList.add("change-in");
    startMarquee();
  }, 90);
}

// The original's GetDominantColor: average of a 32x32 copy, skipping very
// dark and very bright pixels, scaled so the strongest channel is 80%.
function dominantColor(img) {
  try {
    const canvas = document.createElement("canvas");
    canvas.width = canvas.height = 32;
    const ctx = canvas.getContext("2d", { willReadFrequently: true });
    ctx.drawImage(img, 0, 0, 32, 32);
    const px = ctx.getImageData(0, 0, 32, 32).data;
    let r = 0, g = 0, b = 0, n = 0;
    for (let i = 0; i < px.length; i += 4) {
      const sum = px[i] + px[i + 1] + px[i + 2];
      if (sum > 50 && sum < 700) { r += px[i]; g += px[i + 1]; b += px[i + 2]; n++; }
    }
    if (!n) return DEFAULT_ACCENT;
    const avg = [r / n, g / n, b / n].map(Math.floor);
    const max = Math.max(...avg);
    const factor = max > 0 ? (255 / max) * 0.8 : 1;
    return avg.map((c) => Math.min(255, Math.floor(c * factor)));
  } catch {
    return DEFAULT_ACCENT;
  }
}
function setAccent([r, g, b]) {
  const media = $("media");
  media.style.setProperty("--accent", `rgb(${r}, ${g}, ${b})`);
  media.style.setProperty("--accent-light",
    `rgb(${Math.min(255, r + 50)}, ${Math.min(255, g + 50)}, ${Math.min(255, b + 50)})`);
}

function setArt(url) {
  if (url === artUrl) return;
  artUrl = url;
  const img = $("cover-img");
  if (!url) {
    img.hidden = true;
    img.removeAttribute("src");
    setAccent(DEFAULT_ACCENT);
    return;
  }
  img.onload = () => {
    img.hidden = false;
    // Low-res thumbnails (< 300 px) are shown slightly blurred.
    img.classList.toggle("low-res", Math.max(img.naturalWidth, img.naturalHeight) < 300);
    setAccent(dominantColor(img));
  };
  img.src = url;
}

function render(now) {
  const key = now ? `${now.sourceApp}\u0001${now.title}\u0001${now.artist}` : null;
  const changed = key !== trackKey;
  state = now;
  trackKey = key;
  setTitle(titleText(now), changed && $("title").textContent !== "No media playing");
  if (!now) {
    setArt(null);
    return;
  }
  setArt(now.art);
  $("duration").textContent = time(now.durationMs);
  // After a seek, ignore reports until the app catches up (or 2 s pass).
  if (seekPending) {
    const close = Math.abs(now.positionMs - currentPositionMs()) <= 2000;
    if (close || performance.now() > seekPending) seekPending = null;
  }
  if (!seekPending && !dragging) {
    anchor = { at: performance.now(), positionMs: now.positionMs };
    renderProgress();
  }
  $("progress").classList.toggle("disabled", !now.canSeek);
  setIcon($("play-path"), now.playing ? ICON.pause : ICON.play);
  $("play-path").ownerSVGElement.classList.toggle("paused", !now.playing);
  $("shuffle").hidden = !now.canShuffle;
  $("shuffle").classList.toggle("active", now.shuffle === true);
  $("repeat").hidden = !now.canRepeat;
  $("repeat").classList.toggle("active", now.repeat === "list" || now.repeat === "track");
  setIcon($("repeat-path"), now.repeat === "track" ? ICON.repeatOnce : ICON.repeat);
  // Icons in buttons that were hidden could not be measured until now.
  for (const id of ["shuffle", "repeat"]) {
    const svg = $(id).querySelector("svg");
    if (!$(id).hidden && svg.getAttribute("viewBox") === "0 0 256 256") fit(svg.querySelector("path"));
  }
}

function renderVolume(volume, muted) {
  const percent = Math.round(volume * 100);
  $("volume-text").textContent = String(percent);
  setTrack($("volume"), volume);
  const icon = muted || percent === 0 ? ICON.speakerSlash
    : percent < 33 ? ICON.speakerNone
    : percent < 66 ? ICON.speakerLow
    : ICON.speakerHigh;
  setIcon($("vol-path"), icon);
}

async function refreshVolume() {
  try {
    app = state ? await invoke("media_app", { sourceApp: state.sourceApp }) : null;
  } catch {
    app = null;
  }
  $("volume-row").classList.toggle("disabled", !app);
  if (app && dragging !== "volume") renderVolume(app.volume, app.muted);
}

async function refresh() {
  try {
    render(await invoke("now_playing"));
  } catch {
    render(null);
  }
  refreshVolume();
}

function command(name, extra = {}) {
  invoke("media_command", { command: name, ...extra }).then(refresh).catch(() => {});
}
$("play").addEventListener("click", () => command("playPause"));
$("next").addEventListener("click", () => command("next"));
$("previous").addEventListener("click", () => command("previous"));
$("shuffle").addEventListener("click", () => command("shuffle"));
$("repeat").addEventListener("click", () => command("repeat"));

function fraction(track, event) {
  const rect = track.getBoundingClientRect();
  return Math.min(1, Math.max(0, (event.clientX - rect.left) / rect.width));
}

// Seek: drag shows the target, release sends it.
$("progress").addEventListener("pointerdown", (event) => {
  if (!state || !state.canSeek || state.durationMs <= 0) return;
  dragging = "progress";
  $("progress").setPointerCapture(event.pointerId);
  renderProgress(fraction($("progress"), event) * state.durationMs);
});
$("progress").addEventListener("pointermove", (event) => {
  if (dragging === "progress") renderProgress(fraction($("progress"), event) * state.durationMs);
});
$("progress").addEventListener("pointerup", (event) => {
  if (dragging !== "progress") return;
  dragging = null;
  const positionMs = Math.round(fraction($("progress"), event) * state.durationMs);
  anchor = { at: performance.now(), positionMs };
  seekPending = performance.now() + 2000;
  renderProgress();
  command("seek", { positionMs });
});

// Volume of the media app's session; dragging also unmutes, as the original.
function applyVolume(event) {
  const volume = Math.floor(fraction($("volume"), event) * 100) / 100;
  renderVolume(volume, false);
  invoke("set_app_volume", { deviceId: app.deviceId, appKey: app.appKey, volume }).catch(() => {});
  if (app.muted && volume > 0) {
    app.muted = false;
    invoke("set_app_mute", { deviceId: app.deviceId, appKey: app.appKey, muted: false }).catch(() => {});
  }
}
$("volume").addEventListener("pointerdown", (event) => {
  if (!app) return;
  dragging = "volume";
  $("volume").setPointerCapture(event.pointerId);
  applyVolume(event);
});
$("volume").addEventListener("pointermove", (event) => {
  if (dragging === "volume") applyVolume(event);
});
$("volume").addEventListener("pointerup", () => {
  if (dragging === "volume") dragging = null;
});
$("mute").addEventListener("click", () => {
  if (!app) return;
  app.muted = !app.muted;
  renderVolume(app.volume, app.muted);
  invoke("set_app_mute", { deviceId: app.deviceId, appKey: app.appKey, muted: app.muted }).catch(() => {});
});

// Expand: the window grows upwards first, then the card and cover animate.
// Collapse: the card and cover animate, then the window shrinks.
function setExpanded(value, animate) {
  expanded = value;
  const media = $("media");
  const cover = $("cover");
  media.classList.remove("resizing", "collapsing");
  cover.classList.remove("show", "hide");
  $("expand").setAttribute("aria-label", value ? "Hide cover art" : "Show cover art");
  if (!animate) {
    media.classList.toggle("expanded", value);
    cover.hidden = !value;
    return;
  }
  void media.offsetWidth;
  if (value) {
    invoke("media_expand", { expanded: true }).finally(() => {
      cover.hidden = false;
      media.classList.add("resizing", "expanded");
      cover.classList.add("show");
      startMarquee();
    });
  } else {
    media.classList.add("resizing", "collapsing");
    media.classList.remove("expanded");
    cover.classList.add("hide");
    setTimeout(() => {
      if (expanded) return;
      cover.hidden = true;
      media.classList.remove("resizing", "collapsing");
      invoke("media_expand", { expanded: false });
    }, 200);
  }
}
$("expand").addEventListener("click", () => setExpanded(!expanded, true));

document.addEventListener("mouseenter", () => invoke("media_hover", { hovering: true }));
document.addEventListener("mouseleave", () => invoke("media_hover", { hovering: false }));

function start() {
  refresh();
  if (!refreshTimer) refreshTimer = setInterval(refresh, 1000);
  if (!progressTimer) {
    progressTimer = setInterval(() => {
      if (state && state.playing && !dragging && !seekPending) renderProgress();
    }, 100);
  }
}
function stop() {
  clearInterval(refreshTimer);
  clearInterval(progressTimer);
  clearInterval(marqueeTimer);
  refreshTimer = progressTimer = marqueeTimer = null;
}

// Restarts the card's enter ("in") or exit ("out") animation.
function animate(name) {
  const card = $("media");
  card.classList.remove("in", "out");
  void card.offsetWidth;
  card.classList.add(name);
}
listen("media-shown", (event) => {
  setExpanded(event.payload === true, false);
  animate("in");
  start();
  startMarquee();
});
listen("media-hiding", () => animate("out"));
listen("media-hidden", stop);

for (const path of document.querySelectorAll("svg path[d]")) {
  if (path.getAttribute("d")) fit(path);
}
setIcon($("play-path"), ICON.play);
setIcon($("repeat-path"), ICON.repeat);
setIcon($("vol-path"), ICON.speakerHigh);
