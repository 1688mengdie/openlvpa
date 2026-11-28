#!/usr/bin/env node

import { spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { dirname, isAbsolute, join, relative, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  DECLARATION_PATH,
  REPO_ROOT,
  absorbedProjects,
  buildConfigFiles,
  deshellBlockFiles,
  lockFiles,
  mechanism,
  packageDirectoryPath,
  projectRoot,
  snapshotCardOf,
  snapshotMemberOf,
  snapshotPath,
  snapshotSections,
  snapshotSkip,
  snapshotStrippedAbsence,
  stripManifestPath,
  stripRules,
} from './absorbed-sources.mjs';
import { convergenceDeclaration, parseToml, syncAbsorbedMembers } from './sync-absorbed-members.mjs';
import { syncAbsorbedFrontend } from './sync-absorbed-frontend.mjs';
import { targetDir } from './target-dir.mjs';

const NOTICE = join(REPO_ROOT, 'NOTICE');

const TREE_MANIFEST = 'Cargo.toml';

const TREE_PACKAGE_MANIFEST = 'package.json';

const TREE_MANIFESTS = new Set([TREE_MANIFEST, TREE_PACKAGE_MANIFEST]);

const SNAPSHOT_SKIP = new Set(['node_modules', 'target', '.git']);

const MISSED_CARD_TAG = '[sync-upstream] missed-cards:';

/// The snapshot is the tree's own words kept in one file, and every `where` in it is the
/// tree's own path: the take writes the tree whole, so the snapshot names what stands in
/// it, never what stands in the mirror. A card below the tree's root that the mirror
/// carries as a member reaches the mirror at that member's path, which is the one reading
/// `absorbed-sources.mjs` applies to the same line.
function treeWhere(project, where) {
  const prefix = `${project.root}/`;
  return where.startsWith(prefix) ? where.slice(prefix.length) : where;
}

function projectOf(id) {
  const project = absorbedProjects().find((entry) => entry.id === id);
  if (project === undefined) fail(`${id} names no absorbed tree this repository takes`);
  return project;
}

function fail(message) {
  process.stderr.write(`[sync-upstream] FATAL: ${message}\n`);
  process.exit(1);
}

function run(command, args, cwd) {
  const result = spawnSync(command, args, { cwd, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  if (result.error) fail(`${command} could not start: ${result.error.message}`);
  return result;
}

function copyTree(from, to) {
  cpSync(from, to, { recursive: true, verbatimSymlinks: true });
}

function takenRoot(project) {
  const at = join(targetDir(), 'take', project.id);
  return at;
}

/// A tree is taken whole, so a copy that stands is set aside and taken again: a tree is
/// taken and never written into, and what stands there is the copy before this run.
function landTree(project) {
  const at = takenRoot(project);
  rmSync(at, { recursive: true, force: true });
  mkdirSync(dirname(at), { recursive: true });
  const cloned = run('git', ['clone', '--depth', '1', '--single-branch', '--branch', project.branch, project.url, at], REPO_ROOT);
  if (cloned.status !== 0) {
    fail(`${project.id}: ${project.url} on ${project.branch} did not clone: ${(cloned.stderr ?? '').trim().split('\n').slice(-3).join(' ')}`);
  }
  return at;
}

/// The cards the mirror will not read are the ones the strip takes out, and the snapshot
/// keeps every one of them: a card the strip removes and the snapshot omits reaches the
/// merge as a file that is not there.
function assertEveryCardIsKept(project, landing, documents) {
  const capture = buildConfigFiles(project).filter((name) => name.split('/').pop() === TREE_MANIFEST);
  const kept = new Set(documents.map((document) => document.where));
  const missed = capture.filter((name) => !kept.has(name));
  if (missed.length === 0) return;
  throw new Error(`${MISSED_CARD_TAG} ${missed.join(', ')}`);
}

/// The trees are carried into the repository as the take landed them, and a card whose
/// content this repository rewrites does not leave them: the merge reads the card the
/// snapshot keeps, so the tree and the mirror read one copy of every word in it.
function refuseAnEditedTree(project, landing) {
  const face = stripRules(project);
  const treeRoot = projectRoot(project);
  if (!existsSync(treeRoot)) return;
  const out = [];
  const walk = (dir, where) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const full = join(dir, entry.name);
      const named = `${where}${entry.name}`;
      const edited = face.within(entry.name) && !face.spares(entry, treeRoot);
      if (edited) out.push(named);
      if (entry.isDirectory() && !edited) walk(full, `${named}/`);
    }
  };
  walk(treeRoot, '');
  if (out.length > 0) {
    console.log(`[sync-upstream] ${project.id}: setting aside a copy this run replaces, and it carries ${out.length} entry(ies) the strip removes`);
  }
}

function relativeFrom(root, target) {
  return relative(root, target).split(sep).join('/');
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
  const absence = snapshotStrippedAbsence(project);
  const buildInputs = new Set(
    absence === null
      ? []
      : buildConfigFiles(project).filter((name) => !TREE_MANIFESTS.has(name.split('/').pop()) && deshellBlockFiles().includes(name)),
  );
  const buildDeclared = new Set(buildConfigFiles(project));
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      if (entry.isDirectory()) {
        if (!SNAPSHOT_SKIP.has(entry.name) && !snapshotSkip(project, entry.name)) walk(join(dir, entry.name));
        continue;
      }
      const path = join(dir, entry.name);
      const where = relativeFrom(landing, path);
      const buildInput = buildDeclared.has(where);
      const strippedCard = absence !== null && absence.removes.includes(where);
      if (TREE_MANIFESTS.has(entry.name) || buildInput || strippedCard) {
        documents.push(entry.name === TREE_MANIFEST || strippedCard || buildInput
          ? { where, kind: 'toml', text: readToml(path, project, landing) }
          : { where, kind: 'json', text: readJson(path, project, landing) });
        continue;
      }
      if (!buildInputs.has(where)) continue;
      documents.push({ where, kind: 'toml', text: readToml(path, project, landing) });
    }
  };
  walk(landing);
  try {
    assertEveryCardIsKept(project, landing, documents);
  } catch (error) {
    if (!String(error.message).startsWith(MISSED_CARD_TAG)) throw error;
    fail(
      `${project.id}: ${String(error.message).slice(MISSED_CARD_TAG.length).trim()} is a card the strip removes and the ` +
        'snapshot does not keep, so the merge would read a file that stands nowhere after the strip',
    );
  }
  const body = snapshotDocument(project, documents, absence, landing);
  writeFileSync(snapshotPath(project), body, 'utf8');
  const card = snapshotCardOf(project);
  if (snapshotSections(project).get(card.split('/').pop())?.text === undefined) {
    fail(`the take wrote ${snapshotPath(project)} and it carries no ${card}, so the card the mirror reads stands nowhere`);
  }
  return documents;
}

/// The snapshot is the tree's own words kept in one file: a declaration segment stating
/// the remote, branch and root the entry was read from, then one segment per manifest the
/// strip removes, each in where order. It carries no count of anything the tree states,
/// because a count here would be a second answer to what the manifests already say.
function snapshotDocument(project, documents, absence, landing) {
  const rows = [
    '# >>> lvpa-snapshot declaration snapshot',
    `remote ${project.url}`,
    `branch ${project.branch}`,
    `root ${project.root}`,
    '# <<< lvpa-snapshot snapshot',
  ];
  for (const document of [...documents].sort((left, right) => (left.where < right.where ? -1 : left.where > right.where ? 1 : 0))) {
    const where = treeWhere(project, document.where);
    rows.push(`# >>> lvpa-snapshot ${document.kind} ${where}`);
    rows.push(...document.text.replace(/\r\n/g, '\n').replace(/\n$/, '').split('\n'));
    rows.push(`# <<< lvpa-snapshot ${where}`);
  }
  return `${rows.join('\n')}\n`;
}

function strip(projects) {
  for (const project of projects) {
    const root = projectRoot(project);
    if (!existsSync(root)) continue;
    const face = stripRules(project);
    const removed = [];
    const walk = (dir, where) => {
      for (const entry of readdirSync(dir, { withFileTypes: true })) {
        const full = join(dir, entry.name);
        const named = `${where}${entry.name}`;
        if (face.spares(entry, root)) {
          walk(full, `${named}/`);
          continue;
        }
        if (face.within(entry.name)) {
          rmSync(full, { recursive: true, force: true });
          removed.push(named);
          continue;
        }
        if (entry.isDirectory()) walk(full, `${named}/`);
      }
    };
    walk(root, '');
    if (removed.length > 0) {
      console.log(`[sync-upstream] ${project.id}: removed ${removed.length} entr(ies) that speak on their own`);
    }
    if (!existsSync(root)) fail(`${project.id}: the strip left no directory at ${root}, and a tree is stripped rather than deleted`);
  }
  const declaration = JSON.parse(readFileSync(DECLARATION_PATH, 'utf8'));
  if (!Array.isArray(declaration?.projects) || declaration.projects.length === 0) {
    fail(`${DECLARATION_PATH} declares no projects`);
  }
}

function deshell(projects) {
  for (const project of projects) {
    const absence = snapshotStrippedAbsence(project);
    if (absence === null) continue;
    const root = projectRoot(project);
    const within = join(root, ...absence.directories[0].split('/').filter((part) => part !== ''));
    if (existsSync(within)) rmSync(within, { recursive: true, force: true });
  }
}

function patches(projects) {
  const files = readdirSync(join(REPO_ROOT, 'patches')).filter((name) => name.endsWith('.patch.json'));
  const claimed = new Set();
  for (const name of files) {
    const face = JSON.parse(readFileSync(join(REPO_ROOT, 'patches', name), 'utf8'));
    if (typeof face?.landed !== 'string' || !face.landed.includes(':')) {
      fail(`patches/${name} carries no \`landed\` head, so nothing says which tree it lands in`);
    }
    const tree = face.landed.slice(0, face.landed.indexOf(':'));
    if (!projects.some((project) => project.id === tree)) {
      fail(`patches/${name} lands in ${tree}, and that is no absorbed tree this repository takes`);
    }
    claimed.add(tree);
  }
  for (const project of projects) {
    if (claimed.has(project.id)) continue;
    fail(`no declaration under patches/ lands in ${project.id}, and a tree the take strips reaches the mirror through its patches`);
  }
  console.log(`[sync-upstream] patches: ${files.length} declaration(s) stand under patches/`);
}

function landCards(project, landing) {
  return landing;
}

/// The one lock a tree carries is taken down where the take meets it: a tree is a heap of static
/// source, and a lock resolved by that tree answers which versions that tree read rather than
/// which versions the merged graph resolves. The files are named by the strip face rather than
/// by one path, so a lock at any depth of any declared tree leaves here and nowhere else.
function dropLocks(project, root) {
  const dropped = lockFiles(project, root);
  for (const where of dropped) {
    rmSync(join(root, ...where.split('/')), { force: true });
  }
  return dropped;
}

/// The tree the take writes is the stripped tree: what the strip, the deshell face and the one
/// lock rule remove leaves before the copy reaches the mirror, and the copy is made from the tree
/// as it stands after them. A tree that lost its whole root to those rules is a tree the mirror
/// reads a member out of nowhere, so the take stops here.
function writeTree(project, landing) {
  const destination = projectRoot(project);
  rmSync(destination, { recursive: true, force: true });
  mkdirSync(dirname(destination), { recursive: true });
  copyTree(landing, destination);
  strip([project]);
  deshell([project]);
  const locks = dropLocks(project, destination);
  if (!existsSync(destination)) {
    fail(`${project.id}: the strip left no directory at ${destination}, and a tree is stripped rather than deleted`);
  }
  if (locks.length > 0) {
    console.log(`[sync-upstream] ${project.id}: ${locks.length} lock(s) taken down with the strip`);
  }
}

function take(projects) {
  const landed = projects.map((project) => ({ project, landing: landTree(project) }));
  for (const { project, landing } of landed) refuseAnEditedTree(project, landing);
  for (const { project, landing } of landed) dropLocks(project, landing);
  for (const { project, landing } of landed) snapshotManifest(project, landing);
  for (const { project, landing } of landed) writeTree(project, landCards(project, landing));
  for (const { landing } of landed) rmSync(landing, { recursive: true, force: true });
  return { projects };
}

function merge(projects) {
  const members = syncAbsorbedMembers({ check: false });
  syncAbsorbedFrontend({ check: false });
  process.stdout.write(
    `[sync-upstream] merge: ${projects.map((project) => project.id).join(', ')} speak from the repository root alone\n`,
  );
  if (members.pending !== undefined) return { pending: members.pending };
  return { merged: true };
}

/// NOTICE names each tree's remote and branch in one section, and the declaration names
/// the same two. A tree whose two readings part is a tree taken from somewhere other than
/// what is claimed for it, so this step compares the two and stops on a difference.
function compareWithNotice(projects) {
  const notice = readFileSync(NOTICE, 'utf8');
  for (const project of projects) {
    const section = notice.split(/\n(?=\d+\.\s)/u).find((part) => new RegExp(`^\\s*Remote:\\s+${project.url.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}\\s*$`, 'mu').test(part));
    if (section === undefined) {
      fail(`${project.id}: NOTICE names no section for ${project.url}, and a tree is taken from what NOTICE names`);
    }
    const branch = section.match(/^\s*Branch:\s+(\S+)\s*$/mu)?.[1];
    if (branch !== project.branch) {
      fail(`${project.id}: NOTICE names ${branch ?? 'no branch'} for ${project.url} and ${DECLARATION_PATH} names ${project.branch}`);
    }
  }
}

const STEPS = new Map([
  ['take', (projects) => take(projects)],
  ['absorb', (projects) => { compareWithNotice(projects); }],
  ['merge', (projects) => merge(projects)],
  ['strip', (projects) => strip(projects)],
  ['deshell', (projects) => deshell(projects)],
  ['patches', (projects) => patches(projects)],
]);

const ORDER = ['take', 'absorb', 'merge', 'strip', 'deshell', 'patches'];

function runSteps(names) {
  const projects = absorbedProjects();
  for (const name of names) {
    const step = STEPS.get(name);
    if (step === undefined) fail(`${name} is not a step this file carries: ${ORDER.join(', ')}`);
    const settled = step(projects);
    if (settled === undefined || settled.pending === undefined) {
      process.stdout.write(`[sync-upstream] ${name}: done\n`);
      continue;
    }
    process.stdout.write(
      `[sync-upstream] ${name}: the generated tables stand as they were written, and the graph read after them resolves no ` +
        `instance of one crate:\n${settled.pending}\n`,
    );
    return;
  }
}

const requested = process.argv.slice(2);
if (fileURLToPath(import.meta.url) === process.argv[1]) {
  if (requested.length === 0) runSteps(ORDER);
  else runSteps(requested);
}
