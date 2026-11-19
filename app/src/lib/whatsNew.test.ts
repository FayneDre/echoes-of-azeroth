import { beforeEach, describe, expect, it, vi } from "vitest";
import notes from "./release-notes.json";

const store = new Map<string, string>();
vi.stubGlobal("document", { documentElement: { lang: "" } });
vi.stubGlobal("localStorage", {
  getItem: (key: string) => store.get(key) ?? null,
  setItem: (key: string, value: string) => void store.set(key, value),
  removeItem: (key: string) => void store.delete(key),
});

const { i18n } = await import("./i18n/i18n.svelte");
const { bundledNotes, justUpdated, noteLines } = await import("./whatsNew");

describe("release notes", () => {
  it("has English and French lines, one for one, for every version", () => {
    for (const [version, entry] of Object.entries(notes as Record<string, Record<string, string[]>>)) {
      expect(version).toMatch(/^\d+\.\d+\.\d+$/);
      expect(entry.en?.length, version).toBeGreaterThan(0);
      expect(entry.fr?.length, version).toBe(entry.en?.length);
    }
  });

  it("shows the player's language, English as the fallback", () => {
    i18n.locale = "fr";
    expect(noteLines({ en: ["One"], fr: ["Un"] })).toEqual(["Un"]);
    expect(noteLines({ en: ["One"] })).toEqual(["One"]);
    expect(noteLines(null)).toEqual([]);
    expect(bundledNotes("0.0.0")).toBeNull();
  });
});

describe("justUpdated", () => {
  beforeEach(() => store.clear());

  it("shows nothing on a first install, then only after a version change", () => {
    expect(justUpdated("0.3.2")).toBe(false);
    expect(justUpdated("0.3.2")).toBe(false);
    expect(justUpdated("0.3.3")).toBe(true);
    expect(justUpdated("0.3.3")).toBe(false);
  });

  it("reads the marker of launchers up to 0.3.1 once", () => {
    store.set("rk.whatsNew", JSON.stringify({ version: "0.3.2" }));
    expect(justUpdated("0.3.2")).toBe(true);
    expect(store.has("rk.whatsNew")).toBe(false);
    expect(justUpdated("0.3.2")).toBe(false);
  });

  it("ignores a damaged legacy marker", () => {
    store.set("rk.whatsNew", "{not json");
    expect(justUpdated("0.3.2")).toBe(false);
  });
});
