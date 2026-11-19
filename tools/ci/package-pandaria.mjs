// Assembles the Mists of Pandaria prototype artifact (ADR 0024) from a
// finished LoA-Pandaria-Playerbots build:
//
//   <out>/
//     dist/                   servers, map extractors, runtime DLLs (the instance's bin/)
//     dist/configs/*.conf.dist   configuration templates, playerbots.conf.dist included
//     sql/sql/**              core SQL the worldserver's updater reads (SourceDirectory)
//     sql/modules/<dir>/data/sql/**   module SQL (playerbots database updates)
//     build-info.json         source commit, toolchain
//     files.json              every file with size and sha256
//
// Same layout as the AzerothCore artifact (package-server.mjs): the engine
// installs both the same way.
//
// Usage: node tools/ci/package-pandaria.mjs --source <dir> --bin <dir>
//          --mysql <dir> --openssl <dir> --repo <url> --commit <sha> --out <dir>

import { createHash } from "node:crypto";
import { copyFileSync, cpSync, existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { basename, join, relative, sep } from "node:path";
import { parseArgs } from "node:util";

const { values: opts } = parseArgs({
  options: Object.fromEntries(
    ["source", "bin", "mysql", "openssl", "repo", "commit", "out"].map((name) => [name, { type: "string" }]),
  ),
});
for (const [name, value] of Object.entries(opts)) if (!value) fail(`--${name} is required`);
if (!/^[0-9a-f]{40}$/.test(opts.commit)) fail("--commit must be a full 40-character sha");

function fail(message) {
  console.error(`package-pandaria: ${message}`);
  process.exit(1);
}

const walk = (dir) =>
  readdirSync(dir)
    .sort()
    .flatMap((name) => {
      const path = join(dir, name);
      return statSync(path).isDirectory() ? walk(path) : [path];
    });
const posix = (path) => path.split(sep).join("/");

// ------------------------------------------------------------------ dist
const dist = join(opts.out, "dist");
const configs = join(dist, "configs");
mkdirSync(configs, { recursive: true });
for (const path of walk(opts.bin)) {
  const name = basename(path);
  if (/\.(exe|dll)$/i.test(name)) copyFileSync(path, join(dist, name));
  else if (name.endsWith(".conf.dist")) copyFileSync(path, join(configs, name));
}
// Configuration templates the build did not copy next to the executables
// (module ones always stay in the source tree). playerbots.conf is read
// next to worldserver.conf.
for (const path of ["src/server", "modules"].flatMap((dir) => walk(join(opts.source, dir)))) {
  const name = basename(path);
  if (name.endsWith(".conf.dist") && !existsSync(join(configs, name))) {
    copyFileSync(path, join(configs, name));
    console.log(`added ${name} from ${posix(relative(opts.source, path))}`);
  }
}

// Runtime DLLs the servers load from their own directory. legacy.dll is the
// OpenSSL 3 legacy provider (OpenSSLCrypto.cpp loads it from the executable
// directory on Windows).
const runtime = {
  "libmysql.dll": [join(opts.mysql, "lib", "libmysql.dll")],
  "libssl-3-x64.dll": [join(opts.openssl, "bin", "libssl-3-x64.dll")],
  "libcrypto-3-x64.dll": [join(opts.openssl, "bin", "libcrypto-3-x64.dll")],
  "legacy.dll": [
    join(opts.openssl, "bin", "legacy.dll"),
    join(opts.openssl, "lib", "ossl-modules", "legacy.dll"),
    join(opts.openssl, "bin", "ossl-modules", "legacy.dll"),
  ],
};
for (const [name, candidates] of Object.entries(runtime)) {
  if (existsSync(join(dist, name))) continue;
  const found = candidates.find((c) => existsSync(c));
  if (!found) fail(`runtime library ${name} not found (looked in: ${candidates.join(", ")})`);
  copyFileSync(found, join(dist, name));
  console.log(`added ${name} from ${posix(found)}`);
}

const required = [
  "worldserver.exe",
  "authserver.exe",
  "mapextractor.exe",
  "vmap4extractor.exe",
  "vmap4assembler.exe",
  "mmaps_generator.exe",
  ...Object.keys(runtime),
  "configs/worldserver.conf.dist",
  "configs/authserver.conf.dist",
  "configs/playerbots.conf.dist",
];
const missing = required.filter((name) => !existsSync(join(dist, name)));
if (missing.length) fail(`missing in the build output: ${missing.join(", ")}`);

// ----------------------------------------------------------- SQL sources
const sql = join(opts.out, "sql");
cpSync(join(opts.source, "sql"), join(sql, "sql"), { recursive: true });
for (const dir of readdirSync(join(opts.source, "modules")).sort()) {
  const moduleSql = join(opts.source, "modules", dir, "data", "sql");
  if (existsSync(moduleSql)) {
    cpSync(moduleSql, join(sql, "modules", dir, "data", "sql"), { recursive: true });
    console.log(`module SQL: ${dir}`);
  }
}

// ------------------------------------------------------------- manifests
writeFileSync(
  join(opts.out, "build-info.json"),
  JSON.stringify(
    {
      emulator: "pandaria",
      flavor: "playerbot",
      core: { repo: opts.repo, commit: opts.commit },
      toolchain: {
        boost: process.env.BOOST_VERSION ?? null,
        openssl: process.env.OPENSSL_VERSION ?? null,
        mysqlClient: posix(opts.mysql),
      },
      builtAt: new Date().toISOString(),
      ci: process.env.GITHUB_RUN_ID
        ? { repository: process.env.GITHUB_REPOSITORY, runId: process.env.GITHUB_RUN_ID, sha: process.env.GITHUB_SHA }
        : null,
    },
    null,
    2,
  ) + "\n",
);

const files = walk(opts.out)
  .map((path) => {
    const data = readFileSync(path);
    return { path: posix(relative(opts.out, path)), size: data.length, sha256: createHash("sha256").update(data).digest("hex") };
  })
  .filter((f) => f.path !== "files.json");
writeFileSync(join(opts.out, "files.json"), JSON.stringify({ commit: opts.commit, files }, null, 2) + "\n");

const total = files.reduce((sum, f) => sum + f.size, 0);
console.log(`packaged ${files.length} files, ${(total / 1048576).toFixed(1)} MB`);
for (const name of readdirSync(dist).sort()) console.log(`  dist/${name}`);
for (const name of readdirSync(configs).sort()) console.log(`  dist/configs/${name}`);
