#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, readFileSync, statSync, existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { targetDir } from "./target-dir.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const repository = resolve(here, "..");

const wrapperCrate = join(repository, "crates", "seams", "rustc-wrapper");
const manifest = join(wrapperCrate, "Cargo.toml");
const configPath = join(repository, ".cargo", "config.toml");

const BINS = ["lvpa-rustc-wrapper", "lvpa-capability-panorama"];
const CRATES = ["crates/seams/rustc-wrapper", "crates/seams/openalgo-backend"];

function fail(message) {
  process.stderr.write(`[install-rustc-wrapper] FATAL: ${message}\n`);
  process.exit(1);
}

function assertSeamCratesAreMembers() {
  const host = readFileSync(join(repository, "Cargo.toml"), "utf8").split(/\r?\n/);
  const opened = host.findIndex((row) => /^\s*members\s*=\s*\[/.test(row));
  if (opened === -1) {
    fail(`${join(repository, "Cargo.toml")} carries no members array, so no crate stands in the one workspace`);
  }
  const closed = host.findIndex((row, at) => at > opened && /^\s*\]/.test(row));
  if (closed === -1) {
    fail(`${join(repository, "Cargo.toml")} opens the members array and never closes it`);
  }
  const listed = new Set(host.slice(opened + 1, closed).map((row) => row.trim().replace(/,$/, "")));
  for (const crate of CRATES) {
    if (!listed.has(`"${crate}"`)) {
      fail(`${crate} stands outside the members, so cargo would leave it out of the one workspace while the wrapper still injected into it`);
    }
  }
}

function declaredWrapper() {
  const text = readFileSync(configPath, "utf8");
  const line = text
    .split(/\r?\n/)
    .find((row) => /^\s*rustc-wrapper\s*=/.test(row));
  if (!line) {
    fail(`${configPath} declares no [build] rustc-wrapper, so the build would run the system compiler`);
  }
  const match = line.match(/=\s*"([^"]+)"/);
  if (!match) {
    fail(`${configPath} declares rustc-wrapper without a quoted path`);
  }
  return resolve(repository, match[1]);
}

const wrapperPath = declaredWrapper();
if (!wrapperPath.startsWith(repository) || !/[\\/]seams[\\/]/.test(wrapperPath)) {
  fail(`${wrapperPath} is not under the seam tree, so the install would place the wrapper outside what this repository owns`);
}
assertSeamCratesAreMembers();

const target = targetDir();
if (!existsSync(target)) {
  mkdirSync(target, { recursive: true });
}


const environment = {
  ...process.env,
  CARGO_TARGET_DIR: target,
};

const build = spawnSync(
  "cargo",
  [
    "build",
    "--manifest-path",
    manifest,
    "--bins",
    "--config",
    "build.rustc-wrapper=''",
    "--config",
    "build.rustc-workspace-wrapper=''",
  ],
  { cwd: repository, env: environment, stdio: "inherit" },
);
if (build.status !== 0) {
  fail(`cargo build of ${manifest} did not succeed`);
}

const produced = join(target, "debug");
for (const bin of BINS) {
  const from = join(produced, `${bin}.exe`);
  const alternative = join(produced, bin);
  const source = existsSync(from) ? from : alternative;
  if (!existsSync(source)) {
    fail(`the build produced no ${bin} binary under ${produced}`);
  }
  const destination = bin === "lvpa-rustc-wrapper" ? wrapperPath : join(dirname(wrapperPath), `${bin}.exe`);
  mkdirSync(dirname(destination), { recursive: true });
  copyFileSync(source, destination);
  if (!existsSync(destination) || statSync(destination).size === 0) {
    fail(`${destination} is not on disk after the copy`);
  }
}

process.stdout.write(`[install-rustc-wrapper] installed ${BINS.join(", ")} beside ${wrapperPath}\n`);
