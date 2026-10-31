// First-party build linked by https://mpv.io/installation/.
import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { resolve, dirname, join } from "node:path";
import { execFileSync } from "node:child_process";

if (process.platform !== "win32" || process.arch !== "x64") throw new Error("Install mpv ≥0.38 with your OS package manager and set LOCALVOX_MPV if needed");
const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
let development = false;
try { await readFile(join(root, "Cargo.toml")); development = true; } catch { /* Release bundle. */ }
const directory = development ? join(root, "target", "tools", "mpv") : join(root, "tools", "mpv");
await mkdir(directory, { recursive: true });
const executable = join(directory, "mpv.exe");
try {
  const installed = JSON.parse(await readFile(join(directory, "installed.json"), "utf8"));
  const current = createHash("sha256").update(await readFile(executable)).digest("hex");
  if (installed.executable_sha256 === current) { console.log(`mpv already installed: ${executable}`); process.exit(0); }
} catch { /* New installation or an incomplete previous attempt. */ }
const response = await fetch("https://api.github.com/repos/mpv-player/mpv/releases/tags/git-release", { signal: AbortSignal.timeout(30_000), headers: { "User-Agent": "localvox-development-setup" } });
if (!response.ok) throw new Error(`mpv release metadata: HTTP ${response.status}`);
const release = await response.json();
const asset = release.assets.find((asset) => /^mpv-.*-x86_64-w64-mingw32-full\.zip$/.test(asset.name));
if (!asset || !/^sha256:[a-f0-9]{64}$/.test(asset.digest ?? "")) throw new Error("Official mpv archive has no verifiable SHA-256 digest");
const download = await fetch(asset.browser_download_url, { signal: AbortSignal.timeout(120_000) });
if (!download.ok) throw new Error(`mpv archive: HTTP ${download.status}`);
const bytes = Buffer.from(await download.arrayBuffer());
if (bytes.length > 150 * 1024 * 1024) throw new Error("Unexpected mpv archive size");
const digest = createHash("sha256").update(bytes).digest("hex");
if (`sha256:${digest}` !== asset.digest) throw new Error("mpv archive SHA-256 mismatch");
const archive = join(directory, "mpv.zip");
await writeFile(archive, bytes);
// Native Windows archive support; no downloaded extraction helper or shell expansion.
execFileSync("tar.exe", ["-xf", archive, "-C", directory], { windowsHide: true, stdio: "inherit" });
const executableDigest = createHash("sha256").update(await readFile(executable)).digest("hex");
await writeFile(join(directory, "installed.json"), JSON.stringify({ source: asset.browser_download_url, sha256: digest, executable_sha256: executableDigest, installed_at: new Date().toISOString() }, null, 2));
console.log(`Verified mpv installed: ${executable}`);
