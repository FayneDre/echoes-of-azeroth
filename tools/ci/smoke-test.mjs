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
//   optional module SQL must be applied on top of the existing databases. Then,
//   as a player would see it: a real logon on authserver (SRP6 challenge,
//   proof, realm list naming the world port, realm online), random bots in
//   the world, and AHBot auctions in the auction house.
//
// Usage: node tools/ci/smoke-test.mjs --staging <dir> --mysql <dir> --data <dir> --work <dir>
//        [--snapshot <dir>] [--mysql-manifest <component.json>]
//        [--mandatory mod-a,mod-b] [--optional mod-c,mod-d] [--timeout-min 25]
//        [--live-min 10]

import { spawn, spawnSync } from "node:child_process";
import { randomBytes } from "node:crypto";
import { appendFileSync, copyFileSync, cpSync, createWriteStream, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { createConnection } from "node:net";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { login, registration, selfTest } from "./auth-client.mjs";

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
// How long bots and AHBot get to show up once the world is ready.
const liveMs = Number(args["live-min"] ?? 10) * 60_000;
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
// Each optional module's switch, as its catalog manifest declares it
// (`activation.enable` / `disable`: config file -> values).
const activation = (id) => {
  const manifest = join(emulatorDir, "modules", `${id}.json`);
  return existsSync(manifest) ? JSON.parse(readFileSync(manifest, "utf8")).activation : null;
};
const toggle = (enabled) => {
  for (const id of optional) {
    const values = activation(id)?.[enabled ? "enable" : "disable"];
    if (!values) continue;
    for (const [conf, settings] of Object.entries(values)) {
      const file = confFile(conf);
      if (existsSync(file)) setConf(file, settings);
      else fail(`${conf} not found for ${id}: the module config is not packaged`);
    }
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

// ------------------------------------------------------------------ players
const SMOKE_USER = "RKSMOKE";
const SMOKE_PASS = randomBytes(8).toString("hex");
const hex = (buffer) => buffer.toString("hex").toUpperCase();

/** An account as the launcher's driver writes it (AccountMgr::CreateAccount). Returns its id. */
function createAccount(username, password) {
  const { salt, verifier } = registration(username, password);
  return Number(
    sql(
      `INSERT INTO account (username, salt, verifier, expansion, reg_mail, email, joindate) VALUES ('${username}', X'${hex(salt)}', X'${hex(verifier)}', 2, '', '', NOW()); ` +
        "SET @id = LAST_INSERT_ID(); " +
        "INSERT INTO realmcharacters (realmid, acctid, numchars) SELECT realmlist.id, @id, 0 FROM realmlist; " +
        "SELECT @id;",
      "acore_auth",
    ),
  );
}

/** The AHBot owner (module post-install steps): an account, one character, the config naming them. */
function prepareAhBot() {
  const account = createAccount("AHBOT", randomBytes(8).toString("hex"));
  const guid = Number(
    sql(
      "SELECT COALESCE(MAX(guid), 0) + 1 INTO @guid FROM characters; " +
        "INSERT INTO characters (guid, account, name, race, class, gender, level, money, map, zone, position_x, position_y, position_z, orientation, taximask, innTriggerId, at_login) " +
        `VALUES (@guid, ${account}, 'Auctioneer', 1, 1, 0, 1, 0, 0, 12, -8949.95, -132.493, 83.5312, 0, '', 0, 0); ` +
        "SELECT @guid;",
      "acore_characters",
    ),
  );
  setConf(confFile("modules/mod_ahbot.conf"), { "AuctionHouseBot.Account": account, "AuctionHouseBot.GUID": guid });
  note(`✅ AHBot owner: account ${account}, character ${guid}`);
  return guid;
}

/** A real logon, then the realm list: the realm must point at the world port and be online. */
async function checkLogin(label) {
  const started = Date.now();
  let last;
  while (Date.now() - started < 120_000) {
    try {
      const realms = await login({ port: 3724, user: SMOKE_USER, pass: SMOKE_PASS });
      last = JSON.stringify(realms);
      const realm = realms.find((r) => r.address.endsWith(":8085"));
      if (realm && !realm.offline) {
        note(`✅ logon (${label}): SRP6 accepted, realm "${realm.name}" online at ${realm.address}`);
        return;
      }
    } catch (error) {
      last = error.message;
    }
    // The authserver refreshes the realm states every RealmsStateUpdateDelay (20 s).
    await sleep(5000);
  }
  fail(`logon (${label}) failed: ${last}`);
}

/** Waits until bots are in the world and (if AHBot runs) auctions are posted. */
async function checkLive(label, world, ahbotGuid) {
  const started = Date.now();
  let bots = 0;
  let auctions = 0;
  while (Date.now() - started < liveMs) {
    if (world.exitCodeValue !== null) fail(`worldserver (${label}) exited with code ${world.exitCodeValue} while running (see worldserver-${label}.log)`);
    bots = Number(
      sql(
        "SELECT COUNT(*) FROM acore_characters.characters c JOIN acore_auth.account a ON a.id = c.account WHERE c.online <> 0 AND a.username LIKE 'RNDBOT%'",
      ),
    );
    if (ahbotGuid) auctions = Number(sql(`SELECT COUNT(*) FROM acore_characters.auctionhouse WHERE itemowner = ${ahbotGuid}`));
    if (bots > 0 && (!ahbotGuid || auctions > 0)) break;
    await sleep(10_000);
  }
  const after = Math.round((Date.now() - started) / 1000);
  if (bots === 0) fail(`no random bot in the world ${after} s after readiness (${label})`);
  note(`✅ ${bots} random bots in the world after ${after} s (${label})`);
  if (ahbotGuid) {
    if (auctions === 0) fail(`AHBot posted no auction ${after} s after readiness (${label})`);
    note(`✅ AHBot posted ${auctions} auctions (${label})`);
  }
}

async function round(label, allowed, enabled, maxImportSeconds = Infinity, live = false) {
  note(`\n### Round ${label}`);
  toggle(enabled);
  const seconds = dbimport(label, allowed);
  if (seconds > maxImportSeconds) fail(`dbimport (${label}) took ${seconds} s: the restored snapshot is not up to date`);
  let ahbotGuid = null;
  if (live) {
    createAccount(SMOKE_USER, SMOKE_PASS);
    if (enabled && optional.includes("mod-ah-bot")) ahbotGuid = prepareAhBot();
  }

  const auth = startProcess(`authserver-${label}`, exe.authserver, ["-c", confFile("authserver.conf")], work);
  children.push(auth);
  await waitReady(`authserver (${label})`, auth, 3724);
  const world = startProcess(`worldserver-${label}`, exe.worldserver, ["-c", confFile("worldserver.conf")], work);
  children.push(world);
  await waitReady(`worldserver (${label})`, world, 8085);
  if (live) {
    await checkLogin(label);
    await checkLive(label, world, ahbotGuid);
  } else {
    await sleep(20_000); // let bots and modules run for a moment
  }
  if (world.exitCodeValue !== null) fail(`worldserver (${label}) crashed after startup (code ${world.exitCodeValue})`);
  note(`✅ worldserver (${label}) still running after the checks`);
  // ADR 0025: the party module registered its commands (it says so at startup).
  const worldLog = readFileSync(join(logs, `worldserver-${label}.log`), "utf8");
  if (!worldLog.includes("RKPARTY module ready")) fail(`mod-realmkeeper did not load in worldserver (${label}): no "RKPARTY module ready" line`);
  note(`✅ party commands of mod-realmkeeper loaded (${label})`);
  await stopServers();
}

try {
  selfTest();
  await makeSnapshot();
  await restoreSnapshot();
  // On the restored snapshot, dbimport only checks: seconds, not minutes.
  await round("A-modules-off", baseModules, false, 120);
  await round("B-modules-on", [...mandatory, ...optional], true, Infinity, true);
  note("\n✅ Smoke test passed");
} finally {
  await stopServers();
  await stopMysql();
  flushSummary();
}
