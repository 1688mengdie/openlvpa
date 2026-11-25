#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, sep } from 'node:path';
import {
  REPO_ROOT,
  absorbedProjects,
  packageDirectoryPath,
  projectRoot,
  snapshotPath,
  stripManifestPath,
} from './absorbed-sources.mjs';
import { syncAbsorbedMembers } from './sync-absorbed-members.mjs';
import { syncAbsorbedFrontend } from './sync-absorbed-frontend.mjs';
import { syncOpenAlgoFrontend } from './sync-openalgo-frontend.mjs';

const NOTICE_PATH = join(REPO_ROOT, 'NOTICE');
const TAKE_PARENT = join(REPO_ROOT, '.openbitfun');
const PATCH_DIR = join(REPO_ROOT, 'patches');
const SNAPSHOT_MANIFEST = 'Cargo.toml';

function fail(message) {
  process.stderr.write(`[sync-upstream] ${message}\n`);
  process.exit(1);
}

function run(args, options = {}) {
  const result = spawnSync(args[0], args.slice(1), {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
    ...options,
  });
  if (result.error) fail(`${args.join(' ')} could not start: ${result.error.message}`);
  return result;
}

function must(args, options) {
  const result = run(args, options);
  if (result.status !== 0) {
    fail(`${args.join(' ')} exited ${result.status}: ${(result.stderr || '').trim()}`);
  }
  return result;
}

function noticeDeclarations() {
  const text = readFileSync(NOTICE_PATH, 'utf8');
  const remotes = [];
  const branches = [];
  for (const line of text.split(/\r?\n/)) {
    const remote = line.match(/^\s*Remote:\s*(\S+)\s*$/);
    if (remote) remotes.push(remote[1]);
    const branch = line.match(/^\s*Branch:\s*(\S+)\s*$/);
    if (branch) branches.push(branch[1]);
  }
  return { remotes, branches };
}

function compareWithNotice(projects) {
  const { remotes, branches } = noticeDeclarations();
  if (remotes.length !== projects.length || branches.length !== projects.length) {
    fail(
      `NOTICE declares ${remotes.length} Remote line(s) and ${branches.length} Branch line(s), ` +
        `the declaration carries ${projects.length} project(s). Neither is the other's answer.`,
    );
  }
  projects.forEach((project, index) => {
    if (project.url !== remotes[index]) {
      fail(`NOTICE Remote #${index + 1} is ${JSON.stringify(remotes[index])}, ${project.id} declares ${JSON.stringify(project.url)}`);
    }
    if (project.branch !== branches[index]) {
      fail(`NOTICE Branch #${index + 1} is ${JSON.stringify(branches[index])}, ${project.id} declares ${JSON.stringify(project.branch)}`);
    }
  });
}

function probe(project) {
  const result = run(['git', 'ls-remote', '--exit-code', '--heads', project.url, project.branch]);
  if (result.status !== 0) {
    fail(`${project.url} (${project.branch}) answered no such branch, so ${project.id} was left as it is`);
  }
}

function takeRoot(project) {
  return join(TAKE_PARENT, `.${project.id}-take-${process.pid}`);
}

function landTree(project) {
  const landing = takeRoot(project);
  rmSync(landing, { recursive: true, force: true });
  mkdirSync(TAKE_PARENT, { recursive: true });
  const config = ['-c', 'core.autocrlf=false', '-c', 'core.eol=lf'];
  must(['git', ...config, 'clone', '--depth', '1', '--single-branch', '--branch', project.branch, project.url, landing]);
  must(['git', ...config, '-C', landing, 'checkout', '--force', '--', '.']);
  return landing;
}

function snapshotManifest(project, landing) {
  const manifest = join(landing, project.sync?.packageDirectory ?? '', SNAPSHOT_MANIFEST);
  if (!existsSync(manifest)) {
    fail(`the taken tree carries no ${project.sync?.packageDirectory ?? ''}${SNAPSHOT_MANIFEST} to snapshot`);
  }
  writeFileSync(snapshotPath(project), readFileSync(manifest));
}

function stripRules(project) {
  const face = JSON.parse(readFileSync(stripManifestPath(project), 'utf8'));
  for (const key of [
    'workspaceTablePattern',
    'workspaceKeyPattern',
    'excludedFileNames',
    'excludedDirNames',
    'excludedFilePrefixes',
    'excludedFileSuffixes',
    'tables',
    'workspaceTables',
  ]) {
    if (face[key] === undefined) {
      fail(`${project.stripManifest} carries no \`${key}\`, so the strip would act on a rule set that is not there`);
    }
  }
  return {
    workspaceTable: new RegExp(face.workspaceTablePattern),
    workspaceKey: new RegExp(face.workspaceKeyPattern),
    tables: face.tables.map(tableRule),
    workspaceTables: face.workspaceTables.map(tableRule),
    files: new Set(face.excludedFileNames),
    dirs: new Set(face.excludedDirNames),
    prefixes: face.excludedFilePrefixes,
    suffixes: face.excludedFileSuffixes,
  };
}

function tableHeader(line) {
  const header = line.replace(/\r$/, '').match(/^\s*(\[\[?)([^\]]+)(\]\]?)\s*$/);
  return header === null ? null : `${header[1]}${header[2].trim()}]`;
}

function tableRule(name) {
  if (!name.endsWith('*]')) return name;
  const head = name.slice(0, -2);
  return { from: head.endsWith('.') ? head : `${head}.`, to: ']' };
}

function namesTable(header, names) {
  return names.some((rule) => (typeof rule === 'string'
    ? rule === header
    : header.startsWith(rule.from) && header.endsWith(rule.to)));
}

function dropTables(text, names) {
  const kept = [];
  const dropped = [];
  let inside = false;
  for (const line of text.split('\n')) {
    const header = tableHeader(line);
    if (header !== null) {
      inside = namesTable(header, names);
      if (inside) dropped.push(header);
    }
    if (!inside) kept.push(line);
  }
  return dropped.length === 0 ? null : { text: kept.join('\n'), dropped };
}

function relativeFrom(root, target) {
  return relative(root, target).split(sep).join('/');
}

function stripTree(project, landing) {
  const face = stripRules(project);
  const removed = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const full = join(dir, entry.name);
      const path = relativeFrom(landing, full);
      if (entry.isDirectory()) {
        if (face.dirs.has(entry.name)) {
          rmSync(full, { recursive: true, force: true });
          removed.push(`${path}/`);
          continue;
        }
        walk(full);
        continue;
      }
      if (!entry.isFile()) continue;
      const dropFile =
        face.files.has(entry.name) ||
        face.prefixes.some((prefix) => entry.name.startsWith(prefix)) ||
        face.suffixes.some((suffix) => entry.name.endsWith(suffix));
      if (dropFile) {
        rmSync(full, { force: true });
        removed.push(path);
        continue;
      }
      if (entry.name === 'Cargo.toml') {
        const text = readFileSync(full, 'utf8');
        const stripped = dropTables(text, [...face.workspaceTables, ...face.tables]);
        if (stripped !== null) {
          const residue = stripped.text.split('\n').filter((line) => line.trim() !== '' && !line.trim().startsWith('#'));
          if (residue.length === 0) {
            rmSync(full, { force: true });
            removed.push(`${path}: ${stripped.dropped.join(' ')} (nothing but the graph root stood there)`);
            continue;
          }
          if (!/^\s*\[(package|lib|bin|example|test|bench)\]/.test(residue.join('\n'))) {
            fail(`${path}: stripping ${stripped.dropped.join(' ')} leaves text that names no package, so the identity card this file exists to be is gone`);
          }
          writeFileSync(full, stripped.text);
          removed.push(`${path}: ${stripped.dropped.join(' ')}`);
        }
        continue;
      }
      if (entry.name === 'package.json') {
        const kept = dropKeys(readFileSync(full, 'utf8'), face.workspaceKey, path);
        if (kept !== null) {
          writeFileSync(full, kept);
          removed.push(`${path}: workspaces`);
        }
      }
    }
  };
  walk(landing);
  return removed;
}

function dropKeys(text, pattern, path) {
  const kept = text.split('\n').filter((line) => !pattern.test(line));
  try {
    JSON.parse(kept.join('\n'));
  } catch (error) {
    fail(`${path}: dropping its workspace declaration leaves text this reader cannot place: ${error.message}`);
  }
  return kept.join('\n') === text ? null : kept.join('\n');
}

function installTree(project) {
  const destination = projectRoot(project);
  const landing = takeRoot(project);
  if (!existsSync(landing)) {
    fail(`${project.id}: the take this install reads is not standing at ${landing}`);
  }
  rmSync(destination, { recursive: true, force: true });
  mkdirSync(dirname(destination), { recursive: true });
  cpSync(landing, destination, { recursive: true, verbatimSymlinks: true });
}

function mergeLock(project) {
  const lock = join(packageDirectoryPath(project), 'Cargo.lock');
  if (!existsSync(lock)) return;
  const body = readFileSync(lock, 'utf8');
  if (!body.startsWith('# This file is automatically @generated by Cargo.')) {
    fail(`${project.id}: the taken lock does not open on the tree's own lockfile`);
  }
  rmSync(lock, { force: true });
}

function patchFiles() {
  if (!existsSync(PATCH_DIR)) return [];
  return readdirSync(PATCH_DIR)
    .filter((name) => name.endsWith('.patch.json'))
    .sort()
    .map((name) => join(PATCH_DIR, name));
}

function applyPatches(project) {
  for (const path of patchFiles()) {
    const patch = JSON.parse(readFileSync(path, 'utf8'));
    if (patch.landed !== undefined) continue;
    if (typeof patch.file !== 'string' || !patch.file.startsWith(`${project.root}/`)) continue;
    const target = join(REPO_ROOT, ...patch.file.split('/'));
    if (!existsSync(target)) {
      fail(`${path} targets ${patch.file}, which the tree just taken does not carry`);
    }
    let text = readFileSync(target, 'utf8');
    for (const replacement of patch.replacements ?? []) {
      if (replacement.find === replacement.replace) {
        fail(`${path} has the same find and replace, so it changes no byte`);
      }
      if (replacement.find === '') {
        fail(`${path} spells a file whole in its replacement, and the take writes no file from nothing`);
      }
      const hits = text.split(replacement.find).length - 1;
      if (hits === 0) fail(`${path} finds no occurrence of its find text in ${patch.file}`);
      if (hits > 1) fail(`${path} finds ${hits} occurrences of its find text in ${patch.file}, and one anchor names one place`);
      text = text.split(replacement.find).join(replacement.replace);
    }
    writeFileSync(target, text);
  }
}

function main() {
  const projects = absorbedProjects();
  compareWithNotice(projects);
  projects.forEach(probe);

  const taken = projects.map((project) => ({ project, landing: landTree(project) }));

  for (const { project, landing } of taken) snapshotManifest(project, landing);

  syncAbsorbedMembers({ check: false });
  syncAbsorbedFrontend({ check: false });

  for (const { project, landing } of taken) {
    stripTree(project, landing);
    mergeLock(project);
    installTree(project);
    applyPatches(project);
  }

  for (const { project } of taken) rmSync(takeRoot(project), { recursive: true, force: true });

  for (const line of syncOpenAlgoFrontend()) {
    process.stdout.write(
      `[sync-upstream] landed ${line.id} at ${line.target}: ${line.projected} projected file(s), ` +
        `${line.patched.length} patched file(s)\n`,
    );
  }

  process.stdout.write(`[sync-upstream] took ${projects.map((p) => p.id).join(', ')}\n`);
}

main();
