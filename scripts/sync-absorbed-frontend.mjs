#!/usr/bin/env node
import { existsSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { REPO_ROOT, absorbedProjects, frontendRoots, projectRoot } from './absorbed-sources.mjs';

const HOST_WORKSPACE = join(REPO_ROOT, 'pnpm-workspace.yaml');
const SENTINEL = '# [upstream-sync]';
const REGION_KEY = 'absorbed frontend packages';
const SKIP_DIRS = new Set(['node_modules', 'vendor', 'dist', 'target', '.git', '.cargo', 'build']);

function workspaceDeps(manifestPath) {
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  const names = [];
  for (const field of ['dependencies', 'devDependencies', 'optionalDependencies', 'peerDependencies']) {
    for (const [name, spec] of Object.entries(manifest[field] ?? {})) {
      if (typeof spec === 'string' && spec.startsWith('workspace:')) names.push(name);
    }
  }
  return names;
}

function indexPackages(treeRoot) {
  const byName = new Map();
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      if (entry.isDirectory()) {
        if (!SKIP_DIRS.has(entry.name)) walk(join(dir, entry.name));
        continue;
      }
      if (entry.name !== 'package.json') continue;
      const manifestPath = join(dir, entry.name);
      const name = JSON.parse(readFileSync(manifestPath, 'utf8')).name;
      if (typeof name === 'string' && name !== '' && !byName.has(name)) byName.set(name, dir);
    }
  };
  walk(treeRoot);
  return byName;
}

function relativeToRepo(dir) {
  const cut = dir.slice(REPO_ROOT.length + 1).split('\\').join('/');
  return cut === '' ? '.' : cut;
}

function closureForProject(project) {
  const roots = frontendRoots(project).filter((root) => root.membership === 'pnpm-closure');
  if (roots.length === 0) return [];
  const treeRoot = projectRoot(project);
  const byName = indexPackages(treeRoot);
  const wanted = new Set();
  const visited = new Set();
  const queue = roots.map((root) => (root.path === '.' ? treeRoot : join(treeRoot, root.path)));
  while (queue.length > 0) {
    const dir = queue.shift();
    if (visited.has(dir)) continue;
    visited.add(dir);
    const manifestPath = join(dir, 'package.json');
    if (!existsSync(manifestPath)) {
      throw new Error(`[sync-absorbed-frontend] ${project.id}: declared frontend root carries no package.json: ${dir}`);
    }
    wanted.add(dir);
    for (const name of workspaceDeps(manifestPath)) {
      const target = byName.get(name);
      if (!target) {
        throw new Error(
          `[sync-absorbed-frontend] ${project.id}: ${manifestPath} resolves \`${name}\` through a \`workspace:\` spec, ` +
            'and no package.json in that tree declares that name',
        );
      }
      queue.push(target);
    }
  }
  return [...wanted].map(relativeToRepo).sort();
}

function segment() {
  return {
    begin: `${SENTINEL} ${REGION_KEY} (generated; do not edit by hand)`,
    end: `${SENTINEL} ${REGION_KEY} end`,
  };
}

function render(hostText, paths) {
  const { begin, end } = segment();
  const body = [begin, ...paths.map((path) => `  - ${JSON.stringify(path)}`), end];
  const lines = hostText.split('\n');
  const start = lines.indexOf(begin);
  const stop = lines.indexOf(end);
  if (start !== -1 && stop > start) {
    return lines.slice(0, start).concat(body, lines.slice(stop + 1)).join('\n');
  }
  const anchor = lines.findIndex((line) => /^packages:/.test(line));
  if (anchor === -1) return lines.concat(['packages:'], body).join('\n');
  const head = lines.slice(0, anchor + 1);
  if (/^packages:\s*\[\s*\]\s*$/.test(head[anchor])) head[anchor] = 'packages:';
  return head.concat(body, lines.slice(anchor + 1)).join('\n');
}

export function syncAbsorbedFrontend({ check = false, quiet = false } = {}) {
  const hostText = existsSync(HOST_WORKSPACE) ? readFileSync(HOST_WORKSPACE, 'utf8') : 'packages:\n';
  const collected = [];
  let projects = 0;
  for (const project of absorbedProjects()) {
    const paths = closureForProject(project);
    if (paths.length === 0) continue;
    projects += 1;
    collected.push(...paths);
  }
  const unique = [...new Set(collected)].sort();
  const next = render(hostText, unique);
  const summary = `${unique.length} package(s) from ${projects} absorbed tree(s)`;
  if (next === hostText) {
    if (!quiet) console.log(`[sync-absorbed-frontend] up to date: ${summary}`);
    return { packages: unique.length, projects, changed: false };
  }
  if (check) {
    throw new Error(
      `[sync-absorbed-frontend] OUT OF DATE: host pnpm workspace does not match the absorbed manifests (${summary}). ` +
        'Run `node scripts/sync-absorbed-frontend.mjs` to rewrite it.',
    );
  }
  writeFileSync(HOST_WORKSPACE, next, 'utf8');
  if (!quiet) console.log(`[sync-absorbed-frontend] rewrote host pnpm workspace: ${summary}`);
  return { packages: unique.length, projects, changed: true };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    syncAbsorbedFrontend({ check: process.argv.includes('--check') });
    process.exit(0);
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}
