#!/usr/bin/env node
// Minimal 3.3.5a login client for the smoke test: SRP6 registration data as
// AzerothCore computes it, then a real logon against authserver (challenge,
// proof, realm list), as the game client does it. Follows the fork's
// `SRP6.cpp` and `AuthSession.cpp` (SHA-1, g = 7, every number
// little-endian; same as drivers/azerothcore/src/srp6.rs).
//
// Usage: node tools/ci/auth-client.mjs --self-test
//        node tools/ci/auth-client.mjs --login <user> <password> [--port 3724]

import { createHash, randomBytes } from "node:crypto";
import { createConnection } from "node:net";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const N_HEX = "894B645E89E1535BBDAD5B8B290650530801B18EBFBF5E8FAB3C82872A3E9BB7";
const N = BigInt(`0x${N_HEX}`);
const g = 7n;
const k = 3n;
const BUILD = 12340;

const sha1 = (...parts) => {
  const hash = createHash("sha1");
  for (const part of parts) hash.update(part);
  return hash.digest();
};
/** Little-endian bytes → number. */
const fromLE = (bytes) => BigInt(`0x${Buffer.from(bytes).reverse().toString("hex") || "0"}`);
/** Number → `length` little-endian bytes. */
const toLE = (value, length) => {
  const hex = value.toString(16).padStart(length * 2, "0");
  return Buffer.from(hex, "hex").reverse();
};
const modPow = (base, exponent, modulus) => {
  let result = 1n;
  base %= modulus;
  while (exponent > 0n) {
    if (exponent & 1n) result = (result * base) % modulus;
    base = (base * base) % modulus;
    exponent >>= 1n;
  }
  return result;
};
const mod = (a, m) => ((a % m) + m) % m;

const x = (salt, user, pass) => fromLE(sha1(salt, sha1(`${user.toUpperCase()}:${pass.toUpperCase()}`)));

/** Salt and verifier of a new account (`account.salt`, `account.verifier`). */
export function registration(user, pass, salt = randomBytes(32)) {
  return { salt, verifier: toLE(modPow(g, x(salt, user, pass), N), 32) };
}

/** `SRP6::SHA1Interleave`: the 40-byte session key from S (32 bytes, LE). */
function interleave(S) {
  let p = 0;
  while (p < 32 && S[p] === 0) p++;
  if (p & 1) p++;
  p /= 2;
  const even = [];
  const odd = [];
  for (let i = 0; i < 16; i++) {
    even.push(S[2 * i]);
    odd.push(S[2 * i + 1]);
  }
  const h0 = sha1(Buffer.from(even.slice(p)));
  const h1 = sha1(Buffer.from(odd.slice(p)));
  const K = Buffer.alloc(40);
  for (let i = 0; i < 20; i++) {
    K[2 * i] = h0[i];
    K[2 * i + 1] = h1[i];
  }
  return K;
}

const NgHash = () => {
  const gHash = sha1(Buffer.from([7]));
  return Buffer.from(sha1(toLE(N, 32)).map((byte, i) => byte ^ gHash[i]));
};

/** Client side of SRP6: A, the proof M1 and the session key K. */
function clientProof(I, pass, salt, B) {
  const a = fromLE(randomBytes(19));
  const A = toLE(modPow(g, a, N), 32);
  const u = fromLE(sha1(A, B));
  const xv = x(salt, I, pass);
  const S = toLE(modPow(mod(fromLE(B) - k * modPow(g, xv, N), N), a + u * xv, N), 32);
  const K = interleave(S);
  const M1 = sha1(NgHash(), sha1(I), salt, A, B, K);
  return { A, M1, K };
}

/** Reads a TCP stream as "give me n bytes". */
function reader(socket) {
  let buffer = Buffer.alloc(0);
  let waiting = null;
  let failure = null;
  const pump = () => {
    if (!waiting) return;
    if (failure) {
      const { reject } = waiting;
      waiting = null;
      reject(failure);
    } else if (buffer.length >= waiting.n) {
      const { n, resolve } = waiting;
      waiting = null;
      const out = buffer.subarray(0, n);
      buffer = buffer.subarray(n);
      resolve(out);
    }
  };
  socket.on("data", (chunk) => {
    buffer = Buffer.concat([buffer, chunk]);
    pump();
  });
  const end = (error) => {
    failure ??= error ?? new Error("authserver closed the connection");
    pump();
  };
  socket.on("error", end);
  socket.on("close", () => end());
  return (n) =>
    new Promise((resolve, reject) => {
      waiting = { n, resolve, reject };
      pump();
    });
}

const cstring = (buffer, offset) => {
  const end = buffer.indexOf(0, offset);
  return [buffer.subarray(offset, end).toString("utf8"), end + 1];
};

/**
 * Logs `user` in on authserver and returns the realm list: `[{ name,
 * address, flags, characters }]`. Throws with the authserver's result code
 * when the logon is refused.
 */
export async function login({ host = "127.0.0.1", port = 3724, user, pass, timeoutMs = 30_000 }) {
  const I = user.toUpperCase();
  const socket = createConnection({ host, port });
  socket.setTimeout(timeoutMs, () => socket.destroy(new Error(`no answer from ${host}:${port} in ${timeoutMs} ms`)));
  const read = reader(socket);
  try {
    await new Promise((resolve, reject) => {
      socket.once("connect", resolve);
      socket.once("error", reject);
    });

    // AUTH_LOGON_CHALLENGE (client): 30 bytes after `size`, then the name.
    const name = Buffer.from(I, "ascii");
    const challenge = Buffer.alloc(34 + name.length);
    let o = 0;
    o = challenge.writeUInt8(0x00, o); // cmd
    o = challenge.writeUInt8(0x08, o); // protocol
    o = challenge.writeUInt16LE(30 + name.length, o);
    o += challenge.write("WoW\0", o, "ascii");
    o = challenge.writeUInt8(3, o);
    o = challenge.writeUInt8(3, o);
    o = challenge.writeUInt8(5, o);
    o = challenge.writeUInt16LE(BUILD, o);
    o += challenge.write("68x\0", o, "ascii"); // "x86", reversed
    o += challenge.write("niW\0", o, "ascii"); // "Win"
    o += challenge.write("SUne", o, "ascii"); // "enUS"
    o = challenge.writeUInt32LE(0, o); // timezone bias
    o = challenge.writeUInt32LE(0x0100007f, o); // 127.0.0.1
    o = challenge.writeUInt8(name.length, o);
    name.copy(challenge, o);
    socket.write(challenge);

    const head = await read(3);
    if (head[0] !== 0x00) throw new Error(`unexpected answer 0x${head[0].toString(16)} to the logon challenge`);
    if (head[2] !== 0x00) throw new Error(`logon challenge refused (result 0x${head[2].toString(16)})`);
    const body = await read(32 + 1 + 1 + 1 + 32 + 32 + 16 + 1);
    const B = body.subarray(0, 32);
    const gLen = body[32];
    const nLen = body[34];
    if (gLen !== 1 || body[33] !== 7 || nLen !== 32) throw new Error("unexpected SRP6 parameters");
    const Nbytes = body.subarray(35, 67);
    if (fromLE(Nbytes) !== N) throw new Error("unexpected SRP6 modulus");
    const salt = body.subarray(67, 99);
    const securityFlags = body[115];
    if (securityFlags !== 0) throw new Error(`account needs a PIN or token (flags ${securityFlags})`);

    const { A, M1, K } = clientProof(I, pass, salt, B);

    // AUTH_LOGON_PROOF (client).
    socket.write(Buffer.concat([Buffer.from([0x01]), A, M1, Buffer.alloc(20), Buffer.from([0, 0])]));
    const proofHead = await read(2);
    if (proofHead[0] !== 0x01) throw new Error(`unexpected answer 0x${proofHead[0].toString(16)} to the logon proof`);
    if (proofHead[1] !== 0x00) throw new Error(`wrong password or refused logon (result 0x${proofHead[1].toString(16)})`);
    const proof = await read(30);
    const M2 = sha1(A, M1, K);
    if (!proof.subarray(0, 20).equals(M2)) throw new Error("the server proof does not match: SRP6 mismatch");

    // REALM_LIST.
    socket.write(Buffer.from([0x10, 0, 0, 0, 0]));
    const listHead = await read(3);
    if (listHead[0] !== 0x10) throw new Error(`unexpected answer 0x${listHead[0].toString(16)} to the realm list`);
    const list = await read(listHead.readUInt16LE(1));
    const count = list.readUInt16LE(4);
    const realms = [];
    let p = 6;
    for (let i = 0; i < count; i++) {
      const type = list[p];
      const flags = list[p + 2];
      p += 3;
      let realmName;
      let address;
      [realmName, p] = cstring(list, p);
      [address, p] = cstring(list, p);
      p += 4; // population
      const characters = list[p];
      p += 3; // characters, timezone, id
      if (flags & 0x04) p += 5; // build given
      realms.push({ name: realmName, address, type, flags, characters, offline: (flags & 0x02) !== 0 });
    }
    return realms;
  } finally {
    socket.destroy();
  }
}

/** Known vector: an account made by the fork's worldserver (srp6.rs). */
export function selfTest() {
  const salt = Buffer.from("7BEC1B5CBB5C03EA421E3AC81E3788D094AEB78D8FB774CBF661A175E809B65F", "hex");
  const { verifier } = registration("srptest", "testpass", salt);
  const expected = "B21FBEDD867E8D2E3A78346D66C622DB766769EC6D13DEC4A7EA323D3C6B0242";
  if (verifier.toString("hex").toUpperCase() !== expected) {
    throw new Error(`SRP6 verifier mismatch: ${verifier.toString("hex")}`);
  }
  // Interleave: leading zero bytes (an odd count) are skipped in pairs.
  const S = Buffer.alloc(32, 1);
  S[0] = 0;
  const shifted = Buffer.from(S);
  shifted[1] = 0;
  if (!interleave(S).equals(interleave(shifted))) throw new Error("SHA1Interleave skips an odd zero prefix as a pair");

  // A whole exchange against the server side of the fork's SRP6.cpp:
  // B = (k·v + g^b) mod N, S = (A · v^u)^b mod N, M = H(NgHash, H(I), s, A, B, K).
  for (let round = 0; round < 20; round++) {
    const reg = registration("PLAYER", "SECRET");
    const v = fromLE(reg.verifier);
    const b = fromLE(randomBytes(32));
    const B = toLE(mod(k * v + modPow(g, b, N), N), 32);
    const { A, M1, K } = clientProof("PLAYER", "secret", reg.salt, B);
    const u = fromLE(sha1(A, B));
    const serverK = interleave(toLE(modPow(fromLE(A) * modPow(v, u, N), b, N), 32));
    const serverM = sha1(NgHash(), sha1("PLAYER"), reg.salt, A, B, serverK);
    if (!serverK.equals(K) || !serverM.equals(M1)) throw new Error("SRP6 client and server disagree");
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const argv = process.argv.slice(2);
  if (argv[0] === "--self-test") {
    selfTest();
    console.log("auth-client self-test OK");
  } else if (argv[0] === "--login") {
    const portIndex = argv.indexOf("--port");
    const port = portIndex > 0 ? Number(argv[portIndex + 1]) : 3724;
    login({ user: argv[1], pass: argv[2], port })
      .then((realms) => console.log(JSON.stringify(realms, null, 2)))
      .catch((error) => {
        console.error(error.message);
        process.exit(1);
      });
  } else {
    console.error("usage: auth-client.mjs --self-test | --login <user> <password> [--port 3724]");
    process.exit(2);
  }
}
