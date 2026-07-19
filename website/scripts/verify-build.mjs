import { existsSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const websiteRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const dist = resolve(websiteRoot, "dist");
const indexPath = resolve(dist, "index.html");

function fail(message) {
  console.error(`Website artifact verification failed: ${message}`);
  process.exitCode = 1;
}

if (!existsSync(indexPath)) {
  fail("website/dist/index.html is missing");
} else {
  const html = readFileSync(indexPath, "utf8");
  const references = [...html.matchAll(/(?:src|href)="([^"]+)"/g)].map((match) => match[1]);

  for (const reference of references) {
    if (/^(?:https?:|mailto:|tel:|data:|#)/.test(reference)) continue;
    if (reference.startsWith("/")) {
      fail(`root-absolute resource is not Pages-subpath safe: ${reference}`);
      continue;
    }
    const clean = reference.split(/[?#]/, 1)[0];
    if (!clean || clean === "./") continue;
    const target = resolve(dist, clean);
    if (!existsSync(target)) fail(`referenced resource is missing: ${reference}`);
  }
}

const ogPath = resolve(dist, "og-typex.png");
if (!existsSync(ogPath)) {
  fail("og-typex.png is missing");
} else {
  const png = readFileSync(ogPath);
  const signature = png.subarray(1, 4).toString("ascii");
  const width = png.readUInt32BE(16);
  const height = png.readUInt32BE(20);
  if (signature !== "PNG" || width !== 1200 || height !== 630) {
    fail(`og-typex.png must be a 1200x630 PNG, received ${width}x${height}`);
  }
}

for (const staticFile of ["favicon.svg", "robots.txt", "sitemap.xml"]) {
  if (!existsSync(resolve(dist, staticFile))) fail(`${staticFile} is missing`);
}

if (existsSync(resolve(dist, "CNAME"))) fail("CNAME must be configured in GitHub Pages settings");

if (!process.exitCode) console.log("Website artifact verified: relative assets, metadata, and OG preview are present.");
