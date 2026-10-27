// Tray tooltip shown above the media popup (text set by the app).
const { listen } = window.__TAURI__.event;
const tip = document.getElementById("tip");
listen("tip-text", (e) => { tip.textContent = e.payload; });
listen("tip-shown", () => {
  tip.classList.remove("in");
  void tip.offsetWidth;
  tip.classList.add("in");
});
