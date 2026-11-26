#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, statSync, existsSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { OWN_CRATES, SEAM_CRATES } from "./own-crates.mjs";
import { REPOSITORY_ENV } from "./step-guarantees.mjs";
import { targetDir } from "./target-dir.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const repository = resolve(here, "..");

const wrapperCrate = join(repository, "crates", "seams", "rustc-wrapper");
const wrapperCrateRelative = "crates/seams/rustc-wrapper";
const manifest = join(wrapperCrate, "Cargo.toml");

const WRAPPER = "lvpa-rustc-wrapper";
const PANORAMA = "lvpa-capability-panorama";
const BINS = [WRAPPER, PANORAMA];

export function wrapperBinary() {
  return join(targetDir(), "debug", `${WRAPPER}.exe`);
}

export function panoramaBinary() {
  return join(targetDir(), "debug", `${PANORAMA}.exe`);
}

export function wrapperEnvironment(environment = process.env) {
  const held = { ...environment };
  delete held.RUSTC_WRAPPER;
  delete held.RUSTC_WORKSPACE_WRAPPER;
  delete held.CARGO_BUILD_RUSTC_WRAPPER;
  delete held.CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER;
  held[REPOSITORY_ENV] = repository;
  return held;
}

export function wrapperArguments() {
  return [
    '--config',
    `build.rustc-wrapper=${JSON.stringify(wrapperBinary())}`,
    '--config',
    `build.rustc-workspace-wrapper=${JSON.stringify(wrapperBinary())}`,
  ];
}

function fail(message) {
  process.stderr.write(`[install-rustc-wrapper] FATAL: ${message}\n`);
  process.exit(1);
}

function memberList() {
  const host = readFileSync(join(repository, "Cargo.toml"), "utf8").split(/\r?\n/);
  const opened = host.findIndex((row) => /^\s*members\s*=\s*\[/.test(row));
  if (opened === -1) {
    fail(`${join(repository, "Cargo.toml")} carries no members array, so no crate stands in the one workspace`);
  }
  const closed = host.findIndex((row, at) => at > opened && /^\s*\]/.test(row));
  if (closed === -1) {
    fail(`${join(repository, "Cargo.toml")} opens the members array and never closes it`);
  }
  return new Set(host.slice(opened + 1, closed).map((row) => row.trim().replace(/,$/, "")));
}

function assertSeamCratesAreMembers() {
  const listed = memberList();
  for (const crate of OWN_CRATES) {
    if (!listed.has(`"${crate}"`)) {
      fail(`${crate} stands outside the members, so cargo would leave it out of the one workspace while the wrapper still injected into it`);
    }
  }
}

function assertSeamCratesAreBuilt() {
  const listed = memberList();
  const across = spawnSync(
    "cargo",
    ["metadata", "--no-deps", "--format-version", "1", "--manifest-path", join(repository, "Cargo.toml")],
    { cwd: repository, env: { ...process.env, CARGO_TARGET_DIR: targetDir() }, encoding: "utf8" },
  );
  if (across.status !== 0) {
    fail(`cargo metadata over ${join(repository, "Cargo.toml")} exited ${across.status}: ${(across.stderr ?? "").trim()}`);
  }
  const known = new Map(
    JSON.parse(across.stdout).packages.map((entry) => [entry.manifest_path.replace(/\\/g, "/").toLowerCase(), entry.targets]),
  );
  for (const crate of SEAM_CRATES) {
    if (!listed.has(`"${crate}"`)) continue;
    const manifest = join(repository, ...crate.split("/"), "Cargo.toml").replace(/\\/g, "/").toLowerCase();
    if (!known.has(manifest)) {
      fail(`${crate} stands in the members list, and the mirror graph resolves no package for ${manifest}, so nothing would compile it`);
    }
    const bins = known
      .get(manifest)
      .filter((entry) => entry.kind.includes("bin"))
      .map((entry) => entry.name);
    const missing = crate === wrapperCrateRelative ? BINS.filter((bin) => !bins.includes(bin)) : [];
    if (missing.length !== 0) {
      fail(`the install builds ${missing.join(", ")} out of ${crate}, and the mirror graph gives it no such binary, so the build would produce nothing to run`);
    }
  }
}

function declaredWrapper() {
  const binary = wrapperBinary();
  if (!binary.startsWith(join(targetDir(), 'debug'))) {
    fail(`${binary} is not under ${targetDir()}`);
  }
  return binary;
}

export function buildWrapper() {
  declaredWrapper();
  assertSeamCratesAreMembers();
  assertSeamCratesAreBuilt();

  const target = targetDir();
  if (!existsSync(target)) {
    mkdirSync(target, { recursive: true });
  }

  const environment = wrapperEnvironment({ ...process.env, CARGO_TARGET_DIR: target });

  const build = spawnSync(
    "cargo",
    ["build", "--manifest-path", manifest, "--bins"],
    { cwd: repository, env: environment, stdio: "inherit" },
  );
  if (build.status !== 0) {
    fail(`cargo build of ${manifest} did not succeed`);
  }

  const produced = join(target, "debug");
  for (const bin of BINS) {
    const from = join(produced, `${bin}.exe`);
    const alternative = join(produced, bin);
    const candidate = existsSync(from) ? from : alternative;
    if (!existsSync(candidate)) {
      fail(`the build produced no ${bin} binary under ${produced}`);
    }
    if (!existsSync(candidate) || statSync(candidate).size === 0) {
      fail(`${candidate} is not on disk after the build`);
    }
  }

  for (const bin of BINS) {
    const fresh = join(produced, `${bin}.exe`);
    const path = existsSync(fresh) ? fresh : join(produced, bin);
    writeFileSync(join(target, `${bin}.produced`), `${path}:${statSync(path).size}\n`, "utf8");
  }

  process.stdout.write(`[install-rustc-wrapper] built ${BINS.join(", ")} under ${produced}\n`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  buildWrapper();
}
