#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, existsSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { OWN_CRATES, SEAM_CRATES } from "./own-crates.mjs";
import { REPOSITORY_ENV } from "./step-guarantees.mjs";
import { targetDir } from "./target-dir.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const repository = resolve(here, "..");

/// The wrapper this chain installs, named in the environment every step of it runs in: a
/// reading of the graph is a reading of what the build compiles, and the build compiles
/// through this binary.
export const WRAPPER_ENV = "LVPA_RUSTC_WRAPPER";

const STATED = ["workspace.package", "workspace.dependencies"];

const LINT = "workspace.lints";

const SENTINEL = "# [upstream-sync]";

const PINNED = "rust-toolchain.toml";

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

/// The graph is read before cargo starts a compiler, so the pass that reads it hands the wrapper
/// no source to rewrite: the cards are landed by a pass of their own over the same root, and the
/// reading then starts from cards standing at the windows this repository lands them at.
export function landTheCards(root) {
  const status = spawnSync(wrapperBinary(), [LAND], {
    cwd: root,
    env: wrapperEnvironment({ ...process.env, CARGO_TARGET_DIR: targetDir() }),
    stdio: "inherit",
  });
  if (status.status !== 0) {
    fail(
      `the landing pass over ${root} did not succeed, so no graph is read from cards standing in the trees' own windows`,
    );
  }
}

/// The cards go back the way their trees wrote them whether the reading answered or stopped: a
/// landing that stood in a tree after the reading is a tree this repository wrote in.
export function putBackTheCards(root) {
  const status = spawnSync(wrapperBinary(), [PUT_BACK], {
    cwd: root,
    env: wrapperEnvironment({ ...process.env, CARGO_TARGET_DIR: targetDir() }),
    stdio: "inherit",
  });
  if (status.status !== 0) {
    fail(`the pass that puts the cards back over ${root} did not succeed, so a tree may still stand at a landed series`);
  }
}

export const LAND = "--land-card-edges";

export const PUT_BACK = "--put-back-the-cards";

export function wrapperEnvironment(environment = process.env) {
  const held = { ...environment };
  delete held.RUSTC_WRAPPER;
  delete held.RUSTC_WORKSPACE_WRAPPER;
  delete held.CARGO_BUILD_RUSTC_WRAPPER;
  delete held.CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER;
  held[REPOSITORY_ENV] = repository;
  held[WRAPPER_ENV] = wrapperBinary();
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
  for (const crate of SEAM_CRATES) {
    if (!listed.has(`"${crate}"`)) continue;
    const bins = readFileSync(join(repository, ...crate.split("/"), "Cargo.toml"), "utf8");
    const named = [...bins.matchAll(/^\s*name\s*=\s*"([^"]+)"\s*$/gm)].map((found) => found[1]);
    const missing = crate === wrapperCrateRelative ? BINS.filter((bin) => !named.includes(bin)) : [];
    if (missing.length !== 0) {
      fail(`the install builds ${missing.join(", ")} out of ${crate}, and its manifest gives it no such binary, so the build would produce nothing to run`);
    }
  }
}

function copyTree(from, into) {
  if (!statSync(from).isDirectory()) {
    mkdirSync(dirname(into), { recursive: true });
    writeFileSync(into, readFileSync(from));
    return;
  }
  mkdirSync(into, { recursive: true });
  for (const name of readdirSync(from)) {
    if (name === "target" || name === "node_modules") continue;
    copyTree(join(from, name), join(into, name));
  }
}

function seamWrite(bench) {
  const members = OWN_CRATES.map((crate) => {
    const at = join(bench, ...crate.split("/"));
    mkdirSync(at, { recursive: true });
    for (const name of readdirSync(join(repository, ...crate.split("/")))) {
      copyTree(join(repository, ...crate.split("/"), name), join(at, name));
    }
    return crate;
  });
  const text = readFileSync(join(repository, "Cargo.toml"), "utf8");
  const rows = ["[workspace]", "resolver = \"2\"", "members = ["];
  for (const crate of members) rows.push(`"${crate}",`);
  rows.push("]");
  let open = null;
  for (const line of text.split(/\r?\n/)) {
    const header = line.trim().match(/^\[([^\]]+)\]$/);
    if (header !== null) {
      open = STATED.includes(header[1]) ? header[1] : null;
      if (open !== null) rows.push("", `[${open}]`);
      continue;
    }
    if (open === null || line.trim() === "" || line.trim().startsWith(SENTINEL)) continue;
    rows.push(line);
  }
  writeFileSync(join(bench, "Cargo.toml"), `${rows.join("\n")}\n`, "utf8");
  writeFileSync(join(bench, PINNED), readFileSync(join(repository, PINNED)), "utf8");
  return members;
}

function seamBench() {
  const bench = mkdtempSync(join(tmpdir(), "lvpa-seam-workspace-"));
  return { bench, members: seamWrite(bench) };
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

  const { bench } = seamBench();
  let build;
  try {
    build = spawnSync(
      "cargo",
      ["build", "--package", WRAPPER, "--bins"],
      { cwd: bench, env: environment, stdio: "inherit", shell: true },
    );
  } finally {
    rmSync(bench, { recursive: true, force: true });
  }
  if (build.status !== 0) {
    fail(`cargo build of ${wrapperCrateRelative} over the seam workspace did not succeed`);
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
