#!/usr/bin/env node
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { bundleDir, REPO_ROOT, WORK_DIR } from './desktop-config.mjs';
import { treeDidNotGrow } from './tauri-run.mjs';
import { chainGuaranteesOf } from './chain-order.mjs';
import {
  absorbedProjects,
  packageDirectory,
  projectRoot,
  stripRules,
} from './absorbed-sources.mjs';
import { resolveLanding, payloadDeclaration } from './miniapp-entry.mjs';
import { panoramaBinary, wrapperBinary } from './install-rustc-wrapper.mjs';
import { parseToml } from './sync-absorbed-members.mjs';
import { targetDir } from './target-dir.mjs';

export const SEAM_RUN_RECORD_DIR = 'lvpa-seam-run';

export const STEP_RECORD_DIR = 'lvpa-step-run';

export const REPORT_SUFFIX = 'ran-at.txt';

export function reportsOf(stepNames) {
  const held = new Set();
  for (const name of stepNames) {
    if (existsSync(reportAt(name))) held.add(name);
  }
  return held;
}

export const TAKEN_SUFFIX = 'taken-at.txt';

export const REPOSITORY_ENV = 'LVPA_REPO_ROOT';

const SENTINEL = '# [upstream-sync]';

const MARKER_OPEN = ' (generated; do not edit by hand)';

const SEGMENTED_FILES = () => [
  join(REPO_ROOT, 'Cargo.toml'),
  join(REPO_ROOT, 'pnpm-workspace.yaml'),
  join(REPO_ROOT, '.gitattributes'),
  join(resolveLanding(payloadDeclaration()), 'meta.json'),
];

function regionHead(body) {
  return body.endsWith(MARKER_OPEN) ? body.slice(0, -MARKER_OPEN.length) : body;
}

export function segments(text) {
  const held = new Map();
  let open = null;
  for (const line of text.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed.startsWith(SENTINEL)) continue;
    const body = trimmed.slice(SENTINEL.length).trim();
    if (open === null) {
      if (!body.endsWith(MARKER_OPEN)) continue;
      open = { head: regionHead(body), lines: [] };
      continue;
    }
    if (body === `${open.head} end`) {
      held.set(open.head, open.lines.join('\n'));
      open = null;
      continue;
    }
    open.lines.push(trimmed);
  }
  return held;
}

export function held() {
  const all = new Map();
  for (const path of SEGMENTED_FILES()) {
    for (const [head, body] of segments(readFileSync(path, 'utf8'))) all.set(head, body);
  }
  return all.size === 0 ? null : all;
}

function treeAt(directory) {
  const files = [];
  const walk = (at) => {
    for (const entry of readdirSync(at, { withFileTypes: true })) {
      if (entry.name === 'node_modules' || entry.name === '.git') continue;
      const nested = join(at, entry.name);
      if (entry.isDirectory()) {
        walk(nested);
        continue;
      }
      files.push(`${nested}:${statSync(nested).size}`);
    }
  };
  walk(directory);
  return files.length === 0 ? null : digestOf(files);
}

function directoryReads(directory) {
  for (const name of ['Cargo.toml', 'manifest.json', 'package.json']) {
    const path = join(directory, name);
    if (existsSync(path)) return readFileSync(path, 'utf8');
  }
  return null;
}

function absorbedTree(id) {
  const project = absorbedProjects().find((candidate) => candidate.id === id);
  if (project === undefined) return null;
  return project;
}

function backendManifest(project) {
  const crate = packageDirectory(project);
  if (crate === '') {
    return join(projectRoot(project), 'Cargo.toml');
  }
  return join(projectRoot(project), ...crate.split('/').filter(Boolean), 'Cargo.toml');
}

function takenRecord(project) {
  const root = projectRoot(project);
  if (!existsSync(root)) return null;
  const record = join(targetDir(), `${project.id}.${TAKEN_SUFFIX}`);
  if (!existsSync(record)) return null;
  const said = readFileSync(record, 'utf8').trim();
  return said === '' ? null : `${said}:${statSync(root).mtimeMs}`;
}

function treeOutOfLine(project) {
  const face = stripRules(project);
  const root = projectRoot(project);
  const left = [];
  const walk = (dir, where) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const full = join(dir, entry.name);
      const named = `${where}${entry.name}`;
      if (face.spares(entry, root)) {
        walk(full, `${named}/`);
        continue;
      }
      if (entry.isDirectory()) {
        if (!face.within(entry.name)) walk(full, `${named}/`);
        else left.push(`${named}/`);
        continue;
      }
      if (face.within(entry.name)) left.push(named);
    }
  };
  if (existsSync(root)) walk(root, '');
  return left;
}

export function settleTrees(projectId) {
  const record = join(targetDir(), `${projectId}.${TAKEN_SUFFIX}`);
  mkdirSync(dirname(record), { recursive: true });
  writeFileSync(record, `${projectId}:${Date.now()}\n`, 'utf8');
}

function digestOf(entries) {
  return entries.length === 0
    ? null
    : `${entries.length}:${createHash('sha256').update(entries.sort().join('\n')).digest('hex')}`;
}

const READS = new Map([
  ['obf-tree', () => READS.get('upstream')('openbitfun')],
  ['openalgo-desktop-tree', () => READS.get('upstream')('openalgo-desktop')],
  ['own-crates', (path) => READS.get('root')(path)],
  ['seam-crates', (path) => READS.get('root')(path)],
  ['patch-declarations', (path) => READS.get('root')(path)],
  ['host-manifest', (path) => READS.get('root-file')(path)],
  ['host-lock', (path) => READS.get('root-file')(path)],
  ['host-pnpm-workspace', (path) => READS.get('root-file')(path)],
  ['host-pnpm-lock', (path) => READS.get('root-file')(path)],
  ['host-gitattributes', (path) => READS.get('root-file')(path)],
  ['host-notice', (path) => READS.get('root-file')(path)],
  ['build-bench', (path) => READS.get('target')(path)],
  ['rustc-wrapper', () => (existsSync(wrapperBinary())
    ? null
    : `${wrapperBinary()} stands nowhere, so no wrapper build has landed`)],
  ['capability-panorama', () => (existsSync(panoramaBinary())
    ? null
    : `${panoramaBinary()} stands nowhere, so no wrapper build has landed`)],
  ['work-dir', () => (existsSync(WORK_DIR)
    ? null
    : `${WORK_DIR} stands nowhere, and this step reads the work directory every pass writes into`)],
  ['rustc-wrapper-build-dir', () => (existsSync(join(targetDir(), 'debug'))
    ? null
    : `${join(targetDir(), 'debug')} stands nowhere, so no wrapper build has landed`)],
  ['trees-carry-no-artifact', () => {
    const left = treeDidNotGrow();
    return left.length === 0 ? null : 'a build root stands inside an absorbed tree, and a tree carries static source alone';
  }],
  ['upstream', (id) => {
    const project = absorbedTree(id);
    if (project === undefined) {
      return `${id} is no declared absorbed tree, so nothing is taken into it`;
    }
    if (!existsSync(projectRoot(project))) {
      return `no run has taken ${project.root} yet, and this step reads the tree the take lands`;
    }
    const edited = treeOutOfLine(project);
    return edited.length === 0
      ? null
      : `${project.root} carries ${edited.join(', ')}, and that is not what ${project.url} carries on ${project.branch}, the ` +
        `branch NOTICE names: a tree is taken and never written into, so this step reads no copy that stands out of line with ` +
        `the branch it was taken from`;
  }],
  ['root', (path) => (existsSync(join(REPO_ROOT, ...path.split('/')))
    ? null
    : `${path} stands nowhere at the repository root, and this step reads the one copy there is`)],
  ['root-file', (path) => (existsSync(join(REPO_ROOT, ...path.split('/')))
    ? null
    : `${path} stands nowhere at the repository root, and this step reads the one copy there is`)],
  ['tree', (path) => {
    const project = absorbedTree(path);
    return project === undefined
      ? `${path} names no absorbed tree, and this step reads the tree the take lands`
      : READS.get('upstream')(project.id);
  }],
  ['target', (path) => (existsSync(join(targetDir(), ...path.split('/')))
    ? null
    : `${path} stands nowhere under ${targetDir()}, and this step reads what the build leaves there`)],
  ['member', (path) => {
    const [root, ...rest] = path.split('/');
    const project = absorbedTree(root);
    if (project === undefined) {
      return `${path} names no absorbed tree, and this step reads the crate it stands for`;
    }
    const manifest = rest.length === 0 ? backendManifest(project) : join(projectRoot(project), ...rest, 'Cargo.toml');
    if (!existsSync(manifest)) {
      return `${path} carries no Cargo.toml at ${manifest}, and this step reads the crate the mirror carries for it`;
    }
    try {
      parseToml(readFileSync(manifest, 'utf8'));
    } catch (error) {
      return `${manifest} does not read as TOML (${error.message}), and this step reads the crate it stands for`;
    }
    return null;
  }],
]);

const STANDS = new Map([
  ['root', (path) => (path.endsWith('/')
    ? treeAt(join(REPO_ROOT, ...path.split('/').filter((part) => part !== '')))
    : null)],
  ['tree', (path) => {
    const project = absorbedTree(path);
    return project === undefined ? null : treeAt(projectRoot(project));
  }],
  ['upstream-tree', (path) => {
    const project = absorbedTree(path);
    if (project === undefined) {
      return `${path} names no absorbed tree, and this step reads the tree the take lands`;
    }
    return takenRecord(project) === null
      ? `no run has taken ${project.root}, and this step reads the tree the take lands`
      : null;
  }],
  ['target', (path) => (existsSync(join(targetDir(), ...path.split('/')))
    ? treeAt(join(targetDir(), ...path.split('/')))
    : null)],
  ['target-file', (path) => (existsSync(path) ? `${path}:${statSync(path).size}` : null)],
  ['trees-carry-no-artifact', () => null],
  ['target-dir-entry', targetDirEntryStand],
  ['manifest-segment', manifestSegmentStand],
  ['record', recordStand],
  ['member', (path) => {
    const [root, ...rest] = path.split('/');
    const project = absorbedTree(root);
    if (project === undefined) return null;
    const directory = rest.length === 0
      ? (packageDirectory(project) === ''
        ? projectRoot(project)
        : join(projectRoot(project), ...packageDirectory(project).split('/').filter(Boolean)))
      : join(projectRoot(project), ...rest);
    const manifest = directoryReads(directory);
    if (manifest === null) return null;
    const kept = manifest.split(/\r?\n/).filter((row) => !/^\s*tauri\.conf\.json\s*$/.test(row)).join('\n');
    return segments(kept).length === 0 ? 'the manifest carries no workspace segment' : kept;
  }],
  ['miniapp', () => {
    try {
      const landing = resolveLanding(payloadDeclaration());
      const meta = join(landing, 'meta.json');
      return existsSync(meta) ? `${landing}:${digestOf(readdirSync(landing))}` : null;
    } catch (error) {
      return `unreadable: ${error.message}`;
    }
  }],
  ['work-dir', () => (existsSync(WORK_DIR) ? digestOf(readdirSync(WORK_DIR)) : null)],
  ['bundle-dir', () => (existsSync(bundleDir()) ? digestOf(readdirSync(bundleDir())) : null)],
  ['parent-of-work-dir', () => (existsSync(WORK_DIR) ? `${dirname(WORK_DIR)}:${readdirSync(dirname(WORK_DIR)).length}` : null)],
]);
const READ_KINDS = new Set(['report', 'root-file', ...STANDS.keys()]);
const READERS = new Map([
  ['obf-tree', ['openbitfun', 'report']],
  ['openalgo-desktop-tree', ['openalgo-desktop', 'report']],
  ['host-manifest', ['Cargo.toml', 'manifest-segment']],
  ['host-lock', ['Cargo.lock', 'root-file']],
  ['host-pnpm-workspace', ['pnpm-workspace.yaml', 'manifest-segment']],
  ['host-pnpm-lock', ['pnpm-lock.yaml', 'root-file']],
  ['host-gitattributes', ['.gitattributes', 'manifest-segment']],
  ['host-notice', ['NOTICE', 'root-file']],
  ['own-crates', ['crates/', 'root']],
  ['seam-crates', ['crates/seams/', 'root']],
  ['patch-declarations', ['patches/', 'root']],
  ['build-bench', ['lvpa-build-bench', 'target-dir-entry']],
  ['pnpm-store', ['lvpa-pnpm-store', 'target-dir-entry']],
  ['rustc-wrapper', [() => wrapperBinary(), 'target-file']],
  ['capability-panorama', [() => panoramaBinary(), 'target-file']],
  ['rustc-wrapper-build-dir', ['debug', 'target-dir-entry']],
  ['trees-carry-no-artifact', ['', 'trees-carry-no-artifact']],
  ['work-dir', ['', 'work-dir']],
  ['openalgo-desktop-backend', ['openalgo-desktop', 'member']],
  ['seam-run-record', ['seam-record', 'record']],
  ['step-record', ['step-record', 'record']],
  ['miniapp-payload', ['', 'miniapp']],
  ['bundle', ['', 'bundle-dir']],
]);

function readersOver(stepName, declared) {
  return declared.map((id) => {
    const reader = READERS.get(id);
    if (reader === undefined) {
      throw new Error(
        `[step-guarantees] ${stepName} names ${id} in crates/wiring/shape, and this file binds no reader that could say where it stands`,
      );
    }
    return [id, ...reader];
  });
}

let guards = null;

function requirementTable() {
  if (guards === null) {
    guards = new Map(
      [...chainGuaranteesOf()].map(([name, declared]) => [
        name,
        { onDisk: readersOver(name, declared.onDisk), leaves: readersOver(name, declared.leaves) },
      ]),
    );
  }
  return guards;
}

const TAKEN_TREES = new Set(['obf-tree', 'openalgo-desktop-tree']);

function producerOfTree(id) {
  for (const [name, held] of requirementTable()) {
    if (held.leaves.some(([leaf]) => leaf === id)) return name;
  }
  return undefined;
}

function nodeAheadOf(reading, id, stepName) {
  const producer = producerOfTree(id);
  const node = reading.nodeOf(producer, id);
  if (reading.outcomeOf(node) === 'landed') return false;
  return reading.path.indexOf(producer) > reading.path.indexOf(stepName);
}

function onDiskFor(stepName, reading) {
  const held = requirementTable().get(stepName);
  if (held === undefined) {
    throw new Error(
      `[step-guarantees] the shape names the step \`${stepName}\`, and Step::guarantee names no entry for it, so nothing says what that step refuses and what it leaves`,
    );
  }
  if (reading === undefined) return held.onDisk;
  const own = held.onDisk.filter(([id]) => TAKEN_TREES.has(id) && producerOfTree(id) === stepName);
  const later = held.onDisk.filter(([id]) => TAKEN_TREES.has(id) && producerOfTree(id) !== stepName && nodeAheadOf(reading, id, stepName));
  const dropped = [...own, ...later];
  if (later.length > 0 && dropped.length === held.onDisk.length) {
    throw new Error(
      `[step-guarantees] ${stepName} reads ${held.onDisk.map(([id]) => id).join(', ')}, and every one of them is a tree this ` +
        `pass takes after it: ${stepName} reads what the chain has already landed`,
    );
  }
  return held.onDisk.filter(([id]) => !dropped.some(([at]) => at === id));
}

export function leavesOf(stepName) {
  const held = requirementTable().get(stepName);
  if (held === undefined) {
    throw new Error(
      `[step-guarantees] the shape names the step \`${stepName}\`, and Step::guarantee names no entry for it, so nothing says what that step refuses and what it leaves`,
    );
  }
  return held;
}

export function guaranteesOf(stepName) {
  return leavesOf(stepName).leaves;
}

function readerOf(stepName, leaf) {
  const [id, at, kind] = leaf;
  if (!READ_KINDS.has(kind)) {
    throw new Error(
      `[step-guarantees] ${stepName} names ${id}, and ${kind} is no reader kind this declaration binds`,
    );
  }
  const sits = typeof at === 'function' ? at() : at;
  const stands = kind === 'report'
    ? reports
    : kind === 'root-file'
      ? rootFileStand
      : (said, where) => STANDS.get(kind)(said ?? where);
  return { at: sits, stands };
}

function reports(projectId) {
  const project = absorbedTree(projectId);
  if (project === undefined) return null;
  const taken = takenRecord(project);
  if (taken !== null) return taken;
  return existsSync(projectRoot(project)) ? `${project.root}:standing` : null;
}

function rootFileStand(path) {
  const file = join(REPO_ROOT, ...path.split('/'));
  if (!existsSync(file)) return null;
  const text = readFileSync(file, 'utf8');
  return `${file}:${text.length}:${createHash('sha256').update(text).digest('hex')}`;
}

function manifestSegmentStand(path) {
  const file = join(REPO_ROOT, ...path.split('/'));
  if (!existsSync(file)) return null;
  return digestOf([...segments(readFileSync(file, 'utf8')).keys()].sort());
}

function targetDirEntryStand(path) {
  const directory = join(targetDir(), path);
  const left = existsSync(directory)
    ? readdirSync(directory, { withFileTypes: true })
        .filter((entry) => entry.isFile())
        .map((entry) => `${entry.name}:${statSync(join(directory, entry.name)).size}`)
    : [];
  return left.length === 0 ? null : digestOf(left);
}

const RECORD_DIRS = new Map([
  ['seam-record', SEAM_RUN_RECORD_DIR],
  ['step-record', STEP_RECORD_DIR],
]);

function recordStand(id) {
  const directory = join(targetDir(), RECORD_DIRS.get(id));
  if (!existsSync(directory)) return null;
  const left = readdirSync(directory);
  return left.length === 0 ? null : digestOf(left);
}

export function beforeOf(stepName, leaves) {
  const was = new Map();
  for (const [id, ...rest] of leaves) {
    if (id === undefined) {
      throw new Error(
        `[step-guarantees] ${stepName} names a leaf this declaration binds no reader for: ${JSON.stringify(leaves)}`,
      );
    }
    const { at, stands } = readerOf(stepName, [id, ...rest]);
    was.set(id, stands(at));
  }
  return was;
}

function reportAt(stepName) {
  return join(targetDir(), STEP_RECORD_DIR, `${stepName}.${REPORT_SUFFIX}`);
}

export function stepRan(stepName, sequence) {
  const report = reportAt(stepName);
  mkdirSync(dirname(report), { recursive: true });
  writeFileSync(report, `${stepName}:${sequence}\n`, 'utf8');
}

export function guardOf(stepName, reading) {
  for (const [id, path] of onDiskFor(stepName, reading)) {
    const read = READS.get(id);
    if (read === undefined) {
      throw new Error(
        `[step-guarantees] ${stepName} names ${id} as what must stand before it runs, and this declaration holds no reader that could say whether it does`,
      );
    }
    const said = read(path);
    if (said !== null) return said;
  }
  return null;
}

export function landedOf(stepName, was) {
  const leaves = leavesOf(stepName).leaves;
  if (leaves.length === 0) {
    throw new Error(
      `${stepName} declares nothing it leaves behind, so this pass cannot tell the step having run from it not having run`,
    );
  }
  const changed = [];
  for (const [id, ...rest] of leaves) {
    const { at, stands } = readerOf(stepName, [id, ...rest]);
    const now = stands(at);
    if (now === null) {
      throw new Error(
        `${stepName} names ${id} as what it leaves, and nothing of it stands at ${at}, so this step cannot be told to have run`,
      );
    }
    if (now !== was.get(id)) changed.push(id);
  }
  if (changed.length === 0 && leaves.every(([, , kind]) => kind === 'report')) {
    throw new Error(
      `${stepName} ran and left no mark on what it declares it leaves (${leaves.map(([id]) => id).join(', ')}), ` +
        'so this pass leaves the disk as it found it and shows no sign of the step having run',
    );
  }
  return changed.length === 0 ? leaves.map(([id]) => id) : changed;
}