#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  REPO_ROOT,
  TREE_MANIFEST,
  TREE_PACKAGE_MANIFEST,
  absorbedProjects,
  buildConfigFiles,
  lockPath,
  manifestCapture,
  packageDirectory,
  packageDirectoryPath,
  projectRoot,
  snapshotDocument,
  snapshotPath,
  stripRules,
  strippedAbsence,
  strippedDirectoryPath,
  strippedRootOf,
} from './absorbed-sources.mjs';
import { chainOrder } from './chain-order.mjs';
import { parseToml, releaseLockVersions, syncAbsorbedMembers } from './sync-absorbed-members.mjs';
import { syncAbsorbedFrontend, refuseProjectArtifacts as refuseProjectArtifactsOf } from './sync-absorbed-frontend.mjs';
import { syncOpenAlgoFrontend } from './sync-openalgo-frontend.mjs';
import {
  LANDINGS,
  payloadDeclaration,
  payloadRootPath,
  readPatchDeclarations,
} from './miniapp-entry.mjs';

const NOTICE_PATH = join(REPO_ROOT, 'NOTICE');
const TAKE_PARENT = join(REPO_ROOT, '.openbitfun');
const CONVERGENCE = join(REPO_ROOT, 'scripts', 'single-instance.toml');
const DESHELL_FACE = join(REPO_ROOT, 'scripts', 'deshell-manifest.json');

const LANDING_COPIES = new Map([
  ['frontend', () => absorbedProjects()],
  ['member', () => absorbedProjects().filter((project) => packageDirectory(project) !== '')],
  ['create', () => absorbedProjects()],
]);

function landingCopies(name, path) {
  if (!LANDINGS.includes(name)) {
    fail(`${path} names a landing this run does not carry: ${LANDINGS.join(', ')}`);
  }
  return LANDING_COPIES.get(name)();
}

function tomlEntries(path, table) {
  const text = readFileSync(path, 'utf8');
  const entries = [];
  let current = null;
  let inside = false;
  for (const line of text.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (trimmed.startsWith('[[')) {
      inside = trimmed === `[[${table}]]`;
      if (inside) {
        current = {};
        entries.push(current);
      }
      continue;
    }
    if (!inside || trimmed === '' || trimmed.startsWith('#')) continue;
    const key = line.match(/^\s*([A-Za-z0-9_-]+)\s*=\s*"([^"]*)"\s*$/);
    if (key === null) {
      fail(`${path}: no place for this line under [[${table}]]: ${line}`);
    }
    current[key[1]] = key[2];
  }
  return entries;
}

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

const TAKE_RESIDUE = /^\..+-take-\d+$/;

function takenDirectories() {
  if (!existsSync(TAKE_PARENT)) return [];
  return readdirSync(TAKE_PARENT, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && TAKE_RESIDUE.test(entry.name))
    .map((entry) => join(TAKE_PARENT, entry.name));
}

function retireTakeRoots(taken) {
  const thisRun = taken.map(({ project }) => takeRoot(project));
  const ordered = [...thisRun, ...takenDirectories().filter((path) => !thisRun.includes(path))];
  for (const path of ordered) {
    rmSync(path, { recursive: true, force: true });
  }
  return ordered;
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

const SNAPSHOT_SKIP = new Set(['node_modules', 'target', '.git', 'dist', 'build']);

const TREE_MANIFESTS = new Set([TREE_MANIFEST, TREE_PACKAGE_MANIFEST]);

function copyTree(from, into) {
  mkdirSync(into, { recursive: true });
  for (const entry of readdirSync(from, { withFileTypes: true })) {
    const source = join(from, entry.name);
    const destination = join(into, entry.name);
    if (entry.isDirectory()) {
      copyTree(source, destination);
      continue;
    }
    if (entry.isFile()) writeFileSync(destination, readFileSync(source));
  }
}

function treeDigests(root, face, digests = new Map(), where = '') {
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    const path = join(root, entry.name);
    const named = `${where}${entry.name}`;
    if (entry.isDirectory()) {
      if (face.spares(entry, root)) treeDigests(path, face, digests, `${named}/`);
      continue;
    }
    if (entry.isFile()) digests.set(named, createHash('sha256').update(readFileSync(path)).digest('hex'));
  }
  return digests;
}

function changedAgainst(project, landing) {
  const taken = treeDigests(landing, stripRules(project));
  const standing = projectRoot(project);
  if (!existsSync(standing)) return [];
  const current = treeDigests(standing, stripRules(project));
  const changed = [...taken.keys()].filter((where) => current.get(where) !== taken.get(where));
  for (const where of current.keys()) {
    if (!taken.has(where)) changed.push(where);
  }
  return changed.sort();
}

function refuseAnEditedTree(project, landing) {
  const edited = changedAgainst(project, landing);
  if (edited.length === 0) return;
  fail(
    `${project.id} already stands at ${projectRoot(project)} and ${edited.length} of its file(s) are not the bytes ` +
      `${project.url} carries on ${project.branch}, the branch NOTICE names: ${edited.join(', ')}. A tree is taken and never ` +
      'written into, so the copy on disk answers to no commit of the declared branch and this run will not land on it. Remove ' +
      `${projectRoot(project)} whole, and the next take writes that branch there anew`,
  );
}

const MISSED_CARD_TAG = 'lvpa-missed-cards:';

function missedCards(error) {
  const at = String(error?.message ?? '').indexOf(MISSED_CARD_TAG);
  if (at === -1) return null;
  return String(error.message).slice(at + MISSED_CARD_TAG.length).trim();
}

function readToml(path, project, landing) {
  const text = readFileSync(path, 'utf8');
  try {
    parseToml(text);
  } catch (error) {
    fail(`${relativeFrom(landing, path)} does not read as TOML (${error.message}), so the take of ${project.id} is not that tree's own manifest`);
  }
  return text;
}

function readJson(path, project, landing) {
  const text = readFileSync(path, 'utf8');
  try {
    JSON.parse(text);
  } catch (error) {
    fail(`${relativeFrom(landing, path)} does not read as JSON (${error.message}), so the take of ${project.id} is not that tree's own manifest`);
  }
  return text;
}

function snapshotManifest(project, landing) {
  const documents = [];
  const buildInputs = new Set(
    strippedAbsence(project) === null
      ? []
      : buildConfigFiles(project).filter((name) => !TREE_MANIFESTS.has(name.split('/').pop()) && deshellBlockFiles().includes(name)),
  );
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      if (entry.isDirectory()) {
        if (!SNAPSHOT_SKIP.has(entry.name)) walk(join(dir, entry.name));
        continue;
      }
      const path = join(dir, entry.name);
      const where = relativeFrom(landing, path);
      if (TREE_MANIFESTS.has(entry.name)) {
        documents.push(entry.name === TREE_MANIFEST
          ? { where, kind: 'toml', text: readToml(path, project, landing) }
          : { where, kind: 'json', text: readJson(path, project, landing) });
        continue;
      }
      if (!buildInputs.has(where)) continue;
      documents.push({ where, kind: 'toml', text: readToml(path, project, landing) });
    }
  };
  walk(landing);
  const captured = new Set(documents.map((document) => document.where));
  let outside = [];
  try {
    outside = manifestCapture(project).filter((where) => !captured.has(where));
  } catch (error) {
    outside = missedCards(error);
    if (outside === null) throw error;
    outside = outside.split(', ').filter((where) => !captured.has(where));
  }
  if (outside.length > 0) {
    throw new Error(
      `${project.id}: ${outside.length} identity card(s) the take lands are read by the merge and stand outside the ` +
        `snapshot: ${outside.join(', ')}. The strip removes every one of them, so the merge would read a file the snapshot ` +
        'does not carry',
    );
  }
  for (const where of buildInputs) {
    if (captured.has(where)) continue;
    throw new Error(
      `${project.id}: ${where} is a build input the deshell step reads a block out of, and the snapshot carries no segment ` +
        'for it: a shell face with no reading face is a removal this step never takes',
    );
  }
  writeSnapshot(project, documents);
  return documents;
}

function writeSnapshot(project, documents) {
  const standing = snapshotPath(project);
  writeFileSync(standing, snapshotDocument(project, documents, strippedAbsence(project)), 'utf8');
  const after = readFileSync(standing);
  if (after.includes(0xfe) || after.includes(0xff)) {
    fail(`${standing} was written as UTF-16, and a snapshot that a reader cannot open is not one`);
  }
}

function relativeFrom(root, target) {
  return relative(root, target).split(sep).join('/');
}

function tableHeader(line) {
  const header = line.replace(/\r$/, '').match(/^\s*(\[\[?)([^\]]+)(\]\]?)\s*$/);
  return header === null ? null : `${header[1]}${header[2].trim()}]`;
}

function lockSections(text) {
  const lines = text.split('\n');
  const sections = [];
  for (const line of lines) {
    if (tableHeader(line) !== null) {
      sections.push({ header: line.trim(), lines: [line] });
      continue;
    }
    if (sections.length !== 0) sections[sections.length - 1].lines.push(line);
  }
  return sections;
}

function lockFields(section) {
  const fields = {};
  for (const line of section.lines.slice(1)) {
    const match = line.match(/^\s*([A-Za-z0-9_-]+)\s*=\s*(.*?)\s*$/);
    if (match !== null) fields[match[1]] = match[2];
  }
  return fields;
}

function quoted(value, where) {
  const text = value === undefined ? null : value.match(/^"(.*)"$/);
  if (text === null) {
    fail(`a lock entry this step cannot place carries no quoted name or version: ${where}`);
  }
  return text[1];
}

function lockKey(section) {
  if (section.header !== '[[package]]') return null;
  const fields = lockFields(section);
  return `${section.header}\u0000${quoted(fields.name, section.header)}\u0000${quoted(fields.version, section.header)}`;
}

function mergeLocks(projects) {
  const host = lockPath(projects[0]);
  const hostText = readFileSync(host, 'utf8');
  if (!hostText.startsWith('# This file is automatically @generated by Cargo.')) {
    fail(`${host} does not open as the one lock of the mirror graph`);
  }
  const hostVersions = new Map();
  for (const section of lockSections(hostText).filter((entry) => entry.header === '[[package]]')) {
    const fields = lockFields(section);
    const name = quoted(fields.name, section.header);
    if (!hostVersions.has(name)) hostVersions.set(name, new Set());
    hostVersions.get(name).add(quoted(fields.version, section.header));
  }
  const settled = new Map();
  for (const entry of tomlEntries(CONVERGENCE, 'converged')) settled.set(entry.crate, entry.into);
  const taken = [];
  for (const project of projects) {
    const path = join(packageDirectoryPath(project), 'Cargo.lock');
    if (!existsSync(path)) continue;
    const text = readFileSync(path, 'utf8');
    if (!text.startsWith('# This file is automatically @generated by Cargo.')) {
      fail(`${project.id}: the taken lock does not open on the tree's own lockfile`);
    }
    const sections = lockSections(text);
    if (sections.filter((section) => section.header === '[[package]]').length === 0) {
      fail(`${project.id}: the taken lock names no [[package]], so it carries no resolution to merge`);
    }
    taken.push({ project, path, sections });
  }
  if (taken.length === 0) return;
  const versions = new Map();
  for (const [name, held] of hostVersions) versions.set(name, [...held]);
  for (const { project, sections } of taken) {
    for (const section of sections.filter((entry) => entry.header === '[[package]]')) {
      const fields = lockFields(section);
      const name = quoted(fields.name, section.header);
      const version = quoted(fields.version, section.header);
      const carried = versions.get(name) ?? [];
      if (carried.includes(version)) continue;
      if (!settled.has(name)) {
        fail(
          `${project.id}: the taken lock resolves \`${name}\` to ${version} and ${carried.length === 0
            ? 'no lock carries it'
            : `the one lock resolves it to ${carried.join(', ')}`}; ${CONVERGENCE} settles no value for it, ` +
          'so neither reading is the one the mirror carries and this step will not choose',
        );
      }
      versions.set(name, [...carried, version]);
    }
  }
  const released = releaseLockVersions(hostText, (name) => settled.get(name));
  const present = new Set(lockSections(released.text).sections.map(lockKey));
  const missing = taken
    .flatMap(({ sections: from }) => from)
    .filter((section) => !present.has(lockKey(section)));
  for (const { path } of taken) rmSync(path, { force: true });
  if (missing.length === 0 && released.dropped.length === 0) return;
  const grown = [
    released.text,
    ...missing.flatMap((section) => [...section.lines, '']),
  ].join('\n');
  writeFileSync(host, grown);
}

function writeTree(project, landing) {
  const destination = projectRoot(project);
  if (existsSync(destination)) {
    fail(
      `${project.id} stands at ${destination} and this run takes ${project.url} on ${project.branch}: a take writes a tree ` +
        'where none stands and never over one that does, so a copy that stands is the copy before this run and no run reaches ' +
        'into it',
    );
  }
  mkdirSync(dirname(destination), { recursive: true });
  copyTree(landing, destination);
}

function take(projects) {
  const taken = [];
  process.on('exit', () => retireTakeRoots(taken));
  for (const project of projects) taken.push({ project, landing: landTree(project) });
  for (const { project, landing } of taken) refuseAnEditedTree(project, landing);
  for (const { project, landing } of taken) snapshotManifest(project, landing);
  return { projects, taken };
}

function merge(projects) {
  syncAbsorbedMembers({ check: false });
  syncAbsorbedFrontend({ check: false });
  mergeLocks(projects);
  process.stdout.write(
    `[sync-upstream] merge: ${projects.map((project) => project.id).join(', ')} speak from the repository root alone\n`,
  );
}

function packageManifests(root) {
  const found = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      if (entry.name === 'node_modules' || entry.name === 'target' || entry.name === '.git') continue;
      const path = join(dir, entry.name);
      if (entry.isDirectory()) walk(path);
      else if (entry.name === 'package.json') found.push(path);
    }
  };
  walk(root);
  return found;
}

function strip() {
  const projects = absorbedProjects();
  const removed = [];
  const left = [];
  for (const project of projects) {
    const face = stripRules(project);
    const root = projectRoot(project);
    const cards = identityCardNames(project);
    const walk = (dir) => {
      for (const entry of readdirSync(dir, { withFileTypes: true })) {
        const full = join(dir, entry.name);
        if (face.spares(entry, root)) {
          walk(full);
          continue;
        }
        if (!face.within(entry.name) || cards.includes(entry.name)) {
          if (entry.isDirectory()) walk(full);
          continue;
        }
        rmSync(full, { recursive: true, force: true });
        if (existsSync(full)) left.push(relativeFrom(root, full));
        else removed.push(relativeFrom(root, full));
      }
    };
    walk(root);
  }
  if (left.length > 0) {
    fail(`the strip could not remove ${left.join(', ')} from the absorbed trees, and each stands as an artifact the rule face names`);
  }
  const declared = [];
  for (const project of projects) {
    for (const path of packageManifests(projectRoot(project))) {
      const text = readFileSync(path, 'utf8');
      if (/^\s*\[workspace[.\]]/m.test(text) || /"workspaces"\s*:/.test(text)) declared.push(path);
    }
  }
  if (declared.length > 0) {
    fail(`a workspace declaration still stands inside the absorbed trees after the merge: ${declared.join(', ')}`);
  }
  const standing = [];
  for (const project of projects) {
    const face = stripRules(project);
    const root = projectRoot(project);
    const walkCards = (dir) => {
      for (const entry of readdirSync(dir, { withFileTypes: true })) {
        const full = join(dir, entry.name);
        if (face.spares(entry, root)) {
          walkCards(full);
          continue;
        }
        if (face.within(entry.name)) {
          standing.push(relativeFrom(root, full));
          continue;
        }
        if (entry.isDirectory()) walkCards(full);
      }
    };
    walkCards(root);
  }
  if (standing.length > 0) {
    fail(
      `the trees still carry ${standing.join(', ')} after the strip, and each one is a rule the strip face names: a file ` +
        'the merge reads and the tree keeps is a second copy of that reading',
    );
  }
  const released = [];
  const kept = [];
  for (const project of projects) {
    if (strippedAbsence(project) === null) continue;
    const directory = strippedDirectoryPath(project);
    if (existsSync(directory)) {
      released.push(relativeFrom(projectRoot(project), directory));
    } else {
      kept.push(relativeFrom(projectRoot(project), directory));
    }
  }
  if (released.length === 0) {
    fail(
      `no taken tree carries a directory \`sync.strippedPackageDirectory\` names, so the shell face leaves no directory of ` +
        `its own and the deshell step would read the backend crate through a card it gave up: ${kept.join(', ')}`,
    );
  }
  if (removed.length === 0) {
    fail(
      `scripts/strip-manifest.json names no artifact this run found in ${projects.map((project) => project.id).join(', ')}, ` +
        'so nothing says the rule face and the trees describe the same thing',
    );
  }
  process.stdout.write(
    `[sync-upstream] strip: removed ${removed.length} artifact(s) from the taken trees, and ${released.join(', ')} left ` +
      'whole as the shell face it narrowed to\n',
  );
  return { strip: removed.length };
}

const SHELL_FACE = ['tauri.conf.json', 'capabilities', 'icons', 'gen'];

function deshellCrate(directory) {
  const dropped = [];
  for (const name of SHELL_FACE) {
    const path = join(directory, ...name.split('/'));
    if (!existsSync(path)) continue;
    rmSync(path, { recursive: true, force: true });
    if (existsSync(path)) fail(`${path} stands after the shell this step removes took it, so that layer is still in the tree`);
    dropped.push(name);
  }
  return dropped;
}

function deshellFace() {
  const face = JSON.parse(readFileSync(DESHELL_FACE, 'utf8'));
  for (const key of ['files', 'blocks']) {
    if (!Array.isArray(face[key]) || face[key].length === 0) {
      fail(`${DESHELL_FACE} carries no \`${key}\`, so the shell face this step removes stands written nowhere`);
    }
  }
  const blocks = face.blocks.map((block) => {
    for (const key of ['file', 'opens']) {
      if (typeof block[key] !== 'string' || block[key] === '') {
        fail(`${DESHELL_FACE}: a block face carries no \`${key}\`, so nothing says which block in which file is the shell's`);
      }
    }
    const retains = block.retains ?? [];
    if (!Array.isArray(retains) || retains.length === 0 || retains.some((line) => typeof line !== 'string' || line === '')) {
      fail(
        `${DESHELL_FACE}: the block in ${block.file} names no text it leaves standing, so a removed block and a spent ` +
          'anchor would read alike',
      );
    }
    return block;
  });
  for (const project of absorbedProjects()) {
    const absence = strippedAbsence(project);
    if (absence === null) continue;
    const layer = absence.directories[0];
    for (const block of blocks) {
      if (block.file === absence.removes[0]) {
        fail(
          `${DESHELL_FACE}: the block in ${block.file} stands in the card the strip takes off ${project.id}, and that file ` +
            'leaves with the entry it carries: no block survives in it',
        );
      }
      if (layer === '' || !block.file.startsWith(`${layer}/`)) {
        fail(
          `${DESHELL_FACE}: the block in ${block.file} stands outside ${layer}/ of ${project.id}, and the deshell step reads ` +
            'those blocks from that layer alone',
        );
      }
    }
  }
  return { files: face.files, blocks };
}

function bracesOpen(text) {
  return text.split('{').length - text.split('}').length;
}

function openedBrace(text, opens) {
  let depth = 0;
  for (let at = 0; at < text.length; at += 1) {
    const character = text[at];
    if (character !== '{' && character !== '}') continue;
    depth += character === '{' ? 1 : -1;
    if (depth === opens) return at;
  }
  return -1;
}

function dropBlock(path, relative, block) {
  if (!existsSync(path)) {
    fail(
      `${relative} stands nowhere, and both the deshell face and the take's build inputs name it: a block with no file to ` +
        'stand in is a face the tree never carried',
    );
  }
  const text = readFileSync(path, 'utf8');
  const at = text.indexOf(block.opens);
  if (at === -1) {
    fail(
      `${relative} carries no such block, and the deshell face names it there: either the upstream moved it or the face ` +
        'names a text this file does not hold, and neither is a block this step took',
    );
  }
  if (text.indexOf(block.opens, at + 1) !== -1) {
    fail(`${relative} carries the block this step removes more than once, and one anchor names one place`);
  }
  const opens = bracesOpen(text.slice(0, at)) + bracesOpen(block.opens);
  const from = text.lastIndexOf('{', at);
  const to = from === -1 ? -1 : openedBrace(text, opens);
  if (from === -1 || to === -1) {
    fail(`${relative}: the block the face names opens no function this step can close, so its end stands nowhere`);
  }
  const kept = text.slice(0, from + 1) + text.slice(to + 1);
  for (const line of block.retains) {
    if (!kept.includes(line)) {
      fail(
        `${relative}: taking the block out would also take ${line}, which stands outside the window this crate keeps as ` +
          'a member; the face and the tree describe different things',
      );
    }
  }
  writeFileSync(path, kept);
}

function deshell() {
  const face = deshellFace();
  if (face.files.join(',') !== SHELL_FACE.join(',')) {
    fail(
      `${DESHELL_FACE} lists ${face.files.join(', ')} as the shell face, and this step removes ${SHELL_FACE.join(', ')}: ` +
        'one face stands in one place',
    );
  }
  const carried = [];
  const spent = [];
  for (const project of absorbedProjects()) {
    const absence = strippedAbsence(project);
    if (absence === null) continue;
    const directory = strippedDirectoryPath(project);
    if (!existsSync(directory)) {
      for (const block of face.blocks) {
        dropBlock(join(projectRoot(project), ...block.file.split('/')), `${project.id}/${block.file}`, block);
      }
      carried.push(`${project.id}: ${face.blocks.map((block) => block.file).join(', ')}`);
      continue;
    }
    const dropped = deshellCrate(directory);
    if (dropped.length === 0) {
      spent.push(`${absence.segment('Cargo.toml')} stands inside the directory the strip released, and no shell face does`);
      continue;
    }
    carried.push(`${project.id}: ${dropped.join(', ')} from ${directory.slice(projectRoot(project).length + 1).split('\\').join('/')}`);
  }
  if (spent.length > 0) {
    fail(
      `deshell: ${spent.join('; ')}. The blocks are read from the card the backend crate stands in, and the shell face ` +
        'stands where that card left: the two are not one directory here',
    );
  }
  if (carried.length === 0) {
    fail(
      `deshell: no member directory carried a ${SHELL_FACE.join(', ')} face or a block ${DESHELL_FACE} names, so nothing ` +
        'says the shell this step removes was ever there',
    );
  }
  process.stdout.write(`[sync-upstream] deshell: ${carried.join('; ')}\n`);
}

function refuseProjectArtifacts() {
  refuseProjectArtifactsOf(absorbedProjects());
}

function patchStep() {
  const projects = absorbedProjects();
  const payload = payloadDeclaration();
  const declared = readPatchDeclarations();
  for (const { path, patch, landing: head } of declared) {
    const project = projects.find((candidate) => patch.file.startsWith(`${candidate.root}/`));
    if (project === undefined) {
      fail(`${path} names ${patch.file}, which stands in no declared absorbed tree`);
    }
    const inside = patch.file.slice(project.root.length + 1);
    if (inside === '') {
      fail(`${path} names ${patch.file}, which is the tree itself and not a file inside it`);
    }
    const landing = landingCopies(head, path).find((candidate) => candidate.id === project.id);
    if (landing === undefined) {
      fail(`${path} says ${patch.landed} applies it, and ${patch.landed} lands no copy of ${project.id}`);
    }
    const carried = patch.creates;
    if (carried !== undefined) {
      const written = carried?.path;
      if (typeof written !== 'string' || written === '') {
        fail(`${path} carries a file and names no path for it, so nothing says where it is written`);
      }
      continue;
    }
    const released = strippedRootOf(project, inside);
    const face = strippedAbsence(project) === null ? null : strippedRootOf(project, '');
    if (released !== null && released !== face && released !== '..') {
      fail(
        `${path} is anchored in ${inside}, which stands outside the directory the strip released for ${project.id}: ` +
          'a path edge there points at a crate the mirror does not carry',
      );
    }
    const anchored = join(payloadRootPath(landing, payload), ...inside.split('/'));
    const read = !existsSync(anchored) && face !== null && !existsSync(strippedDirectoryPath(project))
      ? join(strippedDirectoryPath(project), ...inside.split('/'))
      : anchored;
    if (!existsSync(read)) {
      fail(
        `${path} is anchored in ${inside}, which the ${head} copy of ${project.id} does not carry, so its find text ` +
          'names nothing in the copy it is replayed over',
      );
    }
  }
  process.stdout.write(
    `[sync-upstream] patches: ${declared.length} declaration(s), each landing on a copy this run lands (${LANDINGS.join(', ')})\n`,
  );
}

function install(shared) {
  for (const { project, landing } of shared.taken) writeTree(project, landing);
  retireTakeRoots(shared.taken);
  shared.taken = [];
}

function main(shared) {
  const projects = absorbedProjects();
  compareWithNotice(projects);
  projects.forEach(probe);
  refuseProjectArtifacts();
  if (process.env.LVPA_TRACE_SNAPSHOT) process.stderr.write(`[trace-main] chain=${CHAIN.join(',')}\n`);
  for (const name of CHAIN) {
    const step = STEP_NAMES.get(name);
    if (step !== undefined) step(projects, shared);
  }
  for (const line of syncOpenAlgoFrontend()) {
    process.stdout.write(
      `[sync-upstream] landed ${line.id} at ${line.target}: ${line.projected} projected file(s), ` +
        `${line.patched.length} patched file(s)\n`,
    );
  }
  process.stdout.write(`[sync-upstream] took ${shared.projects.map((project) => project.id).join(', ')}\n`);
}

const CHAIN = chainOrder('dev');

const RANK = new Map(CHAIN.map((name, at) => [name, at]));

const SHAPE_PRECEDES = new Map([
  ['merge', 'take'],
  ['install', 'merge'],
  ['patches', 'install'],
]);

const STEP_NAMES = new Map([
  ['take', (projects, shared) => Object.assign(shared, take(projects))],
  ['merge', (projects, shared) => merge(projects)],
  ['install', (projects, shared) => install(shared)],
  ['strip', (projects, shared) => Object.assign(shared, strip(projects, shared))],
  ['deshell', deshell],
  ['patches', patchStep],
]);

function stepReads(name) {
  const required = SHAPE_PRECEDES.get(name);
  const at = RANK.get(name) ?? -1;
  const prior = required === undefined ? undefined : RANK.get(required) ?? -1;
  return { at, required, prior };
}

function runSteps(names) {
  const requested = [...new Set(names)].sort((left, right) => RANK.get(left) - RANK.get(right));
  for (const name of requested) {
    const { at, required, prior } = stepReads(name);
    if (required === undefined) continue;
    if (at === -1 || prior === -1) fail(`${name} or ${required} names a step crates/wiring/shape does not carry`);
    if (!requested.slice(0, requested.indexOf(name)).includes(required)) {
      fail(`${name} reads what ${required} leaves on disk, and no ${required} stands before it in this run`);
    }
    if (prior > at) fail(`the chain puts ${required} after ${name}, so ${name} cannot read what it leaves`);
  }
  const projects = absorbedProjects();
  compareWithNotice(projects);
  projects.forEach(probe);
  const shared = {};
  if (names.includes('strip')) refuseProjectArtifacts();
  for (const name of requested) STEP_NAMES.get(name)(projects, shared);
  return shared;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const requested = process.argv.slice(2);
  if (requested.length === 0) {
    main({});
  } else {
    const unknown = requested.filter((name) => !STEP_NAMES.has(name));
    if (unknown.length > 0) {
      fail(`${unknown.join(', ')} is not a step this file carries: ${[...STEP_NAMES.keys()].join(', ')}`);
    }
    runSteps(requested);
  }
}
