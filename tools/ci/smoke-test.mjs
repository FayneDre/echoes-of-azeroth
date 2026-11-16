#!/usr/bin/env node
// End-to-end smoke test of a server artifact, as the launcher will use it:
// portable MySQL 8.4 + packaged binaries/SQL + upstream server data.
//
// Snapshot (ADR 0009): dbimport from empty databases with the base module set
//   (mandatory + `applyWhenDisabled`), slow clean shutdown, then a copy of the
//   data directory without server identity, keys, temporary tablespaces or
//   spare redo files, plus `snapshot.json`. No server has run on it.
// The rounds then run on a *restore* of that snapshot, done as the launcher
// does it (copy, first start with an init file that sets the root password):
// Round A ("modules off"): base set allowed in dbimport (already up to date),
//   optional modules compiled in but disabled -> servers must start; optional
//   module SQL must NOT be applied (validates Updates.AllowedModules naming +
//   inertness).
// Round B ("modules on"): all modules allowed and enabled -> servers must start;
//   optional module SQL must be applied on top of the existing databases.
//
// Usage: node tools/ci/smoke-test.mjs --staging <dir> --mysql <dir> --data <dir> --work <dir>
//        [--snapshot <dir>] [--mysql-manifest <component.json>]
//        [--mandatory mod-a,mod-b] [--optional mod-c,mod-d] [--timeout-min 25]

import { spawn, spawnSync } from "node:child_process";
import { randomBytes } from "node:crypto";
import { appendFileSync, copyFileSync, cpSync, createWriteStream, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { createConnection } from "node:net";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// ------------------------------------------------------------------ args
const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, value, index, all) => {
    if (value.startsWith("--")) pairs.push([value.slice(2), all[index + 1]]);
    return pairs;
  }, []),
);
for (const required of ["staging", "mysql", "data", "work"]) {
  if (!args[required]) {
    console.error(`missing --${required}`);
    process.exit(2);
  }
}
const staging = resolve(args.staging);
const mysqlRoot = resolve(args.mysql);
const work = resolve(args.work);
const mandatory = (args.mandatory ?? "mod-playerbots").split(",").filter(Boolean);
const optional = (args.optional ?? "mod-ah-bot,mod-individual-progression").split(",").filter(Boolean);
// Optional modules whose SQL is applied even when disabled (ADR 0008), read from the
// catalog here so the build plan (part of the build key) does not change.
const emulatorDir = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "catalog", "emulators", process.env.EMULATOR ?? "azerothcore");
const sqlAlways = optional.filter((id) => {
  const manifest = join(emulatorDir, "modules", `${id}.json`);
  return existsSync(manifest) && JSON.parse(readFileSync(manifest, "utf8")).database.applyWhenDisabled === true;
});
const timeoutMs = Number(args["timeout-min"] ?? 25) * 60_000;
const snapshotDir = resolve(args.snapshot ?? join(work, "db-snapshot"));
const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const mysqlManifest = JSON.parse(readFileSync(args["mysql-manifest"] ?? join(repoRoot, "catalog", "components", "mysql-8-4.json"), "utf8"));
const logs = join(work, "logs");
mkdirSync(logs, { recursive: true });

const summary = [];
const note = (line) => {
  console.log(line);
  summary.push(line);
};
// Synchronous cleanup actions, run in reverse order when the test fails
// (fail() exits immediately, so it cannot rely on the final try/finally).
const cleanups = [];
const fail = (message) => {
  note(`❌ ${message}`);
  for (const cleanup of cleanups.reverse()) {
    try {
      cleanup();
    } catch (error) {
      console.error(`cleanup failed: ${error}`);
    }
  }
  flushSummary();
  process.exit(1);
};
const flushSummary = () => {
  if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, summary.join("\n") + "\n");
};

// ------------------------------------------------------------------ helpers
const walk = (dir) =>
  readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    return statSync(path).isDirectory() ? walk(path) : [path];
  });
const findFile = (dir, name) => walk(dir).find((p) => basename(p).toLowerCase() === name.toLowerCase());
const findDir = (dir, name, depth = 3) => {
  if (depth < 0) return undefined;
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    if (!statSync(path).isDirectory()) continue;
    if (entry.toLowerCase() === name) return dir;
    const found = findDir(path, name, depth - 1);
    if (found) return found;
  }
  return undefined;
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const portOpen = (port) =>
  new Promise((done) => {
    const socket = createConnection({ host: "127.0.0.1", port });
    socket.once("connect", () => {
      socket.destroy();
      done(true);
    });
    socket.once("error", () => done(false));
  });

/** Sets `key = value` in an AzerothCore .conf file (replaces or appends). */
function setConf(file, values) {
  let text = readFileSync(file, "utf8");
  for (const [key, raw] of Object.entries(values)) {
    const value = typeof raw === "number" ? String(raw) : `"${raw}"`;
    const escaped = key.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const line = new RegExp(`^${escaped}\\s*=.*$`, "m");
    text = line.test(text) ? text.replace(line, `${key} = ${value}`) : `${text}\n${key} = ${value}\n`;
  }
  writeFileSync(file, text);
}

function startProcess(name, exe, argv, cwd) {
  const log = createWriteStream(join(logs, `${name}.log`), { flags: "a" });
  const child = spawn(exe, argv, { cwd, stdio: ["ignore", "pipe", "pipe"], windowsHide: true });
  child.stdout.pipe(log);
  child.stderr.pipe(log);
  child.exitCodeValue = null;
  child.on("exit", (code) => (child.exitCodeValue = code ?? -1));
  return child;
}

async function waitReady(name, child, port) {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    if (child.exitCodeValue !== null) fail(`${name} exited with code ${child.exitCodeValue} before being ready (see ${name}.log)`);
    if (await portOpen(port)) {
      note(`✅ ${name} ready on port ${port} after ${Math.round((Date.now() - started) / 1000)} s`);
      return;
    }
    await sleep(3000);
  }
  fail(`${name} not ready after ${timeoutMs / 60000} min`);
}

const runSync = (name, exe, argv, cwd) => {
  const result = spawnSync(exe, argv, { cwd, encoding: "utf8", maxBuffer: 256 * 1024 * 1024, windowsHide: true });
  writeFileSync(join(logs, `${name}.log`), (result.stdout ?? "") + (result.stderr ?? ""), { flag: "a" });
  return result;
};

// ------------------------------------------------------------------ locate
const dist = join(staging, "dist");
const exe = Object.fromEntries(
  ["worldserver.exe", "authserver.exe", "dbimport.exe"].map((n) => {
    const path = findFile(dist, n);
    if (!path) fail(`${n} not found in ${dist}`);
    return [n.replace(".exe", ""), path];
  }),
);
const mysqld = join(mysqlRoot, "bin", "mysqld.exe");
const mysqlCli = join(mysqlRoot, "bin", "mysql.exe");
const mysqladmin = join(mysqlRoot, "bin", "mysqladmin.exe");
if (!existsSync(mysqld)) fail(`mysqld.exe not found under ${mysqlRoot}`);
const dataDir = findDir(resolve(args.data), "dbc");
if (!dataDir) fail("server data (dbc/maps/...) not found");
note(`ℹ️ binaries: ${dirname(exe.worldserver)}`);
note(`ℹ️ server data: ${dataDir}`);

// ------------------------------------------------------------------ config
// On Windows AzerothCore resolves its config directory as "configs/" relative to
// the *working directory* (module configs: "configs/modules/"). Every server
// process therefore runs with cwd = instance root (here: `work`).
const conf = join(work, "configs");
mkdirSync(join(conf, "modules"), { recursive: true });
for (const dist of walk(staging).filter((p) => p.endsWith(".conf.dist"))) {
  const isModule = !/(worldserver|authserver|dbimport)\.conf\.dist$/.test(dist);
  const target = join(conf, isModule ? "modules" : "", basename(dist).replace(/\.dist$/, ""));
  copyFileSync(dist, target);
}
const confFile = (name) => join(conf, name);
for (const name of ["worldserver.conf", "authserver.conf", "dbimport.conf"]) {
  if (!existsSync(confFile(name))) fail(`${name}.dist not found in the artifact`);
}
const sourceDir = join(staging, "sql").replaceAll("\\", "/");
const common = { LogsDir: logs.replaceAll("\\", "/"), MySQLExecutable: mysqlCli.replaceAll("\\", "/"), SourceDirectory: sourceDir };
setConf(confFile("dbimport.conf"), { ...common });
// authserver defaults to migrating the auth database itself (Updates.EnableDatabases = 1)
// and would look for SQL in the build machine's source tree: only dbimport migrates (ADR 0007).
setConf(confFile("authserver.conf"), { ...common, "Updates.EnableDatabases": 0 });
setConf(confFile("worldserver.conf"), {
  ...common,
  DataDir: dataDir.replaceAll("\\", "/"),
  "Updates.EnableDatabases": 0, // the launcher runs dbimport itself (controls module SQL)
  "Console.Enable": 0,
});
const playerbotsConf = confFile("modules/playerbots.conf");
if (existsSync(playerbotsConf)) {
  // CI-only: keep startup fast. Never shipped as a user default.
  setConf(playerbotsConf, { "Playerbots.MinRandomBots": 10, "Playerbots.MaxRandomBots": 10 });
}
const moduleToggles = {
  "mod-ah-bot": ["mod_ahbot.conf", { "AuctionHouseBot.EnableSeller": 1, "AuctionHouseBot.EnableBuyer": 1 }, { "AuctionHouseBot.EnableSeller": 0, "AuctionHouseBot.EnableBuyer": 0 }],
  "mod-individual-progression": ["individualProgression.conf", { "IndividualProgression.Enable": 1 }, { "IndividualProgression.Enable": 0 }],
};
const toggle = (enabled) => {
  for (const id of optional) {
    const entry = moduleToggles[id];
    if (!entry) continue;
    const file = confFile(`modules/${entry[0]}`);
    if (existsSync(file)) setConf(file, enabled ? entry[1] : entry[2]);
    else note(`⚠️ ${entry[0]} not found for ${id}`);
  }
  if (optional.includes("mod-individual-progression")) {
    setConf(confFile("worldserver.conf"), enabled ? { EnablePlayerSettings: 1, "DBC.EnforceItemAttributes": 0 } : { EnablePlayerSettings: 0, "DBC.EnforceItemAttributes": 1 });
  }
};

// ------------------------------------------------------------------ mysql
// Same server options as the launcher's my.ini (crates/rk-db/src/settings.rs)
// where they matter for the data directory.
const mysqlPort = 3306;
const databases = ["acore_auth", "acore_characters", "acore_world", "acore_playerbots"];
const myIni = join(work, "my.ini");
const clientCnf = join(work, "client.cnf");
let datadir;
let mysql;
const slash = (p) => p.replaceAll("\\", "/");
const writeMyIni = () =>
  writeFileSync(
    myIni,
    [
      "[mysqld]",
      `basedir="${slash(mysqlRoot)}"`,
      `datadir="${slash(datadir)}"`,
      `log-error="${slash(join(logs, "mysqld.log"))}"`,
      `port=${mysqlPort}`,
      "bind-address=127.0.0.1",
      "mysqlx=OFF",
      "disable-log-bin",
      "character-set-server=utf8mb4",
      "collation-server=utf8mb4_unicode_ci",
      "innodb_buffer_pool_size=1024M",
      "innodb_redo_log_capacity=512M",
      "",
    ].join("\n"),
  );
const writeClientCnf = (password) =>
  writeFileSync(clientCnf, `[client]\nuser=root\npassword=${password}\nhost=127.0.0.1\nport=${mysqlPort}\n`);
// --defaults-extra-file must come first.
const mysqlArgs = (...rest) => [`--defaults-extra-file=${clientCnf}`, ...rest];

const sql = (statement, database) => {
  const argv = mysqlArgs("-N", "-B", "-e", statement);
  if (database) argv.push(database);
  const result = spawnSync(mysqlCli, argv, { encoding: "utf8", windowsHide: true });
  if (result.status !== 0) fail(`SQL failed: ${statement}\n${result.stderr}`);
  return result.stdout.trim();
};

async function startMysql(extra = []) {
  writeMyIni();
  mysql = startProcess("mysqld", mysqld, [`--defaults-file=${myIni}`, ...extra], work);
  await waitReady("mysqld", mysql, mysqlPort);
}

/** Clean shutdown; waits for the process to exit (the data directory is then consistent). */
async function stopMysql() {
  if (!mysql || mysql.exitCodeValue !== null) return;
  spawnSync(mysqladmin, mysqlArgs("shutdown"), { windowsHide: true });
  const started = Date.now();
  while (mysql.exitCodeValue === null) {
    if (Date.now() - started > 180_000) fail("mysqld did not stop within 180 s");
    await sleep(500);
  }
}
cleanups.push(() => {
  if (mysql && mysql.exitCodeValue === null) spawnSync(mysqladmin, mysqlArgs("shutdown"), { windowsHide: true });
});

/** Databases + the account of dbimport and the servers (the launcher's db-provision step). */
function provision() {
  for (const db of databases) {
    sql(`CREATE DATABASE IF NOT EXISTS ${db} DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci`);
  }
  for (const host of ["localhost", "127.0.0.1"]) {
    sql(`CREATE USER IF NOT EXISTS 'acore'@'${host}' IDENTIFIED BY 'acore'`);
    sql(`GRANT ALL PRIVILEGES ON *.* TO 'acore'@'${host}' WITH GRANT OPTION`);
  }
}

const tableExists = (db, table) => sql(`SHOW TABLES LIKE '${table}'`, db) === table;

/** Runs dbimport with `allowed` modules; checks module SQL is applied exactly for them. Returns seconds. */
function dbimport(label, allowed) {
  setConf(confFile("dbimport.conf"), { "Updates.AllowedModules": allowed.join(",") });
  const started = Date.now();
  const result = runSync(`dbimport-${label}`, exe.dbimport, ["-c", confFile("dbimport.conf")], work);
  if (result.status !== 0) fail(`dbimport (${label}) exited with ${result.status} (see dbimport-${label}.log)`);
  const seconds = Math.round((Date.now() - started) / 1000);
  note(`✅ dbimport (${label}) with AllowedModules="${allowed.join(",")}" in ${seconds} s`);
  if (optional.includes("mod-ah-bot")) {
    const applied = tableExists("acore_world", "mod_auctionhousebot");
    if (allowed.includes("mod-ah-bot") !== applied) fail(`mod-ah-bot SQL ${applied ? "applied" : "missing"} with AllowedModules="${allowed.join(",")}"`);
    note(`✅ mod-ah-bot SQL ${applied ? "applied" : "not applied"} as expected`);
  }
  return seconds;
}

// ------------------------------------------------------------------ snapshot
// Server identity, keys and files rebuilt at startup: never shipped (ADR 0009).
// `*_tmp` are the spare redo files (512 MB), recreated by InnoDB.
const excludedFromSnapshot = (name) =>
  name === "auto.cnf" || name === "#innodb_temp" || /\.(pem|pid|err)$/i.test(name) || /_tmp$/.test(name);
const baseModules = [...mandatory, ...sqlAlways];

async function makeSnapshot() {
  note("\n### Database snapshot");
  datadir = join(work, "mysql-build");
  writeMyIni();
  const init = runSync("mysql-init", mysqld, [`--defaults-file=${myIni}`, "--initialize-insecure", "--console"], work);
  if (init.status !== 0) fail("mysqld --initialize-insecure failed (see mysql-init.log)");
  writeClientCnf("");
  await startMysql();
  provision();
  toggle(false);
  dbimport("snapshot", baseModules);
  // Only MySQL's own accounts are shipped; root gets its password at restore.
  for (const host of ["localhost", "127.0.0.1"]) sql(`DROP USER IF EXISTS 'acore'@'${host}'`);
  // Slow shutdown: purge and change buffer merge done, nothing to recover.
  sql("SET GLOBAL innodb_fast_shutdown = 0");
  await stopMysql();

  rmSync(snapshotDir, { recursive: true, force: true });
  cpSync(datadir, join(snapshotDir, "data"), { recursive: true, filter: (src) => !excludedFromSnapshot(basename(src)) });
  const buildInfo = JSON.parse(readFileSync(join(staging, "build-info.json"), "utf8"));
  const manifest = {
    format: 1,
    buildKey: buildInfo.buildKey,
    mysql: { id: mysqlManifest.id, version: mysqlManifest.version, sha256: mysqlManifest.platforms["windows-x86_64"].sha256 },
    databases,
    modules: baseModules,
    createdAt: new Date().toISOString(),
  };
  writeFileSync(join(snapshotDir, "snapshot.json"), JSON.stringify(manifest, null, 2) + "\n");
  const size = walk(join(snapshotDir, "data")).reduce((sum, p) => sum + statSync(p).size, 0);
  note(`✅ snapshot with ${baseModules.join(",")}: ${(size / 1048576).toFixed(0)} MB unpacked`);
}

/** Restores the snapshot into a fresh data directory, as the launcher's db-adopt step. */
async function restoreSnapshot() {
  note("\n### Restore snapshot");
  datadir = join(work, "mysql-data");
  rmSync(datadir, { recursive: true, force: true });
  const started = Date.now();
  cpSync(join(snapshotDir, "data"), datadir, { recursive: true });
  const password = randomBytes(24).toString("hex");
  const initFile = join(work, "init.sql");
  writeFileSync(initFile, `ALTER USER 'root'@'localhost' IDENTIFIED BY '${password}';\n`);
  writeClientCnf(password);
  await startMysql([`--init-file=${initFile}`]);
  rmSync(initFile);
  provision();
  note(`✅ snapshot restored, MySQL ready in ${Math.round((Date.now() - started) / 1000)} s`);
}

// ------------------------------------------------------------------ rounds
const children = [];
cleanups.push(() => {
  for (const child of children) if (child.exitCodeValue === null) child.kill();
});
const stopServers = async () => {
  for (const child of children.splice(0)) if (child.exitCodeValue === null) child.kill();
  await sleep(3000);
};

async function round(label, allowed, enabled, maxImportSeconds = Infinity) {
  note(`\n### Round ${label}`);
  toggle(enabled);
  const seconds = dbimport(label, allowed);
  if (seconds > maxImportSeconds) fail(`dbimport (${label}) took ${seconds} s: the restored snapshot is not up to date`);

  const auth = startProcess(`authserver-${label}`, exe.authserver, ["-c", confFile("authserver.conf")], work);
  children.push(auth);
  await waitReady(`authserver (${label})`, auth, 3724);
  const world = startProcess(`worldserver-${label}`, exe.worldserver, ["-c", confFile("worldserver.conf")], work);
  children.push(world);
  await waitReady(`worldserver (${label})`, world, 8085);
  await sleep(20_000); // let bots and modules run for a moment
  if (world.exitCodeValue !== null) fail(`worldserver (${label}) crashed after startup (code ${world.exitCodeValue})`);
  note(`✅ worldserver (${label}) still running 20 s after readiness`);
  await stopServers();
}

try {
  await makeSnapshot();
  await restoreSnapshot();
  // On the restored snapshot, dbimport only checks: seconds, not minutes.
  await round("A-modules-off", baseModules, false, 120);
  await round("B-modules-on", [...mandatory, ...optional], true);
  note("\n✅ Smoke test passed");
} finally {
  await stopServers();
  await stopMysql();
  flushSummary();
}
