#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { REPO_ROOT } from './desktop-config.mjs';
import { targetDir } from './target-dir.mjs';

const SHAPE = join(REPO_ROOT, 'crates', 'wiring', 'shape');
const ORDER_BIN = 'lvpa-chain-order';

function listChain(entrance, extra) {
  const listed = spawnSync(
    'cargo',
    [
      'run',
      '--quiet',
      '--manifest-path',
      join(SHAPE, 'Cargo.toml'),
      '--bin',
      ORDER_BIN,
      '--',
      ...extra,
      entrance,
    ],
    { cwd: REPO_ROOT, encoding: 'utf8', env: { ...process.env, CARGO_TARGET_DIR: targetDir() } },
  );
  if (listed.status !== 0) {
    process.stderr.write(
      `[chain-order] cargo run --bin ${ORDER_BIN} at ${SHAPE} exited ${listed.status}: ${listed.stderr}\n`,
    );
    process.exit(1);
  }
  const rows = listed.stdout.split(/\r?\n/).filter((name) => name.trim() !== '');
  if (rows.length === 0) {
    process.stderr.write(`[chain-order] ${ORDER_BIN} ${entrance} listed no step\n`);
    process.exit(1);
  }
  return rows;
}

export function chainOrder(entrance) {
  return listChain(entrance, []);
}

export function chainGuarantees(entrance) {
  const rows = listChain(entrance, ['--guarantees']);
  const table = new Map();
  for (const row of rows) {
    const [name, refuses, onDisk] = row.split('\t');
    if (name === undefined || refuses === undefined) {
      process.stderr.write(`[chain-order] ${ORDER_BIN} --guarantees emitted a row this reader cannot place: ${row}\n`);
      process.exit(1);
    }
    table.set(name, { refusesWhen: refuses, onDisk: (onDisk ?? '').split(',').filter((entry) => entry !== '') });  }
  return table;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  for (const name of chainOrder(process.argv[2])) process.stdout.write(`${name}\n`);
}
