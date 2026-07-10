import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { createRequire } from "node:module";
import { spawnSync } from "node:child_process";

const require = createRequire(import.meta.url);
const args = process.argv.slice(2);
const nativeBuildCommands = new Set(["build", "dev"]);

function fail(message) {
  console.error(`[typex tauri] ${message}`);
  process.exit(1);
}

function isFile(file) {
  try {
    return fs.statSync(file).isFile();
  } catch {
    return false;
  }
}

function commandOutput(command, commandArgs, env) {
  const result = spawnSync(command, commandArgs, {
    encoding: "utf8",
    env,
    windowsHide: true,
  });

  if (result.error || result.status !== 0) {
    return undefined;
  }

  return result.stdout.trim();
}

function findEnvKey(env, key) {
  const normalized = key.toLowerCase();
  return Object.keys(env).find((candidate) => candidate.toLowerCase() === normalized);
}

function getEnv(env, key) {
  const existingKey = findEnvKey(env, key);
  return existingKey ? env[existingKey] : undefined;
}

function setEnv(env, key, value) {
  const existingKey = findEnvKey(env, key);
  env[existingKey ?? key] = value;
}

function prependPath(env, key, value) {
  const current = getEnv(env, key) ?? "";
  const entries = current.split(path.delimiter).filter(Boolean);
  if (!entries.some((entry) => entry.toLowerCase() === value.toLowerCase())) {
    setEnv(env, key, [value, ...entries].join(path.delimiter));
  }
}

function findVswhere(env) {
  const installerPath = path.join(
    getEnv(env, "ProgramFiles(x86)") ??
      path.join(getEnv(env, "SystemDrive") ?? "C:", "Program Files (x86)"),
    "Microsoft Visual Studio",
    "Installer",
    "vswhere.exe",
  );

  if (isFile(installerPath)) {
    return installerPath;
  }

  const discovered = commandOutput("where.exe", ["vswhere.exe"], env);
  return discovered?.split(/\r?\n/, 1)[0];
}

function initializeMsvc(env) {
  const targetArch = getEnv(env, "VSCMD_ARG_TGT_ARCH")?.toLowerCase();
  const hostArch = getEnv(env, "VSCMD_ARG_HOST_ARCH")?.toLowerCase();
  if (getEnv(env, "VSCMD_VER") && targetArch === "x64" && hostArch === "x64") {
    return env;
  }

  const vswhere = findVswhere(env);
  if (!vswhere) {
    fail(
      "Visual Studio Build Tools were not found. Install the Desktop development with C++ workload.",
    );
  }

  const installationPath = commandOutput(
    vswhere,
    [
      "-latest",
      "-products",
      "*",
      "-requires",
      "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
      "-property",
      "installationPath",
    ],
    env,
  );
  if (!installationPath) {
    fail("No Visual Studio installation with the MSVC x64 tools was found.");
  }

  const devCmd = path.join(
    installationPath.split(/\r?\n/, 1)[0],
    "Common7",
    "Tools",
    "VsDevCmd.bat",
  );
  if (!isFile(devCmd)) {
    fail(`Visual Studio developer environment script is missing: ${devCmd}`);
  }

  const result = spawnSync(
    `call "${devCmd}" -arch=x64 -host_arch=x64 >nul && set`,
    {
      encoding: "utf8",
      env,
      shell: getEnv(env, "ComSpec") ?? true,
      windowsHide: true,
    },
  );
  if (result.error || result.status !== 0) {
    fail(`Failed to initialize the Visual Studio x64 environment via ${devCmd}.`);
  }

  const initialized = { ...env };
  for (const line of result.stdout.split(/\r?\n/)) {
    const separator = line.indexOf("=");
    if (separator <= 0) {
      continue;
    }
    setEnv(initialized, line.slice(0, separator), line.slice(separator + 1));
  }
  return initialized;
}

function discoverVulkanSdk(env) {
  const configuredSdk = getEnv(env, "VULKAN_SDK");
  if (configuredSdk) {
    return configuredSdk;
  }

  const roots = [
    path.join(getEnv(env, "SystemDrive") ?? "C:", "VulkanSDK"),
    path.join(
      getEnv(env, "ProgramFiles") ??
        path.join(getEnv(env, "SystemDrive") ?? "C:", "Program Files"),
      "VulkanSDK",
    ),
  ];
  const candidates = roots.flatMap((root) => {
    if (!fs.existsSync(root)) {
      return [];
    }
    return fs
      .readdirSync(root, { withFileTypes: true })
      .filter((entry) => entry.isDirectory())
      .map((entry) => path.join(root, entry.name));
  });

  candidates.sort((left, right) =>
    right.localeCompare(left, undefined, { numeric: true, sensitivity: "base" }),
  );
  return candidates[0];
}

function configureVulkan(env) {
  const sdk = discoverVulkanSdk(env);
  if (!sdk) {
    fail("Vulkan SDK was not found. Install the LunarG Vulkan SDK or set VULKAN_SDK.");
  }

  const glslc = path.join(sdk, "Bin", "glslc.exe");
  const cmakeRoot = path.join(sdk, "Lib", "cmake");
  const spirvHeadersConfigs = [
    path.join(cmakeRoot, "SPIRV-HeadersConfig.cmake"),
    path.join(cmakeRoot, "SPIRV-Headers", "SPIRV-HeadersConfig.cmake"),
  ];
  if (!isFile(glslc)) {
    fail(`The Vulkan shader compiler is missing: ${glslc}`);
  }
  if (!spirvHeadersConfigs.some((candidate) => isFile(candidate))) {
    fail(`The Vulkan SDK does not contain SPIRV-HeadersConfig.cmake under ${cmakeRoot}.`);
  }

  setEnv(env, "VULKAN_SDK", sdk);
  prependPath(env, "PATH", path.join(sdk, "Bin"));
  prependPath(env, "CMAKE_PREFIX_PATH", cmakeRoot);
}

function configureNinja(env) {
  setEnv(env, "CMAKE_GENERATOR", "Ninja");

  if (commandOutput("where.exe", ["ninja.exe"], env)) {
    return;
  }

  const wingetNinja = path.join(
    getEnv(env, "LOCALAPPDATA") ?? "",
    "Microsoft",
    "WinGet",
    "Links",
    "ninja.exe",
  );
  if (isFile(wingetNinja)) {
    prependPath(env, "PATH", path.dirname(wingetNinja));
    return;
  }

  fail("Ninja was not found. Typex Windows native builds require the Ninja CMake generator.");
}

function prepareWindowsNativeBuild(env) {
  const prepared = initializeMsvc({ ...env });
  configureNinja(prepared);
  configureVulkan(prepared);
  return prepared;
}

const command = args.find((argument) => nativeBuildCommands.has(argument));
const env =
  process.platform === "win32" && nativeBuildCommands.has(command)
    ? prepareWindowsNativeBuild(process.env)
    : process.env;

const tauriCli = require.resolve("@tauri-apps/cli/tauri.js");
const result = spawnSync(process.execPath, [tauriCli, ...args], {
  env,
  stdio: "inherit",
  windowsHide: false,
});

if (result.error) {
  fail(`Failed to start the Tauri CLI: ${result.error.message}`);
}
if (result.signal) {
  process.kill(process.pid, result.signal);
}
process.exit(result.status ?? 1);
