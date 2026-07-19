import { describe, expect, it } from "vitest";
import { siteCopy } from "./content";
import { SITE_LINKS } from "./links";

function shape(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(shape);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>)
        .sort(([a], [b]) => a.localeCompare(b))
        .map(([key, child]) => [key, shape(child)]),
    );
  }
  return typeof value;
}

function strings(value: unknown): string[] {
  if (typeof value === "string") return [value];
  if (Array.isArray(value)) return value.flatMap(strings);
  if (value !== null && typeof value === "object") {
    return Object.values(value as Record<string, unknown>).flatMap(strings);
  }
  return [];
}

describe("site copy", () => {
  it("keeps English and Chinese resources structurally identical", () => {
    expect(shape(siteCopy["zh-CN"])).toEqual(shape(siteCopy.en));
    expect(strings(siteCopy.en).every((value) => value.trim().length > 0)).toBe(true);
    expect(strings(siteCopy["zh-CN"]).every((value) => value.trim().length > 0)).toBe(true);
  });

  it("publishes the required platform support information", () => {
    expect(siteCopy.en.download.platforms[0].support).toContain("macOS 12");
    expect(siteCopy.en.download.platforms[1].support).toContain("Windows 10 22H2+");
    expect(siteCopy.en.download.platforms[1].support).toContain("Windows 11 x64");
    expect(siteCopy["zh-CN"].download.releaseNote).toContain("GitHub");
  });

  it("uses stable GitHub destinations for source, license, and downloads", () => {
    expect(SITE_LINKS).toEqual({
      repository: "https://github.com/typex-ink/Typex",
      releases: "https://github.com/typex-ink/Typex/releases",
      license: "https://github.com/typex-ink/Typex/blob/master/LICENSE",
    });
  });
});
