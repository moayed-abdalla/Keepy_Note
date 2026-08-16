import { existsSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const nsisDir = join(root, "src-tauri", "target", "release", "bundle", "nsis");
const confPath = join(root, "src-tauri", "tauri.conf.json");
const outPath = join(root, "latest.json");

const REPO = "moayed-abdalla/Keepy_Note";

if (!existsSync(nsisDir)) {
  console.error(`NSIS bundle directory not found: ${nsisDir}`);
  process.exit(1);
}

const setups = readdirSync(nsisDir)
  .filter((name) => name.endsWith("_x64-setup.exe"))
  .map((name) => {
    const path = join(nsisDir, name);
    return { name, path, mtime: statSync(path).mtimeMs };
  })
  .sort((a, b) => b.mtime - a.mtime);

if (setups.length === 0) {
  console.error(`No *_x64-setup.exe found in ${nsisDir}`);
  process.exit(1);
}

const setup = setups[0];
const sigPath = `${setup.path}.sig`;
if (!existsSync(sigPath)) {
  console.error(
    `Signature missing: ${sigPath}\n` +
      "Build with createUpdaterArtifacts and TAURI_SIGNING_PRIVATE_KEY (or TAURI_SIGNING_PRIVATE_KEY_PATH) set."
  );
  process.exit(1);
}

const conf = JSON.parse(readFileSync(confPath, "utf8"));
const version = String(conf.version ?? "").replace(/^v/, "");
if (!version) {
  console.error("Could not read version from src-tauri/tauri.conf.json");
  process.exit(1);
}

const signature = readFileSync(sigPath, "utf8").trim();
const tag = `v${version}`;
const url = `https://github.com/${REPO}/releases/download/${tag}/${setup.name}`;

const latest = {
  version,
  notes: `Keepy Note ${version}`,
  pub_date: new Date().toISOString(),
  platforms: {
    "windows-x86_64": {
      signature,
      url
    }
  }
};

writeFileSync(outPath, `${JSON.stringify(latest, null, 2)}\n`);
console.log(`Wrote ${outPath}`);
console.log(`  version: ${version}`);
console.log(`  asset:   ${setup.name}`);
console.log(`  url:     ${url}`);
console.log("");
console.log("Upload to the GitHub Release (same tag):");
console.log(`  - ${setup.path}`);
console.log(`  - ${sigPath}`);
console.log(`  - ${outPath}`);
