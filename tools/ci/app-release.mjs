#!/usr/bin/env node
// Launcher release helpers (ADR 0015), used by .github/workflows/release-app.yml.
//
//   config  --out <file> [--pubkey <minisign public key>] [--feed <url>]
//       Tauri config override for the release build. With a public key, the
//       build makes updater artifacts and the app checks the signed feed;
//       without one, the release has no auto-update (the app hides it).
//
//   feed    --version <v> --installer-url <url> --signature <file.sig> --out <latest.json>
//       The update feed read by tauri-plugin-updater (static JSON format).
//       Release notes come from app/src/lib/release-notes.json: `notesByLocale`
//       for the launcher, `notes` (English, one change per line) for 0.3.x.
//
//   notes   --version <v>
//       Prints the GitHub release body (English, then French). Fails when the
//       version has no entry in release-notes.json.
//
//   version
//       Prints the app version from app/src-tauri/tauri.conf.json.

import { readFileSync, writeFileSync } from "node:fs";

const [command, ...rest] = process.argv.slice(2);
const options = {};
for (let i = 0; i < rest.length; i += 2) {
  if (!rest[i].startsWith("--")) usage(`unexpected ${rest[i]}`);
  options[rest[i].slice(2)] = rest[i + 1];
}

function usage(message) {
  console.error(`app-release.mjs: ${message}`);
  console.error("usage: app-release.mjs config|feed|notes|version [options] (see the header)");
  process.exit(2);
}

/** The entry of release-notes.json for `version` (required). */
function releaseNotes(version) {
  const all = JSON.parse(readFileSync(new URL("../../app/src/lib/release-notes.json", import.meta.url), "utf8"));
  const notes = all[version];
  if (!notes?.en?.length || !notes?.fr?.length) {
    console.error(`app-release.mjs: app/src/lib/release-notes.json has no English and French notes for ${version}`);
    process.exit(1);
  }
  return notes;
}

function need(name) {
  const value = options[name];
  if (!value) usage(`missing --${name}`);
  return value;
}

switch (command) {
  case "config": {
    const config = { bundle: {} };
    if (options.pubkey) {
      config.bundle.createUpdaterArtifacts = true;
      config.plugins = {
        updater: {
          pubkey: options.pubkey,
          endpoints: [need("feed")],
          windows: { installMode: "passive" },
        },
      };
    }
    writeFileSync(need("out"), JSON.stringify(config, null, 2));
    console.log(options.pubkey ? "release config with the update feed" : "release config without auto-update");
    break;
  }
  case "feed": {
    const signature = readFileSync(need("signature"), "utf8").trim();
    const notes = releaseNotes(need("version"));
    const feed = {
      version: need("version"),
      notes: notes.en.map((line) => `• ${line}`).join("\n"),
      notesByLocale: notes,
      pub_date: new Date().toISOString(),
      platforms: {
        "windows-x86_64": { signature, url: need("installer-url") },
      },
    };
    writeFileSync(need("out"), JSON.stringify(feed, null, 2));
    console.log(`feed for ${feed.version}`);
    break;
  }
  case "notes": {
    const notes = releaseNotes(need("version"));
    const list = (lines) => lines.map((line) => `- ${line}`).join("\n");
    console.log(`## What's new\n\n${list(notes.en)}\n\n## Nouveautés\n\n${list(notes.fr)}`);
    break;
  }
  case "version": {
    const conf = JSON.parse(readFileSync(new URL("../../app/src-tauri/tauri.conf.json", import.meta.url), "utf8"));
    console.log(conf.version);
    break;
  }
  default:
    usage(`unknown command ${command ?? "(none)"}`);
}
