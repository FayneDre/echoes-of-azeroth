#!/usr/bin/env node
// UI smoke test of the desktop app, driven over the DevTools protocol.
//
//   node tools/ci/ui-smoke.mjs config <out.json> [--port 9333]
//       writes a `tauri build --config` override that opens the window's
//       WebView2 debugging port (Tauri ignores WEBVIEW2_* variables).
//   node tools/ci/ui-smoke.mjs run --exe <app.exe> [--port 9333] [--shots <dir>]
//       starts the app on a new, empty data root and checks what a first-time
//       player sees: the shell, the realm rail, the install wizard, tabs that
//       need a realm disabled, both languages, no page scrollbar, no script
//       error. Screenshots go to --shots.
//
// No network action is taken (nothing is installed).

import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { connect } from "./cdp.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const [command, ...rest] = process.argv.slice(2);
const flag = (name, fallback) => {
  const at = rest.indexOf(`--${name}`);
  return at >= 0 ? rest[at + 1] : fallback;
};
const port = Number(flag("port", "9333"));

if (command === "config") {
  const out = rest.find((a) => !a.startsWith("--") && rest[rest.indexOf(a) - 1] !== "--port");
  if (!out) throw new Error("usage: ui-smoke.mjs config <out.json>");
  const conf = JSON.parse(fs.readFileSync(path.join(here, "../../app/src-tauri/tauri.conf.json"), "utf8"));
  // Arrays replace, so the whole window is repeated with the extra argument.
  const windows = conf.app.windows.map((w) => ({
    ...w,
    additionalBrowserArgs: `--remote-debugging-port=${port} --disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection`,
  }));
  fs.writeFileSync(out, JSON.stringify({ app: { windows } }, null, 2));
  console.log(`wrote ${out}`);
} else if (command === "run") {
  await run();
} else {
  console.error("usage: ui-smoke.mjs config <out.json> | run --exe <app.exe>");
  process.exit(2);
}

async function run() {
  const exe = flag("exe");
  if (!exe) throw new Error("--exe is required");
  const shots = path.resolve(flag("shots", "ui-smoke-shots"));
  fs.mkdirSync(shots, { recursive: true });
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "rk-ui-smoke-"));
  const app = spawn(exe, [], { env: { ...process.env, RK_ROOT: root }, stdio: "ignore" });
  const failures = [];
  const check = (ok, what) => {
    console.log(`${ok ? "ok  " : "FAIL"} ${what}`);
    if (!ok) failures.push(what);
  };
  let page;
  try {
    page = await connect(port, { timeoutMs: 90_000 });
    const errors = [];
    page.on("Runtime.exceptionThrown", (e) => errors.push(e.exceptionDetails?.exception?.description ?? "exception"));
    page.on("Runtime.consoleAPICalled", (e) => {
      if (e.type === "error") errors.push(e.args.map((a) => a.value ?? a.description).join(" "));
    });
    await page.send("Runtime.enable");
    await page.send("Page.enable");
    // A fresh load with the listeners on: errors of the first render count.
    await page.send("Page.reload", { ignoreCache: true });

    await page.waitFor(`document.querySelector("nav.rail button, nav.rail a")`, {
      timeoutMs: 60_000,
      what: "the realm rail",
    });
    await page.waitFor(`document.querySelector(".realm-bar")`, { what: "the Home page" });
    check(await page.evaluate(`document.title.includes("RealmKeeper")`), "window title");

    for (const [locale, wanted] of [
      ["EN", ["Import a realm", "Home"]],
      ["FR", ["Importer un royaume", "Accueil"]],
    ]) {
      await page.click(locale);
      const text = await page.waitFor(
        `document.body.innerText.toLowerCase().includes(${JSON.stringify(wanted[0].toLowerCase())}) && document.body.innerText`,
        { what: `${locale} strings` },
      );
      for (const w of wanted) check(text.toLowerCase().includes(w.toLowerCase()), `${locale}: "${w}" shown`);
      fs.writeFileSync(path.join(shots, `home-${locale.toLowerCase()}.png`), await page.screenshot());
    }

    const wizard = await page.evaluate(`document.querySelectorAll("main section.step").length`);
    check(wizard > 0, "install wizard shown on an empty data root");
    const tabs = await page.evaluate(
      `[...document.querySelectorAll("nav.tabs button")].map((b) => ({ text: b.textContent.trim(), disabled: b.disabled }))`,
    );
    check(tabs.length >= 2, "section tabs present");
    check(tabs.slice(1).every((t) => t.disabled), "tabs that need a realm are disabled");
    const overflow = await page.evaluate(`document.documentElement.scrollWidth > innerWidth`);
    check(!overflow, "no horizontal page scrollbar");
    const unnamed = await page.evaluate(
      `[...document.querySelectorAll("button")].filter((b) => !b.textContent.trim() && !b.getAttribute("aria-label") && !b.title).length`,
    );
    check(unnamed === 0, `every button has a name (${unnamed} without)`);
    check(errors.length === 0, `no script error${errors.length ? `: ${errors.join(" | ")}` : ""}`);
  } catch (error) {
    check(false, String(error?.message ?? error));
    if (page) fs.writeFileSync(path.join(shots, "failure.png"), await page.screenshot().catch(() => Buffer.alloc(0)));
  } finally {
    page?.close();
    app.kill();
    await new Promise((resolve) => setTimeout(resolve, 1000));
    fs.rmSync(root, { recursive: true, force: true });
  }
  if (failures.length) {
    console.error(`${failures.length} UI check(s) failed`);
    process.exit(1);
  }
  console.log("UI smoke test passed");
}
