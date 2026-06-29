import { copyFileSync, existsSync, mkdirSync, statSync } from "node:fs";
import { dirname, extname, join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const version = JSON.parse(await readFileText(join(root, "package.json"))).version;

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: root,
    stdio: "inherit",
  });
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

async function readFileText(path) {
  const { readFile } = await import("node:fs/promises");
  return readFile(path, "utf8");
}

function rustcHostTriple() {
  const result = spawnSync("rustc", ["-vV"], {
    cwd: root,
    encoding: "utf8",
  });
  if (result.status !== 0) {
    process.stderr.write(result.stderr ?? "");
    process.exit(result.status ?? 1);
  }
  const host = result.stdout
    .split(/\r?\n/)
    .find((line) => line.startsWith("host: "))
    ?.slice("host: ".length)
    .trim();
  if (!host) {
    throw new Error("Could not determine rustc host triple");
  }
  return host;
}

const host = rustcHostTriple();
const exe = process.platform === "win32" ? ".exe" : "";
const builtBinary = join(root, "target", "release", `podcast_downloader${exe}`);
const sidecarDir = join(root, "src-tauri", "binaries");
const sidecarBinary = join(sidecarDir, `podcast-downloader-tui-${host}${exe}`);
const releaseDir = join(root, "releases");
const releaseName =
  process.platform === "win32"
    ? `PodcastDownloaderTui-${version}-${host}.exe`
    : `podcast-downloader-tui-${version}-${host}`;
const releaseBinary = join(releaseDir, releaseName);

run("cargo", ["build", "--release", "--bin", "podcast_downloader"]);

if (!existsSync(builtBinary) || !statSync(builtBinary).isFile()) {
  throw new Error(`Expected built TUI binary at ${builtBinary}`);
}

mkdirSync(sidecarDir, { recursive: true });
mkdirSync(releaseDir, { recursive: true });
copyFileSync(builtBinary, sidecarBinary);
copyFileSync(builtBinary, releaseBinary);

console.log(`TUI sidecar: ${sidecarBinary}`);
console.log(`TUI release binary: ${releaseBinary}`);

if (extname(releaseBinary) === "") {
  const { chmodSync } = await import("node:fs");
  chmodSync(sidecarBinary, 0o755);
  chmodSync(releaseBinary, 0o755);
}
