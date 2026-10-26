// Preview flyout: polls the audio snapshot while visible and reconciles the
// DOM by device id / app key so sliders being dragged are never replaced.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const devicesEl = document.getElementById("devices");
const statusEl = document.getElementById("status");
const deviceTemplate = document.getElementById("device-template");
const appTemplate = document.getElementById("app-template");
const deviceViews = new Map();
let timer = null;

const SPEAKER = '<svg viewBox="0 0 16 16"><path d="M3 6h2.5L9 3v10L5.5 10H3z"/><path d="M11 5.5a3.5 3.5 0 0 1 0 5" stroke="currentColor" stroke-width="1.4" fill="none"/></svg>';
const MUTED = '<svg viewBox="0 0 16 16"><path d="M3 6h2.5L9 3v10L5.5 10H3z"/><path d="M11 6l3 4M14 6l-3 4" stroke="currentColor" stroke-width="1.4"/></svg>';

function setFill(input) {
  input.style.setProperty("--fill", `${input.value}%`);
}

// Coalesce slider input into at most one call per animation frame.
function throttled(send) {
  let pending = null;
  return (value) => {
    if (pending === null) {
      requestAnimationFrame(() => {
        const v = pending;
        pending = null;
        send(v).catch(showError);
      });
    }
    pending = value;
  };
}

function bindSlider(input, valueEl, send) {
  const push = throttled(send);
  input.addEventListener("pointerdown", () => (input.dataset.dragging = "1"));
  const done = () => delete input.dataset.dragging;
  input.addEventListener("pointerup", done);
  input.addEventListener("pointercancel", done);
  input.addEventListener("input", () => {
    setFill(input);
    valueEl.textContent = input.value;
    push(Number(input.value) / 100);
  });
  input.addEventListener("wheel", (event) => {
    event.preventDefault();
    input.value = Math.min(100, Math.max(0, Number(input.value) + (event.deltaY < 0 ? 2 : -2)));
    input.dispatchEvent(new Event("input"));
  }, { passive: false });
}

function update(row, volume, muted, peak) {
  const input = row.querySelector("input");
  const percent = Math.round(volume * 100);
  if (!input.dataset.dragging && document.activeElement !== input) {
    input.value = percent;
    setFill(input);
    row.querySelector(".value").textContent = percent;
  }
  row.classList.toggle("muted", muted);
  row.dataset.muted = muted ? "1" : "";
  row.querySelector(".peak-fill").style.width = `${Math.min(1, peak) * volume * 100}%`;
}

function createDevice(device) {
  const el = deviceTemplate.content.firstElementChild.cloneNode(true);
  const master = el.querySelector(".master");
  const input = master.querySelector("input");
  bindSlider(input, master.querySelector(".value"), (v) =>
    invoke("set_device_volume", { deviceId: device.id, volume: v }));
  master.querySelector(".mute").addEventListener("click", () =>
    invoke("set_device_mute", { deviceId: device.id, muted: !master.dataset.muted })
      .then(refresh).catch(showError));
  return { el, apps: new Map() };
}

function createApp(deviceId, app) {
  const el = appTemplate.content.firstElementChild.cloneNode(true);
  const input = el.querySelector("input");
  input.setAttribute("aria-label", `${app.displayName} volume`);
  el.querySelector(".initial").textContent = (app.displayName.trim()[0] || "?").toUpperCase();
  el.querySelector(".mute").setAttribute("aria-label", `Mute ${app.displayName}`);
  bindSlider(input, el.querySelector(".value"), (v) =>
    invoke("set_app_volume", { deviceId, appKey: app.key, volume: v }));
  el.querySelector(".mute").addEventListener("click", () =>
    invoke("set_app_mute", { deviceId, appKey: app.key, muted: !el.dataset.muted })
      .then(refresh).catch(showError));
  return el;
}

function render(snapshot) {
  statusEl.hidden = snapshot.devices.length > 0;
  statusEl.textContent = "No active output devices";
  const seen = new Set();
  snapshot.devices.forEach((device, index) => {
    seen.add(device.id);
    let view = deviceViews.get(device.id);
    if (!view) {
      view = createDevice(device);
      deviceViews.set(device.id, view);
    }
    if (devicesEl.children[index] !== view.el) devicesEl.insertBefore(view.el, devicesEl.children[index] || null);
    view.el.querySelector(".device-name").textContent = device.name;
    view.el.querySelector(".default-badge").hidden = !device.isDefault;
    const master = view.el.querySelector(".master");
    update(master, device.volume, device.muted, device.peak);
    master.querySelector(".mute").innerHTML = device.muted ? MUTED : SPEAKER;

    const appsEl = view.el.querySelector(".apps");
    const seenApps = new Set();
    device.apps.forEach((app, appIndex) => {
      seenApps.add(app.key);
      let row = view.apps.get(app.key);
      if (!row) {
        row = createApp(device.id, app);
        view.apps.set(app.key, row);
      }
      if (appsEl.children[appIndex] !== row) appsEl.insertBefore(row, appsEl.children[appIndex] || null);
      row.querySelector(".app-name").textContent = app.displayName;
      row.title = app.executable || app.displayName;
      update(row, app.volume, app.muted, app.peak);
    });
    for (const [key, row] of view.apps) {
      if (!seenApps.has(key)) { row.remove(); view.apps.delete(key); }
    }
  });
  for (const [id, view] of deviceViews) {
    if (!seen.has(id)) { view.el.remove(); deviceViews.delete(id); }
  }
  invoke("fit_flyout", { height: document.getElementById("flyout").scrollHeight }).catch(() => {});
}

function showError(error) {
  statusEl.hidden = false;
  statusEl.textContent = String(error);
}

async function refresh() {
  try {
    render(await invoke("snapshot"));
  } catch (error) {
    showError(error);
  }
}

function start() {
  refresh();
  if (!timer) timer = setInterval(refresh, 100);
}
function stop() {
  clearInterval(timer);
  timer = null;
}

listen("flyout-shown", start);
listen("flyout-hidden", stop);
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") invoke("hide_flyout");
});
refresh();
