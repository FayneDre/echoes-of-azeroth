import { beforeEach, describe, expect, it, vi } from "vitest";

vi.stubGlobal("document", { documentElement: { lang: "" } });
vi.stubGlobal("localStorage", {
  getItem: () => null,
  setItem: () => {},
  removeItem: () => {},
});

const { i18n, t, localized, hasMessage, gigabytes } = await import("./i18n.svelte");

describe("i18n", () => {
  beforeEach(() => {
    i18n.locale = "en";
  });

  it("replaces known placeholders and keeps unknown ones", () => {
    expect(t("error.disk.full", { needed: "9.0", available: "2.0" })).toContain("9.0 GB needed, 2.0 GB free");
    expect(t("error.disk.full")).toMatch(/\{\w+\}/);
  });

  it("follows the chosen locale and marks the document", () => {
    const english = t("nav.language");
    i18n.locale = "fr";
    expect(document.documentElement.lang).toBe("fr");
    expect(t("nav.language")).not.toBe(english);
  });

  it("falls back to English for catalog text without French", () => {
    i18n.locale = "fr";
    expect(localized({ en: "Bots", fr: "Robots" })).toBe("Robots");
    expect(localized({ en: "Bots" })).toBe("Bots");
  });

  it("knows backend error codes only when translated", () => {
    expect(hasMessage("error.backup.incomplete")).toBe(true);
    expect(hasMessage("error.no.such.code")).toBe(false);
  });

  it("formats gigabytes with one decimal in the locale's style", () => {
    expect(gigabytes(9_000_000_000)).toBe("9.0");
    i18n.locale = "fr";
    expect(gigabytes(1_250_000_000)).toBe("1,3");
  });
});
