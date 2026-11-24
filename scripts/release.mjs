// One-command release:   node scripts/release.mjs <notes.md> [--no-publish]
//
// 1. Checks the version is the same in package.json, Cargo.toml and tauri.conf.json,
//    the git tree is committed, and the release does not exist yet.
// 2. Builds in ONE fixed folder (target-build) so libraries are reused (about a minute
//    instead of 4), signing the installer with the private updater key.
// 3. Copies the files to releases/v<version>/ (run/test from THERE, so a running Rewind
//    never locks the build folder again).
// 4. Writes latest.json (what the built-in updater reads) and publishes the GitHub release.

import { execSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const REPO = "PourquoiPasMoi/Rewind";
const root = path.resolve(import.meta.dirname, "..");
const args = process.argv.slice(2);
const notesFile = args.find((a) => !a.startsWith("--"));
const publish = !args.includes("--no-publish");

const fail = (msg) => {
  console.error(`\n  release: ${msg}\n`);
  process.exit(1);
};
const read = (p) => fs.readFileSync(path.join(root, p), "utf8").replace(/^﻿/, "");

if (!notesFile || !fs.existsSync(notesFile)) fail("pass a release notes file: node scripts/release.mjs notes.md");

const version = JSON.parse(read("src-tauri/tauri.conf.json")).version;
const pkg = JSON.parse(read("package.json")).version;
const cargo = /^version\s*=\s*"([^"]+)"/m.exec(read("src-tauri/Cargo.toml"))?.[1];
if (pkg !== version || cargo !== version) fail(`versions differ: tauri.conf ${version}, package.json ${pkg}, Cargo.toml ${cargo}`);

if (publish) {
  const dirty = execSync("git status --porcelain", { cwd: root, encoding: "utf8" }).trim();
  if (dirty) fail(`commit your changes first:\n${dirty}`);
  const exists = spawnSync("gh", ["release", "view", `v${version}`, "--repo", REPO], { stdio: "ignore" }).status === 0;
  if (exists) fail(`v${version} is already released. Bump the version first.`);
}

const keyPath = path.join(os.homedir(), ".tauri", "rewind-updater.key");
if (!fs.existsSync(keyPath)) fail(`updater key missing at ${keyPath} (without it, installed apps can't update)`);

console.log(`\n  Building Rewind ${version}...\n`);
const target = path.join(root, "target-build");
const build = spawnSync("npx", ["tauri", "build"], {
  cwd: root,
  stdio: "inherit",
  shell: true,
  env: {
    ...process.env,
    CARGO_TARGET_DIR: target,
    TAURI_SIGNING_PRIVATE_KEY: fs.readFileSync(keyPath, "utf8"),
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD: "",
  },
});
if (build.status !== 0) fail("build failed. If it says the exe is in use, quit the Rewind that runs from target-build (tray > Quit).");

const setupName = `Rewind_${version}_x64-setup.exe`;
const setup = path.join(target, "release", "bundle", "nsis", setupName);
const sig = `${setup}.sig`;
const exe = path.join(target, "release", "Rewind.exe");
for (const f of [setup, sig, exe]) if (!fs.existsSync(f)) fail(`missing build output ${f}`);

const out = path.join(root, "releases", `v${version}`);
fs.mkdirSync(out, { recursive: true });
fs.copyFileSync(setup, path.join(out, setupName));
fs.copyFileSync(exe, path.join(out, "Rewind.exe"));

const notes = fs.readFileSync(notesFile, "utf8").replace(/^﻿/, "");
const latest = {
  version,
  notes: notes.split(/\r?\n\r?\n/)[0].trim(),
  pub_date: new Date().toISOString(),
  platforms: {
    "windows-x86_64": {
      signature: fs.readFileSync(sig, "utf8").trim(),
      url: `https://github.com/${REPO}/releases/download/v${version}/${setupName}`,
    },
  },
};
fs.writeFileSync(path.join(out, "latest.json"), JSON.stringify(latest, null, 2));
console.log(`\n  Files ready in ${out}`);

if (!publish) process.exit(0);

const gh = spawnSync(
  "gh",
  [
    "release", "create", `v${version}`,
    path.join(out, setupName), path.join(out, "Rewind.exe"), path.join(out, "latest.json"),
    "--repo", REPO, "--title", `Rewind ${version}`, "--notes-file", notesFile, "--latest",
  ],
  { stdio: "inherit" },
);
if (gh.status !== 0) fail("gh release create failed");
console.log(`\n  Released: https://github.com/${REPO}/releases/tag/v${version}`);
console.log(`  Installer: https://github.com/${REPO}/releases/download/v${version}/${setupName}\n`);
