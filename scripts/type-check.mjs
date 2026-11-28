#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { REPO_ROOT } from './desktop-config.mjs';
import { wrapperArguments, wrapperBinary, wrapperEnvironment } from './install-rustc-wrapper.mjs';
import { OWN_CRATES } from './own-crates.mjs';
import { targetDir } from './target-dir.mjs';

const manifest = readFileSync(join(REPO_ROOT, 'Cargo.toml'), 'utf8');
for (const member of OWN_CRATES) {
  if (!manifest.includes(`"${member}"`)) {
    process.stderr.write(
      `[type-check] Cargo.toml does not carry ${member} as a member, so this check would pass over a crate this workspace never builds\n`,
    );
    process.exit(1);
  }
}

const binary = wrapperBinary();
if (!existsSync(binary)) {
  process.stderr.write(`[type-check] ${binary} is not on disk, so this check would run the system compiler\n`);
  process.exit(1);
}
const carrier = wrapperArguments();

const check = (member) =>
  spawnSync('cargo', ['check', ...carrier, '--manifest-path', join(REPO_ROOT, ...member.split('/'), 'Cargo.toml'), '--all-targets'], {
    cwd: REPO_ROOT,
    stdio: 'inherit',
    env: wrapperEnvironment({ ...process.env, CARGO_TARGET_DIR: targetDir() }),
  });

const results = OWN_CRATES.map((member) => ({ member, result: check(member) }));
const failed = results.filter(({ result }) => result.error !== undefined || result.status !== 0);
for (const { member, result } of failed) {
  process.stderr.write(
    `[type-check] ${member}: ${result.error?.message ?? `cargo check exited ${result.status}`}\n`,
  );
}
process.exit(failed.length === 0 ? 0 : 1);
