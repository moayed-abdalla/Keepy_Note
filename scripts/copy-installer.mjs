import { copyFileSync, existsSync, readdirSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const nsisDir = join(root, "src-tauri", "target", "release", "bundle", "nsis");
const dest = join(root, "Keepy-Note-setup.exe");

if (!existsSync(nsisDir)) {
  console.error(`NSIS bundle directory not found: ${nsisDir}`);
  process.exit(1);
}

const setups = readdirSync(nsisDir)
  .filter((name) => name.endsWith("_x64-setup.exe"))
  .map((name) => {
    const path = join(nsisDir, name);
    return { path, mtime: statSync(path).mtimeMs };
  })
  .sort((a, b) => b.mtime - a.mtime);

if (setups.length === 0) {
  console.error(`No *_x64-setup.exe found in ${nsisDir}`);
  process.exit(1);
}

const source = setups[0].path;
copyFileSync(source, dest);
console.log(`Copied ${source} -> ${dest}`);
