#!/usr/bin/env node
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { REPO_ROOT } from './desktop-config.mjs';

export const STORE = join(REPO_ROOT, 'node_modules', '.pnpm');

function key(name) {
  return name.replace('/', '+');
}

export function storeEntry(name, version) {
  const wanted = `${key(name)}@${version}`;
  const present = readdirSync(STORE)
    .filter((entry) => entry === wanted || entry.startsWith(`${wanted}_`) || entry.startsWith(`${wanted}(`))
    .sort((a, b) => a.length - b.length || (a < b ? -1 : 1));
  if (present.length === 0) {
    throw new Error(
      `[offline-store] ${name}@${version} is not in the host store at ${STORE}, so a frontend this chain starts cannot be resolved without fetching`,
    );
  }
  return join(STORE, present[0]);
}

export function packageRoot(name, version) {
  const root = join(storeEntry(name, version), 'node_modules', ...name.split('/'));
  if (!existsSync(join(root, 'package.json'))) {
    throw new Error(`[offline-store] ${name}@${version} stands in ${STORE} without a package.json under ${root}`);
  }
  return root;
}

export function installedVersion(name, version) {
  return JSON.parse(readFileSync(join(packageRoot(name, version), 'package.json'), 'utf8')).version;
}
