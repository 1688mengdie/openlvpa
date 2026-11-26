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

export const GUARANTEES = '--guarantees';

export function chainOrder(entrance) {
  return listChain(entrance, []).map((row) => row.split('\t')[0]);
}

export function chainGuarantees(entrance) {
  const table = new Map();
  for (const row of listChain(entrance, [GUARANTEES])) {
    const [name, onDisk, leaves] = row.split('\t');
    if (name === undefined || onDisk === undefined || leaves === undefined) {
      process.stderr.write(`[chain-order] ${ORDER_BIN} ${GUARANTEES} emitted a row this reader cannot place: ${row}\n`);
      process.exit(1);
    }
    const column = (text) => text.split(',').filter((entry) => entry !== '');
    table.set(name, { onDisk: column(onDisk), leaves: column(leaves) });
  }
  return table;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const asked = process.argv.slice(2);
  if (asked.length === 2 && asked[0] === GUARANTEES) {
    for (const [name, held] of chainGuarantees(asked[1])) {
      process.stdout.write(`${[name, held.onDisk.join(','), held.leaves.join(',')].join('\t')}\n`);
    }
  } else if (asked.length === 1) {
    for (const name of chainOrder(asked[0])) process.stdout.write(`${name}\n`);
  } else {
    process.stderr.write(`[chain-order] name one entrance, or ${GUARANTEES} with one entrance\n`);
    process.exit(2);
  }
}
