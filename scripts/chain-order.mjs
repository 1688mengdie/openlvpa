#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { REPO_ROOT } from './desktop-config.mjs';
import { targetDir } from './target-dir.mjs';

const SHAPE = join(REPO_ROOT, 'crates', 'wiring', 'shape');
const SHAPE_BIN = 'lvpa-chain-order';

export const ENTRANCES = ['dev', 'build'];

const ORDER = ['wrapper', 'take', 'absorb', 'merge', 'strip', 'deshell', 'seam', 'patches', 'install', 'frontend', 'dev', 'build'];

const TERMINUS = new Map([
  ['dev', 'dev'],
  ['build', 'build'],
]);

function refused(entrance) {
  process.stderr.write(`[chain-order] \`${entrance}\` is not an entrance: ${ENTRANCES.join(', ')}\n`);
  process.exit(2);
}

export function chainOrder(entrance) {
  const terminus = TERMINUS.get(entrance);
  if (terminus === undefined) refused(entrance);
  const at = ORDER.indexOf(terminus);
  if (at === -1) {
    process.stderr.write(`[chain-order] ${TERMINUS.get(entrance)} names a step the chain does not carry: ${ORDER.join(', ')}\n`);
    process.exit(1);
  }
  return ORDER.slice(0, at + 1);
}

export function shapeSteps() {
  return [...ORDER];
}

export function chainGuarantees(entrance) {
  const listed = spawnSync(
    'cargo',
    ['run', '--quiet', '--manifest-path', join(SHAPE, 'Cargo.toml'), '--bin', SHAPE_BIN, '--', '--guarantees', entrance],
    { cwd: REPO_ROOT, encoding: 'utf8', env: { ...process.env, CARGO_TARGET_DIR: targetDir() } },
  );
  if (listed.status !== 0) {
    process.stderr.write(
      `[chain-order] ${SHAPE_BIN} could not read Step::guarantee over \`${entrance}\` at ${SHAPE}: ${listed.stderr}\n`,
    );
    process.exit(1);
  }
  const rows = listed.stdout.split(/\r?\n/).filter((row) => row.trim() !== '');
  if (rows.length === 0) {
    process.stderr.write(`[chain-order] ${SHAPE_BIN} named no step for \`${entrance}\`\n`);
    process.exit(1);
  }
  const table = new Map();
  for (const row of rows) {
    const [name, onDisk, leaves] = row.split('\t');
    if (name === undefined || onDisk === undefined || leaves === undefined) {
      process.stderr.write(`[chain-order] ${SHAPE_BIN} emitted a row this reader cannot place: ${row}\n`);
      process.exit(1);
    }
    const column = (text) => text.split(',').filter((entry) => entry !== '');
    table.set(name, { onDisk: column(onDisk), leaves: column(leaves) });
  }
  return table;
}

export function chainGuaranteesOf(entrances) {
  const table = new Map();
  for (const entrance of entrances) {
    for (const [name, declared] of chainGuarantees(entrance)) table.set(name, declared);
  }
  return table;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const asked = process.argv.slice(2);
  if (asked.length === 1) {
    for (const name of chainOrder(asked[0])) process.stdout.write(`${name}\n`);
  } else {
    process.stderr.write('[chain-order] name one entrance: dev, build\n');
    process.exit(2);
  }
}
