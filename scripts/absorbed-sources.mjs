#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));

export const REPO_ROOT = process.env.OPENLVPA_REPO_ROOT
  ? resolve(process.env.OPENLVPA_REPO_ROOT)
  : resolve(SCRIPT_DIR, '..');

export const DECLARATION_PATH = join(SCRIPT_DIR, 'absorbed-sources.json');

export function absorbedProjects() {
  if (!existsSync(DECLARATION_PATH)) {
    throw new Error(`[absorbed-sources] the declaration is missing: ${DECLARATION_PATH}`);
  }
  const declaration = JSON.parse(readFileSync(DECLARATION_PATH, 'utf8'));
  const projects = declaration?.projects;
  if (!Array.isArray(projects) || projects.length === 0) {
    throw new Error(`[absorbed-sources] ${DECLARATION_PATH} declares no projects, so there is no tree to take`);
  }
  const roots = new Set();
  const ids = new Set();
  for (const project of projects) {
    for (const key of ['id', 'root', 'url', 'branch', 'snapshotFile', 'stripManifest']) {
      if (typeof project?.[key] !== 'string' || project[key].length === 0) {
        throw new Error(`[absorbed-sources] a project entry is missing \`${key}\`: ${JSON.stringify(project)}`);
      }
    }
    if (roots.has(project.root)) {
      throw new Error(`[absorbed-sources] two projects declare the root \`${project.root}\`, so a lookup by root cannot choose`);
    }
    roots.add(project.root);
    if (ids.has(project.id)) {
      throw new Error(`[absorbed-sources] two projects declare the id \`${project.id}\`, so a take cannot be addressed to one of them`);
    }
    ids.add(project.id);
    mechanism(project);
    assertMechanismPairing(project);
    frontend(project);
  }
  return projects;
}

export function projectRoot(project) {
  return join(REPO_ROOT, ...project.root.split('/'));
}

export function snapshotPath(project) {
  return join(REPO_ROOT, ...project.snapshotFile.split('/'));
}

export function stripManifestPath(project) {
  return join(REPO_ROOT, ...project.stripManifest.split('/'));
}

const SNAPSHOT_OPENS = '# >>> lvpa-snapshot ';
const SNAPSHOT_CLOSES = '# <<< lvpa-snapshot ';
const SNAPSHOT_HEADER_KIND = 'declaration';
const SNAPSHOT_HEADER_WHERE = 'snapshot';
const SNAPSHOT_HEADER_FIELDS = ['remote', 'branch', 'root'];
const SNAPSHOT_KINDS = ['toml', 'json'];

function snapshotLines(text) {
  if (text.includes('\r')) {
    throw new Error('the snapshot states its lines with a CR in them, and an original carried through it would lose that');
  }
  const lines = text.split('\n');
  if (lines[lines.length - 1] === '') lines.pop();
  return lines;
}

function snapshotBody(text) {
  return text.endsWith('\n') ? text.slice(0, -1) : text;
}

function snapshotLayout(lines, path) {
  const sections = [];
  let open = null;
  for (let at = 0; at < lines.length; at += 1) {
    const line = lines[at];
    if (line.startsWith(SNAPSHOT_OPENS)) {
      if (open !== null) {
        throw new Error(`${path}:${at + 1}: a segment opens at # >>> lvpa-snapshot ${line.slice(SNAPSHOT_OPENS.length)} while the segment for ${open.where} is still open`);
      }
      const [kind, where, ...rest] = line.slice(SNAPSHOT_OPENS.length).split(' ');
      if (rest.length !== 0) {
        throw new Error(`${path}:${at + 1}: the header ${JSON.stringify(line)} carries more than a kind and a where`);
      }
      if (where === undefined) {
        throw new Error(`${path}:${at + 1}: the header ${JSON.stringify(line)} names no where`);
      }
      const header = kind === SNAPSHOT_HEADER_KIND && where === SNAPSHOT_HEADER_WHERE;
      if (!header && !SNAPSHOT_KINDS.includes(kind)) {
        throw new Error(`${path}:${at + 1}: the header ${JSON.stringify(line)} names the kind ${JSON.stringify(kind)}, which is not one of ${SNAPSHOT_KINDS.join(', ')}`);
      }
      open = { kind, where, header, at, body: [], tail: false };
      continue;
    }
    const closes = line.startsWith(SNAPSHOT_CLOSES) ? line.slice(SNAPSHOT_CLOSES.length) : null;
    if (closes !== null) {
      if (open === null) {
        throw new Error(`${path}:${at + 1}: ${JSON.stringify(line)} closes a segment that no header opened`);
      }
      if (closes !== open.where) {
        throw new Error(`${path}:${at + 1}: ${JSON.stringify(line)} closes a segment whose header opened ${open.where}`);
      }
      if (open.body.length === 0 && !open.tail) {
        throw new Error(`${path}: no original stands between the header for ${open.where} and its close, so that segment carries nothing`);
      }
      const text = open.body.join('\n') + (open.tail ? '\n' : '');
      sections.push({ kind: open.kind, where: open.where, header: open.header, text });
      open = null;
      continue;
    }
    if (open === null) {
      throw new Error(`${path}:${at + 1}: this line stands outside every segment, and a snapshot is segments alone: ${JSON.stringify(line)}`);
    }
    if (line === '' && at === lines.length - 1) {
      open.tail = true;
      continue;
    }
    open.body.push(line);
  }
  if (open !== null) {
    throw new Error(`${path}: the segment for ${open.where} opens at # >>> lvpa-snapshot ${open.kind} ${open.where} and no # <<< lvpa-snapshot ${open.where} closes it`);
  }
  return sections;
}

function declarationOf(project) {
  const registered = absorbedProjects().find((entry) => entry.id === project?.id);
  if (registered === undefined) {
    throw new Error(`[absorbed-sources] no project \`${project?.id}\` is declared, so no snapshot answers to it`);
  }
  return registered;
}

function declarationBody(project) {
  const fields = { remote: project.url, branch: project.branch, root: project.root };
  return SNAPSHOT_HEADER_FIELDS.map((field) => `${field} ${fields[field]}`).join('\n');
}

function assertSections(sections, project, path) {
  if (sections.length === 0 || !sections[0].header) {
    throw new Error(`${path}: this snapshot is the pre-merge single-manifest form; the take rewrites it`);
  }
  for (const section of sections.slice(1)) {
    if (section.header) {
      throw new Error(`${path}: the declaration segment stands at ${section.where} and again inside the snapshot, and one snapshot is one tree`);
    }
  }
  const stated = declarationBody(project);
  if (sections[0].text !== stated) {
    throw new Error(`${path}: the declaration segment reads ${JSON.stringify(sections[0].text)} and ${project.id} declares ${JSON.stringify(stated)}, so the snapshot speaks for a tree the take was not told of`);
  }
  for (const section of sections) {
    if (section.where.includes(' ')) {
      throw new Error(`${path}: the segment for ${JSON.stringify(section.where)} names a where with a space in it, and a where is written on an unbroken header`);
    }
  }
  for (let at = 2; at < sections.length; at += 1) {
    if (!(sections[at - 1].where < sections[at].where)) {
      throw new Error(`${path}: the segment for ${sections[at].where} stands after ${sections[at - 1].where}, and a snapshot carries each where once in where order`);
    }
  }
  return new Map(sections.slice(1).map((section) => [section.where, { kind: section.kind, text: section.text }]));
}

export function snapshotSections(project) {
  const declared = declarationOf(project);
  const path = snapshotPath(declared);
  if (process.env.LVPA_TRACE_SNAPSHOT) {
    process.stderr.write(`[trace-path] ${path} cwd=${process.cwd()}\n`);
  }
  if (process.env.LVPA_TRACE_SNAPSHOT) {
    const raw = readFileSync(path, 'utf8');
    const all = snapshotLayout(snapshotLines(raw), path);
    const want = snapshotMemberOf(declared);
    const held = all.find((section) => section.where === want);
    const line = raw.split('\n').find((text) => text.startsWith('argon2'));
    process.stderr.write(
      `[trace-sections] ${path} arcs=${(raw.match(/argon2/g) ?? []).length} ` +
        `body=${held === undefined ? 'absent' : held.text.split('\n').length} ` +
        `body-argon2=${held !== undefined && held.text.includes('argon2')} ` +
        `raw-line=${JSON.stringify(line)}\n`,
    );
  }
  if (!existsSync(path)) {
    throw new Error(`[absorbed-sources] ${path} stands nowhere, and it is the snapshot of ${declared.id} the take writes`);
  }
  return assertSections(snapshotLayout(snapshotLines(readFileSync(path, 'utf8')), path), declared, path);
}

export function snapshotMemberOf(project) {
  const declared = declarationOf(project);
  const directory = declared.sync?.packageDirectory ?? '';
  return directory === '' ? 'Cargo.toml' : `${directory}/Cargo.toml`;
}

export function snapshotMemberText(project) {
  const declared = declarationOf(project);
  const where = snapshotMemberOf(declared);
  const held = snapshotSections(declared).get(where);
  if (process.env.LVPA_TRACE_SNAPSHOT) {
    process.stderr.write(`[trace-member] ${declared.id} where=${where} lines=${held === undefined ? 'absent' : held.text.split('\n').length} argon2=${held !== undefined && held.text.includes('argon2')}\n`);
  }
  if (held === undefined) {
    throw new Error(
      `[absorbed-sources] ${snapshotPath(declared)} carries no segment for ${where}, so the member manifest the mirror ` +
        `derives from ${declared.id} stands written nowhere`,
    );
  }
  return held.text;
}

export function snapshotDocument(project, documents, absence = null) {
  const declared = declarationOf(project);
  const dropped = new Set(absence === null ? [] : absence.removes);
  const ordered = [...documents]
    .filter((document) => !dropped.has(document.where))
    .concat(absence === null ? [] : absence.adds)
    .sort((left, right) => (left.where < right.where ? -1 : left.where > right.where ? 1 : 0));
  const seen = new Set();
  const rows = [`${SNAPSHOT_OPENS}${SNAPSHOT_HEADER_KIND} ${SNAPSHOT_HEADER_WHERE}`, ...declarationBody(declared).split('\n'), `${SNAPSHOT_CLOSES}${SNAPSHOT_HEADER_WHERE}`];
  for (const { kind, where, text } of ordered) {
    if (seen.has(where)) {
      throw new Error(`[absorbed-sources] ${declared.id} carries ${where} twice, and a snapshot carries each where once`);
    }
    seen.add(where);
    if (where.includes(' ')) {
      throw new Error(`[absorbed-sources] ${declared.id} carries the file ${JSON.stringify(where)}, and a where with a space in it has no header to state it`);
    }
    if (!SNAPSHOT_KINDS.includes(kind)) {
      throw new Error(`[absorbed-sources] ${declared.id} carries ${where} as a ${JSON.stringify(kind)}, which is not one of ${SNAPSHOT_KINDS.join(', ')}`);
    }
    for (const line of text.split('\n')) {
      if (line.startsWith(SNAPSHOT_OPENS) || line.startsWith(SNAPSHOT_CLOSES)) {
        throw new Error(`[absorbed-sources] ${where} carries the line ${JSON.stringify(line)}, which reads as a snapshot header or its close`);
      }
    }
    rows.push(`${SNAPSHOT_OPENS}${kind} ${where}`, ...snapshotBody(text).split('\n'), `${SNAPSHOT_CLOSES}${where}`);
  }
  const body = rows.join('\n');
  const path = snapshotPath(declared);
  const readBack = snapshotLayout(snapshotLines(body), path);
  const restored = assertSections(readBack, declared, path);
  const orderedBack = readBack.slice(1);
  if (orderedBack.length !== ordered.length) {
    throw new Error(
      `[absorbed-sources] ${declared.id} carries ${ordered.length} manifest(s) and the snapshot reads back as ${orderedBack.length} ` +
        `segment(s), so at least one manifest's own header line and close line became segment boundaries and the take is writing a ` +
        `snapshot that is not those manifests' copy: ${path}`,
    );
  }
  ordered.forEach(({ where }, at) => {
    if (orderedBack[at].where !== where) {
      throw new Error(
        `[absorbed-sources] the manifest at ${where} reads back at ${orderedBack[at].where} in ${path}, so its own header line or ` +
          "close line became a segment boundary and the snapshot is not that file's copy",
      );
    }
  });
  for (const { where, text } of ordered) {
    if (restored.get(where)?.text !== snapshotBody(text)) {
      throw new Error(`[absorbed-sources] ${where} does not read back as the text the take wrote, so the snapshot would not be ${path}'s copy of that file`);
    }
  }
  const carried = new Set(ordered.map(({ where }) => where));
  const before = snapshotSections(declared);
  for (const where of dropped) {
    if (!before.has(where)) {
      throw new Error(
        `[absorbed-sources] ${where} is the card the take drops from ${path}, and the taken tree carries no such file to drop: ` +
          'the absence would stand for a card nothing took',
      );
    }
    if (carried.has(where)) {
      throw new Error(`[absorbed-sources] ${where} is a card this take drops from ${path}, and the snapshot still carries it`);
    }
  }
  return body;
}

const STRIP_RULE_KEYS = [
  'excludedFileNames',
  'excludedDirNames',
  'excludedFilePrefixes',
  'excludedFileSuffixes',
  'identityCardDirs',
  'readBySourceNames',
];

export const TREE_MANIFEST = 'Cargo.toml';

export const TREE_PACKAGE_MANIFEST = 'package.json';

export function identityCardNames(project) {
  const face = stripRules(project);
  return identityCardFileNames(face);
}

function identityCardFileNames(face) {
  return [TREE_MANIFEST, TREE_PACKAGE_MANIFEST].filter((name) => face.within(name));
}

function readBySource(root, name) {
  const walked = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      if (entry.isDirectory()) {
        if (!new Set(['node_modules', 'target', '.git']).has(entry.name)) walk(join(dir, entry.name));
        continue;
      }
      if (/\.(rs|mjs|js)$/.test(entry.name)) walked.push(join(dir, entry.name));
    }
  };
  walk(root);
  return walked.some((path) => readFileSync(path, 'utf8').includes(`"${name}/`));
}

export function stripManifest(project) {
  return JSON.parse(readFileSync(stripManifestPath(project), 'utf8'));
}

export function stripRules(project) {
  const face = stripManifest(project);
  for (const key of STRIP_RULE_KEYS) {
    if (!Array.isArray(face[key])) {
      throw new Error(
        `[absorbed-sources] ${project.stripManifest} carries no \`${key}\`, so the strip would act on a rule set that is not there`,
      );
    }
  }
  const files = new Set(face.excludedFileNames);
  const dirs = new Set(face.excludedDirNames);
  const cardDirs = new Set(face.identityCardDirs);
  const readNames = new Set(face.readBySourceNames);
  return {
    within: (entry) => withinFace(face, dirs, files, entry),
    spares: (entry, root) => {
      const result = sparesOnFace(face, cardDirs, readNames, entry, root);
      if (process.env.LVPA_TRACE_SNAPSHOT && entry.name === 'dist') process.stderr.write(`[trace-rules-spares] ${root} result=${result} isDir=${entry.isDirectory()}\n`);
      return result;
    },
  };
}

function withinFace(face, dirs, files, entry) {
  const name = typeof entry === 'string' ? entry : entry.name;
  return (
    dirs.has(name)
    || files.has(name)
    || face.excludedFilePrefixes.some((prefix) => name.startsWith(prefix))
    || face.excludedFileSuffixes.some((suffix) => name.endsWith(suffix))
  );
}

function sparesOnFace(face, cardDirs, readNames, entry, root) {
  return (
    entry.isDirectory()
    && (cardDirs.has(entry.name) || (readNames.has(entry.name) && readBySource(root, entry.name)))
  );
}

export function manifestCapture(project) {
  const face = stripRules(project);
  const names = identityCardFileNames(face);
  if (names.length === 0) {
    throw new Error(
      `[absorbed-sources] ${project.stripManifest} strips none of ${TREE_MANIFEST} or ${TREE_PACKAGE_MANIFEST}, so the ` +
        `take of ${project.id} would leave an identity card standing in the tree it lands`,
    );
  }
  const root = projectRoot(project);
  if (!existsSync(root)) {
    throw new Error(`[absorbed-sources] ${root} stands nowhere, so no identity card of ${project.id} is there to read`);
  }
  const captured = [];
  const walk = (dir, where) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const nested = `${where}${entry.name}`;
      const full = join(dir, entry.name);
      if (names.includes(entry.name)) {
        if (face.within(entry.name)) captured.push(nested);
        continue;
      }
      if (!entry.isDirectory()) continue;
      if (face.spares(entry, root)) {
        walk(full, `${nested}/`);
        continue;
      }
      if (face.within(entry.name)) continue;
      walk(full, `${nested}/`);
    }
  };
  walk(root, '');
  return captured;
}

export function lockPath(project) {
  return join(REPO_ROOT, 'Cargo.lock');
}

export function packageManifestPath(project) {
  const declared = project?.sync?.packageManifest;
  if (typeof declared !== 'string' || declared === '') {
    throw new Error(`[absorbed-sources] the project \`${project?.id}\` declares no \`sync.packageManifest\``);
  }
  const root = project?.root;
  if (typeof root !== 'string' || root === '') {
    throw new Error(`[absorbed-sources] the project \`${project?.id}\` declares no \`root\``);
  }
  const rootSegments = root.replace(/\/+$/, '').split('/');
  const segments = declared.replace(/\/+$/, '').split('/');
  const underRoot = segments.length > rootSegments.length && rootSegments.every((segment, index) => segments[index] === segment);
  if (!underRoot) {
    throw new Error(
      `[absorbed-sources] the project \`${project?.id}\` names \`sync.packageManifest\` as ${JSON.stringify(declared)}, ` +
        `which does not stand under its declared root \`${root}\``,
    );
  }
  return segments.slice(rootSegments.length).join('/');
}

export function packageDirectory(project) {
  const declared = project?.sync?.packageDirectory;
  if (declared === undefined) return '';
  if (typeof declared !== 'string' || declared === '' || declared.startsWith('/') || declared.includes('..')) {
    throw new Error(
      `[absorbed-sources] the project \`${project?.id}\` names \`sync.packageDirectory\` as ${JSON.stringify(declared)}, ` +
        'which is not a directory inside its declared root. A tree whose root is the crate leaves the key out.',
    );
  }
  return declared.replace(/\/+$/, '');
}

export function packageDirectoryPath(project) {
  return join(projectRoot(project), ...packageDirectory(project).split('/').filter(Boolean));
}

export function strippedDirectoryPath(project) {
  const declared = project?.sync?.strippedPackageDirectory;
  if (typeof declared !== 'string' || declared === '' || declared.startsWith('/') || declared.includes('..')) {
    throw new Error(
      `[absorbed-sources] the project \`${project?.id}\` names \`sync.strippedPackageDirectory\` as ${JSON.stringify(declared)}, ` +
        'which is not a directory inside its declared root',
    );
  }
  return join(projectRoot(project), ...declared.replace(/\/+$/, '').split('/').filter(Boolean));
}

const STRIPPED_CARD = TREE_MANIFEST;

export function strippedAbsence(project) {
  const declared = project?.sync?.strippedPackageDirectory;
  if (declared === undefined) return null;
  const root = projectRoot(project);
  const within = strippedDirectoryPath(project);
  const segment = (name) => relative(root, join(within, name)).split('\\').join('/');
  if (!snapshotSections(project).has(segment(STRIPPED_CARD))) {
    throw new Error(
      `[absorbed-sources] ${snapshotPath(project)} carries no segment for ${segment(STRIPPED_CARD)}, and that is the card the ` +
        `deshell step takes ${declared} from; a directory nothing stood in is not one the tree gave up`,
    );
  }
  return { segment, directories: [segment('')], adds: [], removes: [segment(STRIPPED_CARD)] };
}

export function deshellBlockFiles() {
  const path = join(SCRIPT_DIR, 'deshell-manifest.json');
  const face = JSON.parse(readFileSync(path, 'utf8'));
  if (!Array.isArray(face.blocks) || face.blocks.length === 0) {
    throw new Error(`${path} names no \`blocks\`, and the blocks the deshell step takes out of a build input stand written there`);
  }
  const files = [];
  for (const block of face.blocks) {
    if (typeof block?.file !== 'string' || block.file === '') {
      throw new Error(`${path}: a block face carries no \`file\`, so nothing says which build input those lines stand in`);
    }
    if (!files.includes(block.file)) files.push(block.file);
  }
  return files;
}

export function strippedRootOf(project, name) {
  const absence = strippedAbsence(project);
  if (absence === null) return null;
  return absence.segment(name).split('/')[0];
}

export function hostDerivedPath(project) {
  const root = join(REPO_ROOT, ...String(project?.root ?? '').split('/').filter(Boolean));
  return join(root, ...packageManifestPath(project).split('/'));
}

export function buildConfigFiles(project) {
  const declared = project?.sync?.buildConfigFiles;
  if (!Array.isArray(declared) || declared.length === 0) {
    throw new Error(
      `[absorbed-sources] the project \`${project?.id}\` declares no \`sync.buildConfigFiles\`, so the take has no list of ` +
        'the build inputs it must find standing in that tree',
    );
  }
  for (const entry of declared) {
    if (typeof entry !== 'string' || entry === '' || entry.startsWith('/') || entry.includes('..')) {
      throw new Error(
        `[absorbed-sources] the project \`${project?.id}\` names \`sync.buildConfigFiles\` entry ${JSON.stringify(entry)}, ` +
          'which is not a path inside its declared root',
      );
    }
  }
  return [...declared];
}

export const MECHANISMS = Object.freeze(['cargo-workspace', 'single-crate']);

export function mechanism(project) {
  const declared = project?.sync?.mechanism;
  if (!MECHANISMS.includes(declared)) {
    throw new Error(
      `[absorbed-sources] the project \`${project?.id}\` declares \`sync.mechanism\` as ${JSON.stringify(declared)}, ` +
        `which is not one of ${MECHANISMS.join(', ')}`,
    );
  }
  return declared;
}

export function packageDirectoryRequirement(project) {
  if (mechanism(project) === 'cargo-workspace') return 'none';
  return project?.sync?.backendPackage === undefined && project?.sync?.strippedPackageDirectory === undefined
    ? 'none'
    : 'optional';
}

export function assertMechanismPairing(project) {
  const declared = project?.sync?.packageDirectory;
  const wants = packageDirectoryRequirement(project);
  const present = typeof declared === 'string' && declared !== '';
  if (wants === 'none' && present) {
    throw new Error(
      `[absorbed-sources] the project \`${project?.id}\` names no crate below its root, and it names ` +
        `\`sync.packageDirectory\` as ${JSON.stringify(declared)}: the tree's own crate stands at its root`,
    );
  }
  return wants;
}

export const ROOT_MEMBERSHIPS = Object.freeze(['pnpm-closure', 'mini-app-payload', 'own-resolver']);

export function frontendDestinations(project) {
  const declared = frontend(project);
  if (declared === null) return [];
  return declared.roots.map((root) => ({
    path: root.path === '.' ? project.root : `${project.root}/${root.path}`,
    membership: root.membership,
  }));
}

export function frontend(project) {
  const declared = project?.sync?.frontend;
  if (declared === undefined || declared === null) return null;
  if (typeof declared !== 'object' || Array.isArray(declared)) {
    throw new Error(`[absorbed-sources] the project \`${project?.id}\` declares \`sync.frontend\` as ${JSON.stringify(declared)}, which is not an object`);
  }
  const roots = declared.roots;
  if (!Array.isArray(roots) || roots.length === 0) {
    throw new Error(`[absorbed-sources] the project \`${project?.id}\` declares \`sync.frontend\` with no root, so the key describes nothing`);
  }
  for (const root of roots) {
    if (typeof root !== 'object' || root === null) {
      throw new Error(`[absorbed-sources] the project \`${project?.id}\` names a \`sync.frontend\` root as ${JSON.stringify(root)}, which does not say where it goes`);
    }
    if (typeof root.path !== 'string' || root.path === '' || root.path.startsWith('/') || root.path.includes('..')) {
      throw new Error(
        `[absorbed-sources] the project \`${project?.id}\` names \`sync.frontend\` root path ${JSON.stringify(root.path)}, ` +
          'which is not a path inside its declared root',
      );
    }
    if (!ROOT_MEMBERSHIPS.includes(root.membership)) {
      throw new Error(
        `[absorbed-sources] the project \`${project?.id}\` names \`sync.frontend\` root ${JSON.stringify(root.path)} with ` +
          `membership ${JSON.stringify(root.membership)}, which is not one of ${ROOT_MEMBERSHIPS.join(', ')}`,
      );
    }
  }
  return { roots: roots.map((root) => ({ path: root.path, membership: root.membership })) };
}

export function frontendRoots(project) {
  const declared = frontend(project);
  return declared === null ? [] : declared.roots.map((root) => root.path);
}

export function frontendMembership(project, root) {
  const declared = frontend(project);
  if (declared === null) {
    throw new Error(`[absorbed-sources] the project \`${project?.id}\` declares no \`sync.frontend\`, so ${JSON.stringify(root)} has no membership`);
  }
  const held = declared.roots.find((entry) => entry.path === root);
  if (held === undefined) {
    throw new Error(
      `[absorbed-sources] the project \`${project?.id}\` names no \`sync.frontend\` root ${JSON.stringify(root)} among ` +
        `${declared.roots.map((entry) => entry.path).join(', ')}, so nothing says where that frontend goes`,
    );
  }
  return held.membership;
}
