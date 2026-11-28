#!/usr/bin/env node
import { existsSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  REPO_ROOT,
  TREE_PACKAGE_MANIFEST,
  absorbedProjects,
  frontendDestinations,
  frontendMembership,
  frontendRoots,
  projectRoot,
  snapshotSections,
  stripRules,
} from './absorbed-sources.mjs';
import { payloadDeclaration, payloadProject, resolveLanding } from './miniapp-entry.mjs';

const HOST_WORKSPACE = join(REPO_ROOT, 'pnpm-workspace.yaml');
const DECLARATION = join(REPO_ROOT, 'scripts', 'miniapp-payload.json');
const SENTINEL = '# [upstream-sync]';
const REGION_KEY = 'absorbed frontend packages';

function readManifest(text, where) {
  try {
    return JSON.parse(text);
  } catch (error) {
    throw new Error(
      `[sync-absorbed-frontend] ${where} is not readable as JSON (${error.message}), so the closure cannot tell which ` +
        'packages that tree names',
    );
  }
}

function workspaceDeps(manifest, where) {
  const names = [];
  for (const field of ['dependencies', 'devDependencies', 'optionalDependencies', 'peerDependencies']) {
    for (const [name, spec] of Object.entries(manifest[field] ?? {})) {
      if (typeof spec === 'string' && spec.startsWith('workspace:')) names.push(name);
    }
  }
  return names;
}

function snapshotManifests(project) {
  const byName = new Map();
  for (const [where, section] of snapshotSections(project)) {
    if (where.split('/').pop() !== TREE_PACKAGE_MANIFEST) continue;
    const directory = where.includes('/') ? where.slice(0, where.lastIndexOf('/')) : '.';
    const manifest = readManifest(section.text, `${project.id}/${where}`);
    const name = manifest?.name;
    if (typeof name === 'string' && name !== '' && !byName.has(name)) {
      byName.set(name, { directory, manifest, where });
    }
  }
  return byName;
}

function miniappRoot() {
  const payload = payloadDeclaration();
  const landing = resolveLanding(payload);
  if (!landing.endsWith(`${payload.appId}`)) {
    throw new Error(
      `[sync-absorbed-frontend] ${DECLARATION} lands ${payload.appId} at ${landing}, and the landing names a MiniApp other ` +
        'than the one that card names',
    );
  }
  const declared = absorbedProjects()
    .flatMap((project) => frontendRoots(project)
      .filter((root) => frontendMembership(project, root) === 'mini-app-payload')
      .map((root) => ({ project, root })));
  if (declared.length !== 1) {
    throw new Error(
      `[sync-absorbed-frontend] the declaration names ${declared.length} frontend root(s) as \`mini-app-payload\` ` +
        `(${declared.map(({ project, root }) => `${project.id}: ${JSON.stringify(root)}`).join(', ') || 'none'}), and a host ` +
        'carries one MiniApp landing',
    );
  }
  const [{ project, root }] = declared;
  if (project.id !== payload.source || root !== payload.root) {
    throw new Error(
      `[sync-absorbed-frontend] ${DECLARATION} lands ${payload.source}'s ${JSON.stringify(payload.root)}, and the declaration ` +
        `names ${project.id}'s ${JSON.stringify(root)} as the \`mini-app-payload\`: one payload is one landing`,
    );
  }
  return root;
}
function relativeToRepo(path) {
  const cut = path.slice(REPO_ROOT.length + 1).split('\\').join('/');
  return cut === '' ? '.' : cut;
}

function closureForProject(project, landing) {
  const roots = frontendRoots(project);
  if (roots.length === 0) return [];
  const treeRoot = projectRoot(project);
  const byName = snapshotManifests(project);
  const byDirectory = new Map([...byName.values()].map((entry) => [entry.directory, entry]));
  const wanted = new Set();
  const visited = new Set();
  const queue = [];
  for (const root of roots) {
    if (root === landing.root && project.id === landing.id) continue;
    if (frontendMembership(project, root) !== 'pnpm-closure') continue;
    if (!byDirectory.has(root)) {
      throw new Error(
        `[sync-absorbed-frontend] ${project.id}: declared frontend root ${JSON.stringify(root)} carries no package.json in ` +
          'the snapshot the take wrote, so the closure has nothing to start from there',
      );
    }
    queue.push(root);
  }
  while (queue.length > 0) {
    const asked = queue.shift();
    const entry = byByNameOrDirectory(byName, byDirectory, asked);
    if (entry === undefined) {
      throw new Error(
        `[sync-absorbed-frontend] ${project.id}: the snapshot carries no package.json for ${JSON.stringify(asked)}, and the ` +
          'closure reads every package it carries out of the snapshot',
      );
    }
    if (visited.has(entry.directory)) continue;
    visited.add(entry.directory);
    wanted.add(entry.directory);
    for (const name of workspaceDeps(entry.manifest, `${project.id}/${entry.where}`)) {
      if (!byName.has(name)) {
        throw new Error(
          `[sync-absorbed-frontend] ${project.id}: ${entry.where} resolves \`${name}\` through a \`workspace:\` spec, and no ` +
            'package.json in that tree declares that name',
        );
      }
      queue.push(name);
    }
  }
  return [...wanted]
    .map((directory) => relativeToRepo(join(treeRoot, ...directory.split('/').filter((part) => part !== '.'))))
    .sort();
}

function byByNameOrDirectory(byName, byDirectory, asked) {
  return byName.get(asked) ?? byDirectory.get(asked);
}

function segment(key = REGION_KEY) {
  return {
    begin: `${SENTINEL} ${key} (generated; do not edit by hand)`,
    end: `${SENTINEL} ${key} end`,
  };
}

function renderRegion(lines, key, rows) {
  const { begin, end } = segment(key);
  const body = [begin, ...rows, end];
  const start = lines.indexOf(begin);
  const stop = lines.indexOf(end);
  if (start !== -1 && stop > start) return lines.slice(0, start).concat(body, lines.slice(stop + 1));
  const anchor = lines.findIndex((line) => /^packages:/.test(line));
  if (anchor === -1) return lines.concat(['packages:'], body);
  const head = lines.slice(0, anchor + 1);
  if (/^packages:\s*\[\s*\]\s*$/.test(head[anchor])) head[anchor] = 'packages:';
  return head.concat(body, lines.slice(anchor + 1));
}

function render(hostText, paths) {
  const lines = renderRegion(hostText.split('\n'), REGION_KEY, paths.map((path) => `  - ${JSON.stringify(path)}`));
  return lines.join('\n');
}

function statedPaths(hostText) {
  const lines = hostText.split('\n');
  const start = lines.indexOf(segment().begin);
  const stop = lines.indexOf(segment().end);
  if (start === -1 || stop <= start) return [];
  const held = [];
  for (const line of lines.slice(start + 1, stop)) {
    const quoted = line.trim().match(/^-\s*"([^"]+)"$/);
    if (quoted !== null) held.push(quoted[1]);
  }
  return held;
}

export function syncAbsorbedFrontend({ check = false, quiet = false } = {}) {
  if (!existsSync(HOST_WORKSPACE)) {
    throw new Error(`[sync-absorbed-frontend] ${HOST_WORKSPACE} stands nowhere, so no closure is in place to compare against`);
  }
  const read = readFileSync(HOST_WORKSPACE, 'utf8');
  const projects = absorbedProjects();
  const carrier = payloadProject(payloadDeclaration());
  const landing = { id: carrier.id, root: miniappRoot() };
  const adopted = [...new Set([landing.root, ...frontendRoots(carrier)])]
    .map((entry) => join(carrier.root, ...entry.split('/')).split('\\').join('/'));
  const collected = [];
  const elsewhere = [];
  let carried = 0;
  for (const project of projects) {
    for (const destination of frontendDestinations(project)) {
      if (destination.membership !== 'pnpm-closure') elsewhere.push(destination);
    }
    const paths = closureForProject(project, landing);
    if (paths.length === 0) continue;
    carried += 1;
    collected.push(...paths);
  }
  const unique = [...new Set(collected)].sort();
  const written = statedPaths(read);
  const outside = adopted.filter((path) => unique.includes(path));
  if (outside.length > 0) {
    throw new Error(
      `[sync-absorbed-frontend] ${outside.join(', ')} is reached from a frontend root of ${landing.id} and is also a root the ` +
        'one frontend closure carries, and one root is resolved by one set of packages',
    );
  }
  const misplaced = elsewhere.filter((entry) => written.includes(entry.path));
  if (misplaced.length > 0) {
    throw new Error(
      `[sync-absorbed-frontend] ${misplaced
        .map((entry) => `FRONTEND: ${entry.path} is ${entry.membership} and stands under ${segment().begin} of pnpm-workspace.yaml`)
        .join('; ')}`,
    );
  }
  const nextText = render(read, unique);
  const summary = `${unique.length} package(s) from ${carried} absorbed tree(s)`;
  if (nextText === read) {
    if (!quiet) console.log(`[sync-absorbed-frontend] up to date: ${summary}`);
    return { packages: unique.length, projects: carried, changed: false };
  }
  if (check) {
    throw new Error(
      `[sync-absorbed-frontend] OUT OF DATE: host pnpm workspace does not match the absorbed manifests (${summary}). ` +
        'Run `node scripts/sync-absorbed-frontend.mjs` to rewrite it.',
    );
  }
  writeFileSync(HOST_WORKSPACE, nextText, 'utf8');
  if (!quiet) console.log(`[sync-absorbed-frontend] rewrote host pnpm workspace: ${summary}`);
  return { packages: unique.length, projects: carried, changed: true };
}

export function refusedProjectArtifacts(project) {
  const face = stripRules(project);
  const root = projectRoot(project);
  const standing = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const full = join(dir, entry.name);
      if (face.spares(entry, root)) {
        walk(full);
        continue;
      }
      if (face.within(entry.name)) {
        standing.push(relativeToRepo(full));
        continue;
      }
      if (entry.isDirectory()) walk(full);
    }
  };
  if (existsSync(root)) walk(root);
  return standing;
}

export function refuseProjectArtifacts(projects) {
  const left = projects.flatMap((project) => refusedProjectArtifacts(project));
  if (left.length === 0) return;
  throw new Error(
    `[sync-absorbed-frontend] ${left.join(', ')} stands in an absorbed tree, and no run removes an artifact from a tree: a ` +
      'tree is taken whole and never written into, so a copy that stands is the copy, and a strip rule face removes only what ' +
      'the take wrote on its way in',
  );
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
