import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptsDir = dirname(fileURLToPath(import.meta.url));
const websiteRoot = resolve(scriptsDir, "..");
const repositoryRoot = resolve(websiteRoot, "..");

mkdirSync(resolve(websiteRoot, "public"), { recursive: true });
copyFileSync(
  resolve(repositoryRoot, "assets/icon/typex.svg"),
  resolve(websiteRoot, "public/favicon.svg"),
);
