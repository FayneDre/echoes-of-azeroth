#!/usr/bin/env node
// Signed artifact index (ADR 0010): the CI side. The launcher side is
// crates/eoa-catalog/src/index.rs; both read crates/eoa-catalog/trusted-keys.json.
//
//   node tools/ci/index.mjs keygen --private-out <file.pem>
//       New Ed25519 key pair: the private key (PKCS#8 PEM) goes to the file,
//       the public key and its id are printed. Never commit the private key.
//   node tools/ci/index.mjs entry --build-info <build-info.json> --snapshot <snapshot.json>
//       --platform windows-x86_64 --server <server.zip> --server-url <url>
//       --db-snapshot <snapshot.zip> --db-snapshot-url <url> --out <entry.json>
//       One build entry: pins from build-info.json, sizes and sha256 of the files.
//   node tools/ci/index.mjs update [--previous <index.json>] --entry <entry.json> --out <index.json>
//       Verifies the previous index, adds (or replaces, same build key) the entry,
//       increments the serial and signs with $EOA_INDEX_SIGNING_KEY (PEM text).
//   node tools/ci/index.mjs verify <index.json>
//       Verifies against the trusted keys and prints the payload.

import { createHash, createPrivateKey, createPublicKey, generateKeyPairSync, sign, verify } from "node:crypto";
import { createReadStream, readFileSync, writeFileSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const DOMAIN = "echoes-of-azeroth/index/v1\n";
const here = dirname(fileURLToPath(import.meta.url));
const trustedKeysFile = join(here, "..", "..", "crates", "eoa-catalog", "trusted-keys.json");

const die = (message) => {
  console.error(`index.mjs: ${message}`);
  process.exit(1);
};
const options = (argv) => {
  const out = { _: [] };
  for (let i = 0; i < argv.length; i++) {
    if (argv[i].startsWith("--")) out[argv[i].slice(2)] = argv[++i];
    else out._.push(argv[i]);
  }
  return out;
};
const required = (opts, ...names) => {
  for (const name of names) if (opts[name] === undefined) die(`missing --${name}`);
};
const readJson = (path) => JSON.parse(readFileSync(path, "utf8"));

/** Raw 32-byte public key (hex) of a KeyObject. */
const rawPublicKey = (key) => Buffer.from(createPublicKey(key).export({ format: "jwk" }).x, "base64url").toString("hex");
export const keyId = (publicKeyHex) => createHash("sha256").update(Buffer.from(publicKeyHex, "hex")).digest("hex").slice(0, 16);
const publicKeyFromRaw = (hex) =>
  createPublicKey({ key: { kty: "OKP", crv: "Ed25519", x: Buffer.from(hex, "hex").toString("base64url") }, format: "jwk" });

/** Envelope for `payload` (a JSON text), signed with a private KeyObject. */
export function seal(payload, privateKey) {
  const signature = sign(null, Buffer.from(DOMAIN + payload, "utf8"), privateKey).toString("hex");
  return { format: 1, keyId: keyId(rawPublicKey(privateKey)), signature, payload };
}

/** Verified payload object of an envelope, or an exception. */
export function open(envelope, trusted = readJson(trustedKeysFile).keys) {
  if (envelope.format !== 1) throw new Error(`unsupported envelope format ${envelope.format}`);
  const key = trusted.find((k) => keyId(k.publicKey) === envelope.keyId);
  if (!key) throw new Error(`index signed with an unknown key ${envelope.keyId}`);
  const ok = verify(null, Buffer.from(DOMAIN + envelope.payload, "utf8"), publicKeyFromRaw(key.publicKey), Buffer.from(envelope.signature, "hex"));
  if (!ok) throw new Error("invalid index signature");
  const index = JSON.parse(envelope.payload);
  if (index.format !== 1) throw new Error(`unsupported index format ${index.format}`);
  return index;
}

const fileFacts = (path) =>
  new Promise((done, reject) => {
    const hash = createHash("sha256");
    let size = 0;
    createReadStream(path)
      .on("data", (chunk) => {
        size += chunk.length;
        hash.update(chunk);
      })
      .on("error", reject)
      .on("end", () => done({ size, sha256: hash.digest("hex") }));
  });

async function entry(opts) {
  required(opts, "build-info", "snapshot", "platform", "server", "server-url", "db-snapshot", "db-snapshot-url", "out");
  const info = readJson(opts["build-info"]);
  const snapshot = readJson(opts.snapshot);
  if (snapshot.buildKey !== info.buildKey) die(`snapshot is for build ${snapshot.buildKey}, server is ${info.buildKey}`);
  const build = {
    emulator: info.emulator,
    flavor: info.flavor,
    platform: opts.platform,
    buildKey: info.buildKey,
    core: { repo: info.core.repo, commit: info.core.commit },
    modules: Object.fromEntries([...info.modules].sort((a, b) => a.id.localeCompare(b.id)).map((m) => [m.id, m.commit])),
    server: { urls: [opts["server-url"]], ...(await fileFacts(opts.server)) },
    dbSnapshot: {
      urls: [opts["db-snapshot-url"]],
      ...(await fileFacts(opts["db-snapshot"])),
      mysql: { id: snapshot.mysql.id, version: snapshot.mysql.version },
      modules: snapshot.modules,
    },
    builtAt: info.builtAt ?? null,
    source: info.ci ?? null,
  };
  writeFileSync(opts.out, JSON.stringify(build, null, 2) + "\n");
  console.log(`entry ${build.emulator}/${build.flavor} ${build.buildKey}: server ${build.server.size} B, snapshot ${build.dbSnapshot.size} B`);
}

function update(opts) {
  required(opts, "entry", "out");
  const pem = process.env.EOA_INDEX_SIGNING_KEY;
  if (!pem) die("EOA_INDEX_SIGNING_KEY is not set");
  const privateKey = createPrivateKey(pem);
  const trusted = readJson(trustedKeysFile).keys;
  if (!trusted.some((k) => k.publicKey === rawPublicKey(privateKey))) {
    die(`the signing key ${keyId(rawPublicKey(privateKey))} is not in trusted-keys.json: launchers would reject the index`);
  }
  const previous = opts.previous ? open(readJson(opts.previous), trusted) : { format: 1, serial: 0, builds: [] };
  const build = readJson(opts.entry);
  const same = (b) => b.emulator === build.emulator && b.flavor === build.flavor && b.platform === build.platform && b.buildKey === build.buildKey;
  // Replaced entries move to the end: the last listed match is the newest.
  const builds = [...previous.builds.filter((b) => !same(b)), build];
  const index = { format: 1, serial: previous.serial + 1, published: new Date().toISOString(), builds };
  writeFileSync(opts.out, JSON.stringify(seal(JSON.stringify(index, null, 2), privateKey), null, 2) + "\n");
  console.log(`index serial ${index.serial}: ${builds.length} build(s), signed with ${keyId(rawPublicKey(privateKey))}`);
}

function keygen(opts) {
  required(opts, "private-out");
  const { privateKey } = generateKeyPairSync("ed25519");
  writeFileSync(opts["private-out"], privateKey.export({ format: "pem", type: "pkcs8" }), { mode: 0o600, flag: "wx" });
  const publicKey = rawPublicKey(privateKey);
  console.log(JSON.stringify({ publicKey, keyId: keyId(publicKey) }, null, 2));
}

const isMain = process.argv[1] && basename(process.argv[1]) === basename(fileURLToPath(import.meta.url));
if (isMain) {
  const [command, ...rest] = process.argv.slice(2);
  const opts = options(rest);
  try {
    if (command === "keygen") keygen(opts);
    else if (command === "entry") await entry(opts);
    else if (command === "update") update(opts);
    else if (command === "verify") console.log(JSON.stringify(open(readJson(opts._[0] ?? die("missing <index.json>"))), null, 2));
    else die("usage: index.mjs keygen|entry|update|verify (see the header)");
  } catch (error) {
    die(error.message);
  }
}
