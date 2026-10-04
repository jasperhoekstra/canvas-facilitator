// AC-UI: render the real UI against mocked IPC at both reference sizes with headless Edge/Chrome.
// Usage: node scripts/screenshots.mjs [outDir]   (set BROWSER to override the browser path)
import { spawn, execFileSync } from "node:child_process";
import { existsSync, mkdirSync } from "node:fs";
import { resolve } from "node:path";

const out = resolve(process.argv[2] ?? "screenshots");
mkdirSync(out, { recursive: true });
const browser =
  process.env.BROWSER ??
  [
    "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
    "C:/Program Files/Google/Chrome/Application/chrome.exe",
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
  ].find(existsSync);
if (!browser) throw new Error("No Edge/Chrome found; set BROWSER");

const vite = spawn(process.execPath, ["node_modules/vite/bin/vite.js", "--port", "1421", "--strictPort"], { stdio: "pipe" });
await new Promise((ok) => vite.stdout.on("data", (d) => String(d).includes("Local") && ok()));

const sizes = [[1440, 900], [1280, 720]];
const views = ["live", "live&long", "live&late", "live&panel", "detail", "history", "costs", "settings", "setup", "new"];
const zoom = [["live", 2], ["live&panel", 2]]; // 200% zoom of a 1440×900 window
try {
  for (const [w, h] of sizes) {
    for (const v of views) shot(`view=${v}`, w, h, 1);
  }
  for (const [v, f] of zoom) shot(`view=${v}`, 1440 / f, 900 / f, f);
} finally {
  vite.kill();
}

function shot(query, w, h, scale) {
  const name = `${query.replace(/[=&]/g, "-")}-${w}x${h}${scale > 1 ? `@${scale}x` : ""}.png`;
  execFileSync(browser, [
    "--headless=new", "--disable-gpu", "--hide-scrollbars", `--force-device-scale-factor=${scale}`,
    `--window-size=${w},${h}`, "--virtual-time-budget=3000", `--screenshot=${resolve(out, name)}`,
    `http://localhost:1421/mock.html?${query}`,
  ], { stdio: "ignore" });
  console.log("✓", name);
}
