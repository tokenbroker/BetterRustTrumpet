// Flyout, after the original's FlyoutWindow/DeviceView/AppItemView. Devices
// and apps are reconciled by id/key so a slider in use is never replaced.
// While visible: meters at 30 fps from cached interfaces (`peaks`), a full
// snapshot every second and whenever Windows reports a change.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { Menu, MenuItem, CheckMenuItem, PredefinedMenuItem, Submenu } = window.__TAURI__.menu;

const $ = (id) => document.getElementById(id);
const deviceTemplate = $("device-template");
const appTemplate = $("app-template");

// The user's settings (flyout_config); defaults until they are loaded.
const settings = {
  peakMeterFps: 30, // PeakMeterFps (20 in eco mode)
  peakMeterStyle: 0, // PeakMeterStyle: Classic, Dotted, Blocks, Bars, Wave
  useVolumeTickSound: true, // UseVolumeTickSound
  useSmoothVolumeAnimation: true, // UseSmoothVolumeAnimation
  volumeAnimationSpeed: 0.08, // VolumeAnimationSpeed
  showAppTooltips: true, // ShowAppTooltips
};
const PEAK_SMOOTHING = 0.35;

// Each device carries `hidden` and `hiddenApps`; each app `hidden`,
// `appId`, `exeName`, its `rule` and its per-process `sessions`.
let snapshot = { devices: [] };
let expanded = false; // IsExpanded, from the settings on first show
let configLoaded = false;
let pinned = false;
const deviceViews = new Map(); // id -> view
let visible = false;
let refreshTimer = null;
let peakTimer = null;
let popup = null; // { kind, deviceId, appKey, view }

// ---------- helpers ----------
const percent = (v) => Math.round(v * 100);
// Hidden app entries saved for a device (may include apps not running).
function hiddenOn(deviceId) {
  return findDevice(deviceId)?.hiddenApps || [];
}
function entryOf(deviceId, app) {
  return { deviceId, appId: app.appId, exeName: app.exeName, displayName: app.displayName };
}
const isLocked = (app) => app.rule?.volumeMode === "Lock";
function exeName(app) {
  const path = app.executable || "";
  return path.split(/[\\/]/).pop() || app.displayName;
}
// Apps by executable name, ordinal (AppItemViewModel's comparer); System
// sounds last.
function sortApps(apps) {
  return [...apps].sort((a, b) => {
    if (a.isSystemSounds !== b.isSystemSounds) return a.isSystemSounds ? 1 : -1;
    const x = exeName(a), y = exeName(b);
    return x < y ? -1 : x > y ? 1 : 0;
  });
}
// Collapsed: the default device only. Expanded: every visible device, the
// others in reverse name order (they are inserted at the top in name order)
// and the default at the bottom, nearest the taskbar.
function shownDevices() {
  const visibleDevices = snapshot.devices.filter((d) => !d.hidden);
  const def = visibleDevices.find((d) => d.isDefault);
  if (!expanded) return def ? [def] : visibleDevices.slice(0, 1);
  const others = visibleDevices.filter((d) => !d.isDefault)
    .sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0))
    .reverse();
  return def ? [...others, def] : others;
}

// ---------- tick sound ----------
const tick = new Audio("tick.wav");
let lastTick = 0;
function playTick(volume) {
  if (!settings.useVolumeTickSound) return;
  const now = performance.now();
  if (now - lastTick < 50) return;
  lastTick = now;
  tick.volume = Math.max(0.1, volume / 100);
  tick.currentTime = 0;
  tick.play().catch(() => {});
}

// ---------- slider (VolumeSlider) ----------
// value 0..100. `send(value)` applies it; sends are coalesced per frame.
function makeSlider(el, send) {
  const state = { value: 0, busyUntil: 0, dragging: false, anim: null, peaks: [0, 0], drawn: [0, 0] };
  let pending = null;
  const push = (value) => {
    if (pending === null) {
      requestAnimationFrame(() => {
        const v = pending;
        pending = null;
        send(v);
      });
    }
    pending = value;
  };
  const width = () => el.clientWidth;
  const draw = () => {
    const left = (width() - 16) * (state.value / 100);
    el.querySelector(".thumb").style.left = `${left}px`;
    el.querySelector(".fill").style.width = `${left + 8}px`;
  };
  const set = (value, fromUser) => {
    value = Math.max(0, Math.min(100, Math.round(value)));
    if (value === state.value && !fromUser) return;
    const changed = value !== state.value;
    state.value = value;
    draw();
    el.dispatchEvent(new CustomEvent("value", { detail: value }));
    if (fromUser && changed) {
      push(value);
      playTick(value);
    }
  };
  const fromPointer = (event) => {
    const rect = el.getBoundingClientRect();
    return ((event.clientX - rect.left - 8) / (rect.width - 16)) * 100;
  };
  // Click on the track slides there (CubicEase EaseOut); the duration
  // follows VolumeAnimationSpeed (default 0.08 -> about 292 ms).
  const animateTo = (target) => {
    cancelAnimationFrame(state.anim);
    if (!settings.useSmoothVolumeAnimation) return set(target, true);
    const speed = Math.min(0.5, Math.max(0.02, settings.volumeAnimationSpeed));
    const duration = 320 - ((speed - 0.02) / 0.48) * 220;
    const start = state.value, began = performance.now();
    const step = (now) => {
      const t = Math.min(1, (now - began) / duration);
      const eased = 1 - Math.pow(1 - t, 3);
      set(start + (target - start) * eased, true);
      state.busyUntil = performance.now() + 500;
      if (t < 1 && !state.dragging) state.anim = requestAnimationFrame(step);
    };
    state.anim = requestAnimationFrame(step);
  };
  el.addEventListener("pointerdown", (event) => {
    if (event.button !== 0 || el.classList.contains("disabled")) return;
    event.stopPropagation();
    el.setPointerCapture(event.pointerId);
    state.busyUntil = Infinity;
    const onThumb = event.target.classList.contains("thumb");
    state.dragging = onThumb;
    if (onThumb) cancelAnimationFrame(state.anim);
    else animateTo(fromPointer(event));
  });
  el.addEventListener("pointermove", (event) => {
    if (!el.hasPointerCapture(event.pointerId)) return;
    if (!state.dragging) {
      // Dragging after a track press takes over from the slide.
      state.dragging = true;
      cancelAnimationFrame(state.anim);
    }
    set(fromPointer(event), true);
  });
  const release = () => {
    state.dragging = false;
    state.busyUntil = performance.now() + 500;
  };
  el.addEventListener("pointerup", release);
  el.addEventListener("pointercancel", release);
  // Mouse wheel: +/-2 per notch.
  el.addEventListener("wheel", (event) => {
    event.preventDefault();
    event.stopPropagation();
    if (el.classList.contains("disabled")) return;
    cancelAnimationFrame(state.anim);
    set(state.value + (event.deltaY < 0 ? 2 : -2), true);
    state.busyUntil = performance.now() + 500;
  }, { passive: false });
  return {
    el,
    get value() { return state.value; },
    step(delta) {
      if (el.classList.contains("disabled")) return;
      cancelAnimationFrame(state.anim);
      set(state.value + delta, true);
      state.busyUntil = performance.now() + 500;
    },
    // External value; ignored while the user is using the slider.
    update(value) {
      if (performance.now() < state.busyUntil) return;
      set(value, false);
      draw();
    },
    redraw: draw,
    setPeaks(channels, volume) { state.peaks = channels.map((c) => c * volume); },
    // Each frame the drawn bars move 35% of the way to their target. Only
    // Classic draws the second (right channel) bar.
    animatePeaks() {
      const span = width() - 16;
      const bars = el.querySelectorAll(".peak");
      for (let i = 0; i < (settings.peakMeterStyle === 0 ? 2 : 1); i++) {
        state.drawn[i] += (state.peaks[i] - state.drawn[i]) * PEAK_SMOOTHING;
        if (state.drawn[i] < 0.001) state.drawn[i] = 0;
        bars[i].style.width = `${Math.max(0, span * state.drawn[i])}px`;
      }
    },
    resetPeaks() {
      state.peaks = [0, 0];
      state.drawn = [0, 0];
      el.querySelectorAll(".peak").forEach((b) => (b.style.width = "0px"));
    },
  };
}

// ---------- device view ----------
// Device glyphs (Segoe MDL2): Bar0..Bar3 by level, Mute.
function deviceGlyphs(device) {
  if (device.muted) return { ghost: "", level: "" };
  const v = device.volume;
  const level = v > 0.66 ? "" : v > 0.33 ? "" : v > 0 ? "" : "";
  return { ghost: "", level };
}

function bindDeviceRow(row, deviceId) {
  const slider = makeSlider(row.querySelector(".slider"), (value) => {
    // The value before this change and the name make an undo step.
    const device = findDevice(deviceId);
    const previous = device ? percent(device.volume) : null;
    if (device) device.volume = value / 100;
    invoke("set_device_volume", { deviceId, volume: value / 100, previous, name: device?.name ?? "" })
      .catch(() => {});
  });
  slider.el.addEventListener("value", (e) => (row.querySelector(".device-value").textContent = e.detail));
  row.querySelector(".device-mute").addEventListener("click", (event) => {
    event.stopPropagation();
    toggleDeviceMute(deviceId);
  });
  row.addEventListener("keydown", (event) => rowKeys(event, slider, () => toggleDeviceMute(deviceId),
    () => openDevicePopup(deviceId)));
  return slider;
}

function createDeviceView(device) {
  const el = deviceTemplate.content.firstElementChild.cloneNode(true);
  const view = { id: device.id, el, apps: new Map() };
  view.slider = bindDeviceRow(el.querySelector(".device-row"), device.id);
  el.querySelector(".device-block").addEventListener("contextmenu", (event) => {
    event.preventDefault();
    openDevicePopup(device.id);
  });
  return view;
}

function updateDeviceRow(row, slider, device) {
  const glyphs = deviceGlyphs(device);
  row.querySelector(".ghost").textContent = glyphs.ghost;
  row.querySelector(".level").textContent = glyphs.level;
  slider.update(percent(device.volume));
  row.querySelector(".device-value").textContent = slider.value;
  row.dataset.muted = device.muted ? "1" : "";
}

// ---------- app rows ----------
// `child` is one process's session, for the app popup's rows (the
// original's ChildApps); those set that session only and record no undo.
function createAppRow(deviceId, app, inPopup = false, child = null) {
  const el = appTemplate.content.firstElementChild.cloneNode(true);
  const row = { key: app.key, el, deviceId, processId: child?.processId ?? null };
  const name = child ? child.displayName : app.displayName;
  const letter = el.querySelector(".letter");
  if (app.isSystemSounds) {
    letter.classList.add("glyph");
    letter.style.fontWeight = "normal";
    letter.textContent = "";
  } else {
    const first = [...app.displayName.trim()].find((c) => /[\p{L}\p{N}]/u.test(c));
    letter.textContent = first ? first.toUpperCase() : "?";
  }
  if (app.executable) {
    invoke("app_icon", { path: app.executable }).then((url) => {
      if (!url) return;
      const img = el.querySelector(".app-icon");
      img.src = url;
      img.hidden = false;
      letter.hidden = true;
    }).catch(() => {});
  }
  const mute = el.querySelector(".app-mute");
  if (settings.showAppTooltips) mute.title = name;
  mute.setAttribute("aria-label", `${name}, toggle mute`);
  row.slider = makeSlider(el.querySelector(".slider"), (value) => {
    if (child) {
      invoke("set_process_volume", { deviceId, appKey: app.key, processId: child.processId, volume: value / 100 })
        .catch(() => {});
      return;
    }
    const current = findApp(deviceId, app.key);
    const previous = current ? percent(current.volume) : null;
    if (current) current.volume = value / 100;
    invoke("set_app_volume", { deviceId, appKey: app.key, volume: value / 100, previous, name: app.displayName })
      .catch(() => {});
  });
  row.slider.el.addEventListener("value", (e) => (el.querySelector(".app-value").textContent = e.detail));
  const toggleMute = () => (child ? toggleProcessMute(deviceId, app.key, child.processId) : toggleAppMute(deviceId, app.key));
  mute.addEventListener("click", (event) => {
    event.stopPropagation();
    if (event.ctrlKey) return;
    toggleMute();
  });
  // Middle-click on the icon hides the app on this device.
  el.querySelector(".icon-cell").addEventListener("auxclick", (event) => {
    if (event.button === 1 && !inPopup) hideApp(deviceId, app.key, el);
  });
  el.querySelector(".icon-cell").addEventListener("mousedown", (event) => {
    if (event.button === 1) event.preventDefault();
  });
  // Ctrl+click (not on the slider) solos the app on its device.
  el.addEventListener("click", (event) => {
    if (event.ctrlKey && !child && !event.target.closest(".slider")) solo(deviceId, app.key, el);
  });
  if (!inPopup) {
    el.addEventListener("contextmenu", (event) => {
      event.preventDefault();
      openAppPopup(deviceId, app.key);
    });
  }
  el.addEventListener("keydown", (event) => rowKeys(event, row.slider,
    toggleMute, () => !inPopup && openAppPopup(deviceId, app.key)));
  return row;
}

// A Lock rule disables the slider (it would only snap back) and shows the
// lock badge on the icon.
function updateAppRow(row, app) {
  const source = row.processId === null ? app : app.sessions?.find((s) => s.processId === row.processId);
  if (!source) return;
  row.slider.update(percent(source.volume));
  row.el.querySelector(".app-value").textContent = row.slider.value;
  row.el.classList.toggle("muted", source.muted);
  const locked = isLocked(app);
  row.slider.el.classList.toggle("disabled", locked);
  row.el.querySelector(".lock-badge").hidden = !locked;
}

// Right/+ and Left/- step 1, M or . toggle mute, Space opens the popup.
function rowKeys(event, slider, mute, open) {
  if (event.key === "ArrowRight" || event.key === "+") slider.step(1);
  else if (event.key === "ArrowLeft" || event.key === "-") slider.step(-1);
  else if (event.key === "m" || event.key === "M" || event.key === ".") mute();
  else if (event.key === " ") open();
  else return;
  event.preventDefault();
  event.stopPropagation();
}

// ---------- actions ----------
function findDevice(id) {
  return snapshot.devices.find((d) => d.id === id);
}
function findApp(deviceId, key) {
  return findDevice(deviceId)?.apps.find((a) => a.key === key);
}
function toggleDeviceMute(deviceId) {
  const device = findDevice(deviceId);
  if (!device) return;
  device.muted = !device.muted;
  render();
  invoke("set_device_mute", { deviceId, muted: device.muted, name: device.name }).then(refresh).catch(() => {});
}
function toggleAppMute(deviceId, key) {
  const app = findApp(deviceId, key);
  if (!app) return;
  app.muted = !app.muted;
  render();
  invoke("set_app_mute", { deviceId, appKey: key, muted: app.muted, name: app.displayName })
    .then(refresh).catch(() => {});
}
function toggleProcessMute(deviceId, key, processId) {
  const session = findApp(deviceId, key)?.sessions?.find((s) => s.processId === processId);
  if (!session) return;
  session.muted = !session.muted;
  render();
  invoke("set_process_mute", { deviceId, appKey: key, processId, muted: session.muted })
    .then(refresh).catch(() => {});
}
// Mutes every other app on the device and unmutes this one; if it is
// already soloed, unmutes everything.
function solo(deviceId, key, el) {
  const device = findDevice(deviceId);
  if (!device) return;
  const apps = device.apps.filter((a) => !a.hidden);
  const soloed = apps.every((a) => (a.key === key ? !a.muted : a.muted)) && apps.length > 1;
  for (const a of apps) {
    const muted = soloed ? false : a.key !== key;
    if (a.muted !== muted) {
      a.muted = muted;
      invoke("set_app_mute", { deviceId, appKey: a.key, muted, name: a.displayName }).catch(() => {});
    }
  }
  el.classList.remove("solo-pulse");
  void el.offsetWidth;
  el.classList.add("solo-pulse");
  render();
}
// Hidden entries are saved (HiddenAppEntriesJson), so they stay hidden
// after a restart, as in the original.
function hideApp(deviceId, key, el) {
  const app = findApp(deviceId, key);
  if (!app) return;
  const finish = () => {
    app.hidden = true;
    const device = findDevice(deviceId);
    device.hiddenApps = [...hiddenOn(deviceId), entryOf(deviceId, app)];
    render();
    invoke("hide_app", { entry: entryOf(deviceId, app) }).then(refresh).catch(() => {});
  };
  if (el) {
    el.classList.add("hiding");
    setTimeout(finish, 140);
  } else {
    finish();
  }
}

// ---------- popups (AppPopup / DialogChrome) ----------
function tool(glyph, label, action, extra = "") {
  const button = document.createElement("button");
  button.type = "button";
  button.className = `tool glyph ${extra}`;
  button.textContent = glyph;
  button.title = label;
  button.setAttribute("aria-label", label);
  button.addEventListener("click", (event) => action(event, button));
  return button;
}

// Native menus open below their button, like ButtonEx.
async function menuBelow(button, items) {
  const menu = await Menu.new({ items });
  const rect = button.getBoundingClientRect();
  const { LogicalPosition } = window.__TAURI__.dpi;
  await menu.popup(new LogicalPosition(rect.left, rect.bottom));
}

function deviceMenuItems(deviceId) {
  const items = [];
  const device = findDevice(deviceId);
  if (!device) return items;
  const visibleCount = snapshot.devices.filter((d) => !d.hidden).length;
  if (!device.isDefault) {
    items.push({ text: "Set as default device", action: () => {
      invoke("set_default_device", { deviceId }).then(refresh).catch(() => {});
      closePopup();
    } });
    if (visibleCount > 1) {
      items.push({ text: "Hide this device", action: () => {
        device.hidden = true;
        closePopup();
        render();
        invoke("hide_device", { deviceId, name: device.name }).then(refresh).catch(() => {});
      } });
    }
  }
  if (hiddenOn(deviceId).length) {
    items.push({ submenu: "Restore hidden app entries on this device", items: restoreItems(deviceId) });
  }
  return items;
}

function restoreItems(deviceId) {
  const entryName = (e) => e.displayName || e.exeName || e.appId;
  const restored = () => {
    closePopup();
    refresh();
  };
  return [
    ...hiddenOn(deviceId).map((e) => ({
      text: entryName(e),
      action: () => invoke("unhide_app", { entry: { ...e, deviceId } }).then(restored).catch(() => {}),
    })),
    { separator: true },
    { text: "Restore all on this device",
      action: () => invoke("unhide_device_apps", { deviceId }).then(restored).catch(() => {}) },
  ];
}

async function buildItems(specs) {
  const out = [];
  for (const spec of specs) {
    if (spec.separator) out.push(await PredefinedMenuItem.new({ item: "Separator" }));
    else if (spec.submenu) out.push(await Submenu.new({ text: spec.submenu, items: await buildItems(spec.items) }));
    else if (spec.checked !== undefined) out.push(await CheckMenuItem.new({ text: spec.text, checked: spec.checked, action: spec.action }));
    else out.push(await MenuItem.new({ text: spec.text, action: spec.action }));
  }
  return out;
}

function showPopup(title, tools, body, anchor, offset) {
  closePopup(true);
  $("popup-title").textContent = title;
  $("popup-toolbar").replaceChildren(...tools);
  $("popup-body").replaceChildren(body);
  $("dismiss").hidden = false;
  $("dismiss").classList.remove("out");
  const pop = $("popup");
  pop.hidden = false;
  const rootRect = $("root").getBoundingClientRect();
  const anchorRect = anchor.getBoundingClientRect();
  const height = pop.offsetHeight;
  const top = Math.max(0, Math.min(anchorRect.top - rootRect.top - offset, rootRect.height - height));
  pop.style.top = `${top}px`;
}

// The rule buttons (FocusedAppItemViewModel), left to right: Lock, Launch
// volume, Focus lost, Hard mute. Each takes effect and closes the popup.
function ruleTools(deviceId, app) {
  const rule = app.rule || { volumeMode: "None", volumePercent: 0, hardMuted: false, focusLostEnabled: false };
  const change = (kind, value = 0) => {
    closePopup();
    invoke("app_rule", { change: {
      deviceId, appKey: app.key, exeName: app.exeName, displayName: app.displayName,
      iconPath: app.executable || null, kind, value,
    } }).then(refresh).catch(() => {});
  };
  const active = (button, on) => button.classList.toggle("active", on);
  const current = percent(findApp(deviceId, app.key)?.volume ?? app.volume);
  const locked = rule.volumeMode === "Lock";
  // Lock is direct: set the slider, then lock it there.
  const lock = tool("", locked ? "Unlock this app's volume" : "Lock volume at the current level",
    () => change(locked ? "none" : "lock", current));
  active(lock, locked);
  // Launch is a menu: picking the value is the action.
  const launch = tool("", "Set launch volume", async (_, button) => {
    const isLaunch = rule.volumeMode === "Launch";
    await menuBelow(button, await buildItems([
      ...[10, 20, 30, 50, 75].map((v) => ({
        text: `${v}%`, checked: isLaunch && rule.volumePercent === v, action: () => change("launch", v),
      })),
      { separator: true },
      { text: `Current volume (${current}%)`, action: () => change("launch", current) },
      { separator: true },
      { text: "No volume rule", checked: rule.volumeMode === "None", action: () => change("none") },
    ]));
  });
  active(launch, rule.volumeMode === "Launch");
  const focus = tool("", "Focus lost behavior", async (_, button) => {
    await menuBelow(button, await buildItems([{
      text: rule.focusLostEnabled ? "Exclude this app when focus is lost" : "Include this app when focus is lost",
      checked: rule.focusLostEnabled, action: () => change("focusLost"),
    }]));
  });
  active(focus, rule.focusLostEnabled);
  const hard = tool("", "Hard mute this app", async (_, button) => {
    await menuBelow(button, await buildItems([{
      text: rule.hardMuted ? "Stop keeping muted" : "Keep muted (survives restart)",
      checked: rule.hardMuted, action: () => change("hardMute"),
    }]));
  });
  active(hard, rule.hardMuted);
  return [lock, launch, focus, hard];
}

function openAppPopup(deviceId, key) {
  const device = findDevice(deviceId);
  const app = findApp(deviceId, key);
  const view = deviceViews.get(deviceId);
  const anchor = view?.apps.get(key)?.el;
  if (!device || !app || !anchor) return;
  // One row per process session, like the original's ChildApps.
  const sessions = app.sessions?.length ? app.sessions : [null];
  const rows = sessions.map((session) => {
    const row = createAppRow(deviceId, app, true, session);
    updateAppRow(row, app);
    return row;
  });
  const body = document.createElement("div");
  body.className = "apps";
  body.replaceChildren(...rows.map((r) => r.el));
  const tools = app.exeName ? ruleTools(deviceId, app) : [];
  tools.push(tool("", "Hide this app entry", () => {
    closePopup();
    hideApp(deviceId, key, anchor);
  }));
  if (!app.isSystemSounds) {
    tools.push(tool("", "Assign this app to a playback device", async (_, button) => {
      const current = findApp(deviceId, key)?.routedDeviceId || null;
      const route = (target) => invoke("set_app_device", { appKey: key, deviceId: target })
        .then(refresh).catch(() => {});
      await menuBelow(button, await buildItems([
        { text: "Always use the default playback device", checked: current === null, action: () => route(null) },
        { separator: true },
        ...snapshot.devices.map((d) => ({ text: d.name, checked: current === d.id, action: () => route(d.id) })),
      ]));
    }));
  }
  tools.push(tool("", "Close", () => closePopup(), "close"));
  popup = { kind: "app", deviceId, key, rows };
  // Shifted up by 48 + 6 + 1 so its first row lies over the original row.
  showPopup(app.displayName, tools, body, anchor, 48 + 6 + 1);
  rows.forEach((r) => r.slider.redraw());
}

function openDevicePopup(deviceId) {
  const device = findDevice(deviceId);
  const view = deviceViews.get(deviceId);
  if (!device || !view) return;
  const items = deviceMenuItems(deviceId);
  // With nothing to offer the original does not open the popup at all.
  if (!items.length) return;
  const row = deviceTemplate.content.querySelector(".device-row").cloneNode(true);
  const slider = bindDeviceRow(row, deviceId);
  updateDeviceRow(row, slider, device);
  const tools = [
    tool("", "More commands", async (_, button) => menuBelow(button, await buildItems(deviceMenuItems(deviceId)))),
    tool("", "Close", () => closePopup(), "close"),
  ];
  popup = { kind: "device", deviceId, row, slider };
  showPopup(device.name, tools, row, view.el.querySelector(".device-row"), 48 + 6);
  slider.redraw();
}

function closePopup(immediate = false) {
  if (!popup && $("popup").hidden) return;
  popup = null;
  $("popup").hidden = true;
  if (immediate) {
    $("dismiss").hidden = true;
    return;
  }
  const dismiss = $("dismiss");
  dismiss.classList.add("out");
  setTimeout(() => {
    if (!popup) dismiss.hidden = true;
  }, 150);
}
$("dismiss").addEventListener("click", () => closePopup());

// ---------- header ----------
function showExpanded() {
  document.body.classList.toggle("expanded", expanded);
  $("expand").setAttribute("aria-label", expanded
    ? "Collapse to show only the default playback device"
    : "Expand to show all playback devices");
}
$("expand").addEventListener("click", () => {
  expanded = !expanded;
  showExpanded();
  invoke("set_expanded", { expanded }).catch(() => {});
  render(true);
});
// The original's header menu: its handler never matches, so it does nothing.
$("header").addEventListener("contextmenu", async (event) => {
  event.preventDefault();
  if (event.target.closest(".header-button")) return;
  const menu = await Menu.new({ items: await buildItems([{ text: "Set as default device", action: () => {} }]) });
  await menu.popup();
});
$("pin").addEventListener("click", async () => {
  pinned = await invoke("toggle_pin");
  showPin();
});
function showPin() {
  $("pin").textContent = pinned ? "" : "";
}
$("restore").addEventListener("click", async (event) => {
  const top = shownDevices()[0];
  if (top) await menuBelow(event.currentTarget, await buildItems(restoreItems(top.id)));
});

// ---------- render ----------
function render(cascade = false) {
  const shown = shownDevices();
  const container = $("devices");
  $("no-devices").hidden = snapshot.devices.length > 0;
  $("list-wrap").hidden = snapshot.devices.length === 0;
  const visibleCount = snapshot.devices.filter((d) => !d.hidden).length;
  $("header").hidden = shown.length === 0;
  $("expand").disabled = visibleCount <= 1;
  $("header-name").textContent = shown[0]?.name || "";
  $("restore").hidden = !shown[0] || hiddenOn(shown[0].id).length === 0;

  const keep = new Set(shown.map((d) => d.id));
  for (const [id, view] of deviceViews) {
    if (!keep.has(id)) {
      view.el.remove();
      deviceViews.delete(id);
    }
  }
  shown.forEach((device, index) => {
    let view = deviceViews.get(device.id);
    if (!view) {
      view = createDeviceView(device);
      deviceViews.set(device.id, view);
    }
    if (container.children[index] !== view.el) container.insertBefore(view.el, container.children[index] || null);
    const title = view.el.querySelector(".device-title");
    title.textContent = device.name;
    // The top device's name is in the header instead (row height kept).
    title.classList.toggle("hidden-title", index === 0);
    updateDeviceRow(view.el.querySelector(".device-row"), view.slider, device);

    const appsEl = view.el.querySelector(".apps");
    const apps = sortApps(device.apps.filter((a) => !a.hidden));
    const keepApps = new Set(apps.map((a) => a.key));
    for (const [key, row] of view.apps) {
      if (!keepApps.has(key)) {
        row.el.remove();
        view.apps.delete(key);
      }
    }
    apps.forEach((app, appIndex) => {
      let row = view.apps.get(app.key);
      if (!row) {
        row = createAppRow(device.id, app);
        view.apps.set(app.key, row);
      }
      if (appsEl.children[appIndex] !== row.el) appsEl.insertBefore(row.el, appsEl.children[appIndex] || null);
      updateAppRow(row, app);
    });
  });
  if (popup) {
    const app = popup.kind === "app" ? findApp(popup.deviceId, popup.key) : null;
    const device = findDevice(popup.deviceId);
    if (popup.kind === "app" && app) popup.rows.forEach((r) => updateAppRow(r, app));
    else if (popup.kind === "device" && device) updateDeviceRow(popup.row, popup.slider, device);
    else closePopup(true);
  }
  for (const view of deviceViews.values()) {
    view.slider.redraw();
    for (const row of view.apps.values()) row.slider.redraw();
  }
  if (cascade) runCascade();
  fit();
}

// Row cascade on open: each device block and app row, top to bottom,
// delayed min(i, 24) x 28 ms.
function runCascade() {
  const rows = [...document.querySelectorAll("#devices .device-block, #devices .app")];
  rows.forEach((row, i) => {
    row.classList.remove("cascade");
    row.style.setProperty("--delay", `${Math.min(i, 24) * 28}ms`);
    void row.offsetWidth;
    row.classList.add("cascade");
  });
}

function fit() {
  const height = $("list-wrap").hidden ? $("no-devices").offsetHeight + 48 : $("list-wrap").offsetHeight;
  invoke("fit_flyout", { height }).catch(() => {});
  requestAnimationFrame(() => {
    const scroller = $("scroller");
    document.body.classList.toggle("scrollbar", scroller.scrollHeight > scroller.clientHeight + 1);
  });
}

async function refresh() {
  try {
    snapshot = await invoke("snapshot");
    render();
  } catch {
    // Keep the last state; the next refresh retries.
  }
}

async function readPeaks() {
  let peaks = [];
  try {
    peaks = await invoke("peaks");
  } catch {
    return;
  }
  for (const p of peaks) {
    const device = findDevice(p.deviceId);
    const view = deviceViews.get(p.deviceId);
    if (!device || !view) continue;
    if (p.appKey === null || p.appKey === undefined) {
      view.slider.setPeaks(p.channels, device.volume);
      if (popup?.kind === "device" && popup.deviceId === p.deviceId) popup.slider.setPeaks(p.channels, device.volume);
    } else {
      const app = device.apps.find((a) => a.key === p.appKey);
      if (!app) continue;
      const inPopup = popup?.kind === "app" && popup.deviceId === p.deviceId && popup.key === p.appKey;
      if (p.processId === null || p.processId === undefined) {
        view.apps.get(p.appKey)?.slider.setPeaks(p.channels, app.volume);
      } else if (inPopup) {
        const session = app.sessions?.find((s) => s.processId === p.processId);
        const row = popup.rows.find((r) => r.processId === p.processId);
        if (session && row) row.slider.setPeaks(p.channels, session.volume);
      }
    }
  }
  for (const view of deviceViews.values()) {
    view.slider.animatePeaks();
    for (const row of view.apps.values()) row.slider.animatePeaks();
  }
  if (popup?.slider) popup.slider.animatePeaks();
  popup?.rows?.forEach((r) => r.slider.animatePeaks());
}

// ---------- show / hide ----------
// The original's flyout colours for the system theme (App.xaml): dark,
// light (SystemUsesLightTheme), or accent ("Show accent colour on Start
// and taskbar"), plus UseCustomSliderColors and the peak meter style.
const rgba = (hex, alpha) => {
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
};
async function applyConfig() {
  let config;
  try {
    config = await invoke("flyout_config");
  } catch {
    return; // Keep the defaults.
  }
  for (const key of Object.keys(settings)) {
    if (config[key] !== undefined) settings[key] = config[key];
  }
  if (!configLoaded) {
    configLoaded = true;
    expanded = config.isExpanded;
    showExpanded();
  }
  const accentMode = config.accentOnTaskbar && !config.light;
  const root = document.documentElement.style;
  const body = document.body.classList;
  body.toggle("light", config.light);
  body.toggle("accent-mode", accentMode);
  body.toggle("opaque", !config.transparency);
  root.setProperty("--accent", config.accent);
  root.setProperty("--accent-dark1", config.accentDark1);
  root.setProperty("--accent-30", rgba(config.accent, config.transparency ? 0.3 : 0.5));
  root.setProperty("--accent-22", rgba(config.accent, 0.22));
  // FlyoutThemeTrackRightBackground: accent mode, light, dark.
  root.setProperty("--track-rest", accentMode ? "rgba(255, 255, 255, 0.5)"
    : config.light ? "rgba(0, 0, 0, 0.224)" : "rgba(255, 255, 255, 0.224)");
  // Custom colours; an unset one falls back to the original's defaults
  // (#0078D7 for thumb, fill and peaks, #505050 for the track).
  const custom = config.customColors;
  const set = (name, value) => (value ? root.setProperty(name, value) : root.removeProperty(name));
  set("--thumb", custom && (custom.thumb || "rgb(0, 120, 215)"));
  set("--fill", custom && (custom.fill || "rgb(0, 120, 215)"));
  set("--track", custom && (custom.track || "rgb(80, 80, 80)"));
  set("--peak", custom && (custom.peak || "rgb(0, 120, 215)"));
  body.toggle("custom-colors", !!custom);
  for (let i = 0; i <= 4; i++) body.toggle(`peak-style-${i}`, settings.peakMeterStyle === i);
  if (visible) {
    clearInterval(peakTimer);
    peakTimer = setInterval(readPeaks, 1000 / settings.peakMeterFps);
  }
}

async function start() {
  visible = true;
  closePopup(true);
  const rootEl = $("root");
  rootEl.classList.remove("closing", "opening");
  await applyConfig();
  await refresh();
  void rootEl.offsetWidth;
  rootEl.classList.add("opening");
  runCascade();
  document.body.classList.remove("keyboard");
  const first = document.querySelector("#devices .device-row");
  if (first) first.focus({ preventScroll: true });
  if (!refreshTimer) refreshTimer = setInterval(refresh, 1000);
  if (!peakTimer) peakTimer = setInterval(readPeaks, 1000 / settings.peakMeterFps);
}
function stop() {
  visible = false;
  clearInterval(refreshTimer);
  clearInterval(peakTimer);
  refreshTimer = peakTimer = null;
  closePopup(true);
  for (const view of deviceViews.values()) {
    view.slider.resetPeaks();
    for (const row of view.apps.values()) row.slider.resetPeaks();
  }
}

listen("flyout-shown", start);
listen("flyout-hiding", () => {
  const rootEl = $("root");
  rootEl.classList.remove("opening");
  void rootEl.offsetWidth;
  rootEl.classList.add("closing");
});
listen("flyout-hidden", stop);
listen("devices-changed", () => visible && refresh());
listen("volume-changed", () => visible && refresh());

document.addEventListener("pointerdown", () => document.body.classList.remove("keyboard"), true);
document.addEventListener("keydown", (event) => {
  if (["Tab", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"].includes(event.key)) {
    document.body.classList.add("keyboard");
  }
  // Ctrl+Z / Ctrl+Y undo and redo the last volume or mute change.
  if (event.ctrlKey && !event.altKey && (event.key === "z" || event.key === "y")) {
    invoke(event.key === "z" ? "undo" : "redo").then((applied) => applied && refresh()).catch(() => {});
    event.preventDefault();
    return;
  }
  if (event.key === "Escape") {
    if (popup) closePopup();
    else invoke("hide_flyout");
    event.preventDefault();
  } else if (event.altKey && event.code === "Space") {
    event.preventDefault();
  }
});
document.addEventListener("contextmenu", (event) => event.preventDefault());
window.addEventListener("resize", () => {
  for (const view of deviceViews.values()) {
    view.slider.redraw();
    for (const row of view.apps.values()) row.slider.redraw();
  }
});

invoke("is_pinned").then((value) => {
  pinned = value;
  showPin();
});
refresh();
// Events emitted before this script loaded (e.g. `--show`) are missed.
window.__TAURI__.window.getCurrentWindow().isVisible().then((shown) => shown && start());
