#!/usr/bin/env node

import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { contentOf, lineTerminator, readStringArray, splitLines, tableEntries, tableLines } from './toml-lines.mjs';
import { OWN_CRATES } from './own-crates.mjs';
import {
  TREE_MANIFEST,
  absorbedProjects,
  assertMechanismPairing,
  frontendRoots,
  mechanism,
  packageDirectory,
  projectRoot,
  snapshotMemberText,
  snapshotSections,
  stripRules,
  strippedAbsence,
} from './absorbed-sources.mjs';

const __dirname = dirname(fileURLToPath(import.meta.url));
const HOST_ROOT = resolve(__dirname, '..');
const HOST_MANIFEST = join(HOST_ROOT, 'Cargo.toml');
const HOST_LOCK = join(HOST_ROOT, 'Cargo.lock');
const HOST_ATTRIBUTES = join(HOST_ROOT, '.gitattributes');
const CONVERGENCE = join(__dirname, 'single-instance.toml');
const LANDING_IDENTITY = join(__dirname, 'landing-identity.toml');

const SENTINEL_PREFIX = '# [upstream-sync]';
const TARGET_ROOT = 'openalgo-desktop';

function convergenceDeclaration() {
  if (!existsSync(CONVERGENCE)) {
    throw new Error(`[sync-absorbed-members] the single-instance declaration is missing: ${CONVERGENCE}`);
  }
  const text = readFileSync(CONVERGENCE, 'utf8');
  const declared = { converged: new Map(), pathHeld: new Set() };
  const read = (name) => {
    const header = `[[${name}]]`;
    const entries = [];
    let inside = false;
    let current = null;
    for (const line of splitLines(text).map(contentOf)) {
      const trimmed = line.trim();
      if (trimmed.startsWith('[[')) {
        inside = trimmed === header;
        if (inside) {
          current = {};
          entries.push(current);
        }
        continue;
      }
      if (!inside || trimmed === '' || trimmed.startsWith('#')) continue;
      const key = line.match(/^\s*([A-Za-z0-9_-]+)\s*=\s*"([^"]*)"\s*$/);
      if (key === null) {
        throw new Error(`[sync-absorbed-members] ${CONVERGENCE}: no place for this line under ${header}: ${line}`);
      }
      current[key[1]] = key[2];
    }
    if (entries.length === 0) {
      throw new Error(`[sync-absorbed-members] ${CONVERGENCE}: ${header} stands nowhere in the declaration`);
    }
    return entries;
  };
  for (const entry of read('converged')) {
    for (const key of ['crate', 'into', 'holds', 'why']) {
      if (typeof entry[key] !== 'string' || entry[key] === '') {
        throw new Error(`[sync-absorbed-members] ${CONVERGENCE}: a [[converged]] entry carries no \`${key}\``);
      }
    }
    if (!['sole', 'subtree'].includes(entry.holds)) {
      throw new Error(
        `[sync-absorbed-members] ${CONVERGENCE}: \`${entry.crate}\` holds ${JSON.stringify(entry.holds)}, and a converged ` +
          'crate is either sole in the graph or alone but for the major versions another crate carries as its own',
      );
    }
    if (declared.converged.has(entry.crate)) {
      throw new Error(`[sync-absorbed-members] ${CONVERGENCE}: \`${entry.crate}\` is converged twice, and one crate resolves once`);
    }
    declared.converged.set(entry.crate, { into: entry.into, holds: entry.holds });
  }
  for (const entry of read('path_held')) {
    for (const key of ['crate', 'why']) {
      if (typeof entry[key] !== 'string' || entry[key] === '') {
        throw new Error(`[sync-absorbed-members] ${CONVERGENCE}: a [[path_held]] entry carries no \`${key}\``);
      }
    }
    if (declared.pathHeld.has(entry.crate)) {
      throw new Error(`[sync-absorbed-members] ${CONVERGENCE}: \`${entry.crate}\` is named twice in [[path_held]], and an edge is one key`);
    }
    if (declared.converged.has(entry.crate)) {
      throw new Error(
        `[sync-absorbed-members] ${CONVERGENCE}: \`${entry.crate}\` is converged and named as path-held, and a crate whose one ` +
          'instance is a path cannot also be one the generated entry states',
      );
    }
    declared.pathHeld.add(entry.crate);
  }
  if (declared.converged.size === 0 || declared.pathHeld.size === 0) {
    throw new Error(`[sync-absorbed-members] ${CONVERGENCE}: the declaration carries ${declared.converged.size} converged crate(s) and ${declared.pathHeld.size} path-held crate(s), and one of the two says nothing`);
  }
  return declared;
}

function contentLine(line) {
  const text = contentOf(line).trim();
  let quote = null;
  for (let i = 0; i < text.length; i += 1) {
    const character = text[i];
    if (quote !== null) {
      if (character === '\\' && quote === '"') {
        i += 1;
        continue;
      }
      if (character === quote) quote = null;
      continue;
    }
    if (character === '"' || character === "'") {
      quote = character;
      continue;
    }
    if (character === '#') return text.slice(0, i).trim();
  }
  return text;
}

function bracketAt(text, from) {
  let quote = null;
  let depth = 0;
  for (let i = from; i < text.length; i += 1) {
    const character = text[i];
    if (quote !== null) {
      if (character === '\\' && quote === '"') {
        i += 1;
        continue;
      }
      if (character === quote) quote = null;
      continue;
    }
    if (character === '"' || character === "'") {
      quote = character;
      continue;
    }
    if (depth === 0 && character !== '{' && character !== '[') return i;
    if (depth === 0 && character !== '{' && character !== '[') return i;
    if (character === '{' || character === '[') depth += 1;
    if (character === '}' || character === ']') {
      depth -= 1;
      if (depth === 0) return i;
    }
  }
  return -1;
}

function splitValues(text) {
  return splitAt(text, ',');
}

function splitAt(text, separator) {
  const parts = [];
  let quoted = null;
  let depth = 0;
  let head = 0;
  for (let i = 0; i < text.length; i += 1) {
    const character = text[i];
    if (quoted !== null) {
      if (character === '\\' && quoted === '"') {
        i += 1;
        continue;
      }
      if (character === quoted) quoted = null;
      continue;
    }
    if (character === '"' || character === "'") {
      quoted = character;
      continue;
    }
    if (character === '{' || character === '[') depth += 1;
    if (character === '}' || character === ']') depth -= 1;
    if (character === separator && depth === 0) {
      parts.push(text.slice(head, i).trim());
      head = i + 1;
    }
  }
  parts.push(text.slice(head).trim());
  return parts.filter((part) => part !== '');
}

function scalarValue(plain, where) {
  if (/^"{3}[\s\S]*"{3}$/.test(plain) || /^'{3}[\s\S]*'{3}$/.test(plain)) return;
  if (/^"(?:[^"\\]|\\.)*"$/.test(plain) || /^'[^']*'$/.test(plain)) return;
  if (/^[-+]?(?:\d+(?:\.\d+)?(?:[eE][-+]?\d+)?|inf|nan)$/.test(plain)) return;
  if (/^(?:true|false)$/.test(plain)) return;
  if (/^\d{4}-\d{2}-\d{2}(?:[T ]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:[Zz]|[-+]\d{2}:\d{2})?)?$/.test(plain)) return;
  if (/^\d{2}:\d{2}:\d{2}(?:\.\d+)?$/.test(plain)) return;
  throw new Error(`parseToml: ${where} carries a value this reader cannot place: ${plain}`);
}

function assertValue(text, where) {
  const plain = text.trim();
  if (!/^[\[{]/.test(plain)) {
    scalarValue(plain, where);
    return;
  }
  const close = bracketAt(plain, 0);
  if (close !== plain.length - 1) {
    throw new Error(`parseToml: ${where} carries a ${JSON.stringify(plain)} this reader cannot tell where it ends`);
  }
  for (const piece of splitValues(plain.slice(1, close))) {
    const pair = piece.match(/^(?:"(?:[^"\\]|\\.)*"|'[^']*'|[A-Za-z0-9_-]+(?:\.[A-Za-z0-9_-]+)*)\s*=\s*([\s\S]*)$/);
    const value = plain[0] === '[' || pair === null ? piece : pair[1];
    assertValue(value, where);
  }
}

export function parseToml(text) {
  const tables = new Set();
  const pending = [];
  const flush = () => {
    const stated = pending.find((entry) => entry.key !== null);
    if (stated === undefined) {
      pending.length = 0;
      return;
    }
    const where = pending.map((entry) => entry.raw.trim()).join('\n');
    const rest = pending
      .slice(pending.indexOf(stated))
      .map((entry, index) => (index === 0 ? entry.text.slice(entry.keyLength) : entry.text))
      .join('\n')
      .trim();
    assertValue(rest, where);
    pending.length = 0;
  };
  for (const line of splitLines(text)) {
    const bare = contentOf(line).trim();
    if (bare.startsWith('[[') || bare.startsWith('[')) {
      const header = bare.match(/^(?:(?:\[\[([^\]]+)\]\])|(?:\[([^\]]+)\]))$/);
      if (header === null) {
        throw new Error(`parseToml: this reader cannot place the table header: ${bare}`);
      }
      flush();
      tables.add((header[1] ?? header[2]).trim());
      continue;
    }
    const content = contentLine(line);
    if (content === '') continue;
    const assigned = content.match(/^([A-Za-z0-9_.-]+)\s*=\s*/);
    if (assigned !== null) {
      flush();
      pending.push({ text: content, raw: line, key: assigned[1], keyLength: assigned[0].length });
    } else {
      pending.push({ text: content, raw: line, key: null, keyLength: 0 });
    }
    const first = pending.find((entry) => entry.key !== null);
    if (first === undefined) continue;
    const rest = pending
      .slice(pending.indexOf(first))
      .map((entry, index) => (index === 0 ? entry.text.slice(entry.keyLength) : entry.text))
      .join('\n');
    if (bracketAt(rest, 0) === -1) continue;
    flush();
  }
  flush();
  return tables;
}

function patchKey(line) {
  const match = contentOf(line).match(/^\s*([A-Za-z0-9_-]+)\s*=/);
  return match === null ? null : match[1];
}

function convergedDependency(entries, declaration) {
  const written = entries.map((entry) => {
    const held = declaration.converged.get(entry.key);
    if (held === undefined) return entry;
    const { rest } = dependencyValue(entry);
    const value = held.holds === 'subtree' ? held.range : held.into;
    return rest === ''
      ? { ...entry, value: `"${value}"`, lines: [`${entry.indent}${entry.key} = "${value}"`] }
      : { ...entry, value: `{ version = "${value}", ${rest} }`, lines: [`${entry.indent}${entry.key} = { version = "${value}", ${rest} }`] };
  });
  const listed = new Set(written.map((entry) => entry.key));
  for (const [key, held] of declaration.converged) {
    if (listed.has(key)) continue;
    const value = held.holds === 'subtree' ? held.range : held.into;
    written.push({ key, indent: '', value: `"${value}"`, lines: [`${key} = "${value}"`] });
  }
  return written;
}

function closesSegment(line) {
  return line.endsWith(' end') && sentinelStem(line)?.kind === 'end';
}

function carriedByRow(lines) {
  const carried = [];
  let under = false;
  for (const line of lines) {
    if (!under) {
      if (line.startsWith(SENTINEL_PREFIX) && !closesSegment(line)) {
        under = true;
        carried.push(line);
      }
      continue;
    }
    if (closesSegment(line)) break;
    carried.push(line);
  }
  return carried;
}

function patchGroupsOf(lines, snapshotName) {
  const entries = [];
  const held = [];
  const closes = [];
  for (const line of lines) {
    const key = patchKey(line);
    if (key !== null) {
      entries.push({ key, lines: [...held, line], closed: closes.length !== 0 });
      held.length = 0;
      closes.length = 0;
      continue;
    }
    if (closesSegment(line)) closes.push(line);
    held.push(line);
  }
  if (entries.length === 0) return entries;
  const carried = held.filter((line) => line.startsWith(SENTINEL_PREFIX) && !closesSegment(line));
  if (carried.length !== 0) {
    throw new Error(
      `host manifest: ${JSON.stringify(contentOf(carried[carried.length - 1]))} stands below the last \`[patch.crates-io]\` entry, and ` +
        'that row opens the segment the tree after it writes: an entry written below it would answer for that tree',
    );
  }
  const groups = [];
  for (const entry of entries) {
    if (entry.closed) continue;
    const own = carriedByRow(entry.lines);
    groups.push({ key: entry.key, lines: own.length === 0 ? entry.lines : own });
  }
  return groups;
}

function pendingRequirement(graph, project, key) {
  const root = `${HOST_ROOT.replace(/\\/g, '/')}/${project.root}`;
  const said = graph?.pending.get(`${root}\u0000${key}`);
  return said === undefined ? null : said;
}

function convergedElements(declaration, graph, project) {
  const held = new Map();
  for (const [key, value] of declaration.converged) {
    const demanded = pendingRequirement(graph, project, key);
    if (demanded === null) continue;
    held.set(key, { snapshotName: project.snapshotFile, version: demanded, range: statedRange(demanded) });
  }
  return { converged: held, pathHeld: declaration.pathHeld };
}

function convergedShared(declaration, statements) {
  const converged = new Map();
  for (const statement of statements) {
    for (const [key, value] of statement.converged) {
      const held = converged.get(key);
      converged.set(key, held === undefined ? [value] : [...held, value]);
    }
  }
  const declared = declaration.converged;
  const shared = new Map();
  for (const [key, value] of declared) {
    const stated = converged.get(key);
    if (stated === undefined) continue;
    if (value.holds === 'sole') {
      if (stated.length !== 1) {
        throw new Error(
          `[sync-absorbed-members] ${CONVERGENCE} holds \`${key}\` sole, and ` +
            `${stated.map((each) => each.snapshotName).join(' and ')} both name it: \`sole\` says one tree names the crate ` +
            'and the other does not, so a crate both trees name is converged by the one range both name or not at all',
        );
      }
      shared.set(key, { ...value, into: stated[0].version });
      continue;
    }
    shared.set(key, { ...value, range: sharedRange(key, stated) });
  }
  return { converged: shared, pathHeld: declaration.pathHeld };
}

function assertConvergenceCarried(declaration, expected, graph) {
  const kept = new Set();
  for (const { project, segments } of expected) {
    for (const group of segments.patches ?? []) {
      kept.add(`${project.snapshotFile}\u0000${group.key}`);
    }
  }
  const carried = [];
  for (const { project } of expected) {
    const snapshotName = project.snapshotFile;
    const text = snapshotMemberText(project);
    if (!/^[ \t]*\[patch\.crates-io\]/m.test(text)) continue;
    for (const entry of tableEntries(text, 'patch.crates-io')) {
      if (declaration.pathHeld.has(entry.key)) carried.push(`${snapshotName}: ${entry.key}`);
    }
  }
  if (carried.length !== declaration.pathHeld.size) {
    throw new Error(
      `[sync-absorbed-members] ${CONVERGENCE} names ${declaration.pathHeld.size} path-held crate(s), and the trees ` +
        `declare ${carried.length} of them:\n${carried.join('\n')}\n` +
        'A crate the trees no longer hold by a path edge would leave this step reporting an edge that is not there.',
    );
  }
  for (const one of carried) {
    const [snapshotName, key] = one.split(': ');
    const generated = kept.has(`${snapshotName}\u0000${key}`);
    if (!generated) {
      throw new Error(
        `[sync-absorbed-members] ${CONVERGENCE} names \`${key}\` path-held, and ${snapshotName}'s generated segment drops ` +
          'that edge: the path is what holds that one instance, so the edge stating it is the edge the mirror carries',
      );
    }
  }
  const unseen = [];
  for (const key of declaration.converged.keys()) {
    const demanding = expected.filter(({ project }) => requiresInGraph(graph, project, key));
    if (demanding.length === 0) continue;
    const held = demanding.some(({ segments }) => (segments.dependencies ?? []).some((entry) => entry.key === key));
    if (!held) unseen.push(`${key} (${demanding.length} tree(s) demand it and no generated entry carries it)`);
  }
  if (unseen.length !== 0) {
    throw new Error(
      `[sync-absorbed-members] ${CONVERGENCE} converges ${unseen.join(', ')}, so the converged value would reach nothing.`,
    );
  }
}

function requiresInGraph(graph, project, key) {
  return pendingRequirement(graph, project, key) !== null;
}

function snapshotDeclaresWorkspaceText(text) {
  return /^[ \t]*\[workspace\]/m.test(text);
}

function seamMemberLines() {
  return [
    '# This repository\'s own seam crates. They take in no absorbed tree, so they are',
    '# written once here and named after no tree.',
    ...OWN_CRATES.map((entry) => `"${entry}",`),
  ];
}

const REGIONS = [
  { key: 'absorbed members', file: 'manifest', holder: 'tree', table: 'workspace', array: 'members', padding: 0 },
  { key: 'seam members', file: 'manifest', holder: 'none', table: 'workspace', array: 'members', padding: 0 },
  { key: 'absorbed excludes', file: 'manifest', holder: 'all', table: 'workspace', array: 'exclude', padding: 0 },
  { key: 'absorbed workspace data', file: 'manifest', holder: 'all', table: 'workspace.dependencies', array: null, padding: 0 },
  { key: 'absorbed patches', file: 'manifest', holder: 'tree', table: 'patch.crates-io', array: null, padding: 0 },
  { key: 'absorbed profiles', file: 'manifest', holder: 'all', table: null, array: null, padding: 0 },
  { key: 'absorbed byte fidelity', file: 'attributes', holder: 'tree', table: null, array: null, padding: 0 },
];

const MIRROR_MEMBERS = new Set(['absorbed members', 'seam members']);

const MARKER_OPEN = ' (generated; do not edit by hand)';

function regionStem(key, holder) {
  return `${key}${holder === '' ? '' : `: ${holder}`}`;
}

function holderOf(region, stem, declared, treeIds) {
  for (const holder of declared) {
    if (stem === regionStem(region.key, holder)) return holder;
  }
  if (stem === region.key && declared.has('')) return '';
  if (region.holder === 'tree') {
    for (const treeId of treeIds) {
      if (stem === `${region.key} ${treeId}` || stem === `${region.key}: ${treeId}`) return treeId;
    }
    return undefined;
  }
  if (region.holder === 'all' && stem === region.key) return '';
  for (const treeId of treeIds) {
    if (stem === `${region.key} ${treeId}` || stem === `${region.key}: ${treeId}`) return '';
  }
  return undefined;
}

function markerName(key, holder) {
  return `${SENTINEL_PREFIX} ${regionStem(key, holder)}`;
}

function markers(holder, key) {
  const name = markerName(key, holder);
  return {
    absent: `${name} absent`,
    begin: `${name}${MARKER_OPEN}`,
    end: `${name} end`,
  };
}

function sentinelStem(line) {
  const read = line.match(/^# \[upstream-sync\] (.+)$/);
  if (read === null) return null;
  if (read[1].endsWith(' (generated; do not edit by hand)')) {
    return { stem: read[1].slice(0, -' (generated; do not edit by hand)'.length), kind: 'open' };
  }
  if (read[1].endsWith(' end')) {
    return { stem: read[1].slice(0, -' end'.length), kind: 'end' };
  }
  if (read[1].endsWith(' absent')) {
    return { stem: read[1].slice(0, -' absent'.length), kind: 'absent' };
  }
  return { stem: read[1], kind: 'note' };}

function spansFor(content) {
  const spans = [];
  let cursor = 0;
  while (cursor < content.length) {
    const at = content.findIndex((line, index) => index >= cursor && ['open', 'absent'].includes(sentinelStem(line)?.kind));
    if (at === -1) break;
    const { stem, kind } = sentinelStem(content[at]);
    if (kind === 'absent') {
      spans.push({ from: at, to: at, stem });
      cursor = at + 1;
      continue;
    }
    const close = content.findIndex((line, index) => index > at && line === `# [upstream-sync] ${stem} end`);
    if (close === -1) throw new Error(`host manifest: end sentinel not found for ${content[at]}`);
    const inner = content
      .slice(at + 1, close)
      .some((line) => sentinelStem(line)?.kind === 'open');
    if (inner) {
      cursor = at + 1;
      continue;
    }
    spans.push({ from: at, to: close, stem });
    cursor = close + 1;
  }
  return spans;
}

export function writeRegions(manifestText, attributesText, sections, treeIds) {
  const files = new Map([
    ['manifest', { terminator: lineTerminator(manifestText), lines: splitLines(manifestText) }],
    ['attributes', { terminator: lineTerminator(attributesText), lines: splitLines(attributesText) }],
  ]);
  const namesOf = new Map();
  for (const region of REGIONS) {
    if (!files.has(region.file)) {
      throw new Error(`host manifest: ${region.key} names the file ${JSON.stringify(region.file)}, which this step does not write`);
    }
    namesOf.set(region.key, new Set(sections
      .filter((section) => section.key === region.key)
      .map(({ holder }) => holder)));
  }
  for (const region of REGIONS) {
    const file = files.get(region.file);
    const content = file.lines.map(contentOf);
    if (region.table !== null && !content.some((line) => line.trim() === `[${region.table}]`)) {
      throw new Error(`host manifest: ${region.key} stands nowhere and its table [${region.table}] is not in the file`);
    }
    if (region.array !== null && arrayAnchor(content, region.array, region.table === null ? 0 : content.findIndex((line) => line.trim() === `[${region.table}]`) + 1) === -1) {
      throw new Error(`host manifest: ${region.key} stands nowhere and its array ${region.array} = [ is not in the file`);
    }
  }
  const claimed = new Set();
  const found = new Map();
  for (const region of REGIONS) {
    const file = files.get(region.file);
    for (const span of spansFor(file.lines.map(contentOf))) {
      if (claimed.has(`${region.file}\u0000${span.stem}`)) continue;
      const holder = holderOf(region, span.stem, namesOf.get(region.key), treeIds);
      if (holder === undefined) continue;
      const at = `${region.file}\u0000${regionStem(region.key, holder)}`;
      if (region.holder === 'all' && found.has(at)) {
        claimed.add(`${region.file}\u0000${span.stem}`);
        continue;
      }
      if (found.has(at)) {
        throw new Error(`host manifest: ${regionStem(region.key, holder)} stands more than once in ${region.file}, and one region stands once`);
      }
      claimed.add(`${region.file}\u0000${span.stem}`);
      found.set(at, { region, span, holder });
    }
  }
  const rank = (file, region, holder) => {
    const span = found.get(`${file}\u0000${regionStem(region.key, holder)}`);
    return span === undefined ? Number.MAX_SAFE_INTEGER : span.span.from;
  };
  const located = { manifest: [], attributes: [] };
  for (const [name, entry] of found) {
    const [file] = name.split('\u0000');
    located[file].push(entry);
  }
  const all = [...found.values()];
  const plans = [];
  for (const [name, file] of files) {
    const spans = [...located[name]].sort((left, right) => left.span.from - right.span.from);
    let cursor = 0;
    for (const region of REGIONS.filter((entry) => entry.file === name)) {
      const held = spans.filter((entry) => entry.region.key === region.key);
      const owned = sections
        .filter((entry) => entry.key === region.key)
        .sort((left, right) => rank(name, region, left.holder) - rank(name, region, right.holder));
      const live = owned.filter((section) => section.body !== null).length;
      const blocks = owned.map((section) => ({
        holder: section.holder,
        rows: sectionBlock(section).map((line) => line + file.terminator),
      }));
      if (held.length === 0) {
        const at = insertionPoint(file.lines, region, cursor);
        plans.push({ file: name, region, blocks, at: at.at, span: null, live, swallowed: [] });
        if (live > 0) cursor = at.at;
        continue;
      }
      const from = held[0].span.from;
      const to = held[held.length - 1].span.to;
      const swallowed = all.filter(
        (entry) =>
          entry.region.file === name &&
          entry.region.key !== region.key &&
          held.some((own) => entry.span.from > own.span.from && entry.span.to <= own.span.to),
      );
      plans.push({
        file: name,
        region,
        blocks,
        at: from,
        span: { from, to },
        live,
        swallowed,
        slots: held.map((entry) => ({ from: entry.span.from, to: entry.span.to, holder: entry.holder })),
      });
    }
  }
  for (const plan of plans) {
    if (plan.swallowed.length === 0) continue;
    throw new Error(
      `host manifest: ${plan.region.key} would take in ${plan.swallowed.map((entry) => entry.region.key).join(', ')}, ` +
        'which stand under other keys, and one region replaces only its own lines',
    );
  }
  const claim = new Map();
  for (const plan of plans) {
    if (plan.live === 0) continue;
    const held = claim.get(`${plan.file}\u0000${plan.at}`);
    if (held !== undefined && plan.span === null) {
      throw new Error(
        `host manifest: ${held.region.key} and ${plan.region.key} both write the line after ` +
          `${JSON.stringify(contentOf(files.get(plan.file).lines[plan.at]))} in the ${plan.file} file, and one line answers one region`,
      );
    }
    claim.set(`${plan.file}\u0000${plan.at}`, plan);
  }
  const edits = [];
  for (const plan of plans) {
    if (plan.span === null) {
      const rows = plan.blocks.flatMap((block) => block.rows);
      if (rows.length > 0) {
        edits.push({ file: plan.file, from: plan.at + 1, to: plan.at, rows });
      }
      continue;
    }
    for (const slot of plan.slots) {
      const rows = plan.blocks.filter((block) => block.holder === slot.holder).flatMap((block) => block.rows);
      edits.push({ file: plan.file, from: slot.from, to: slot.to, rows });
    }
  }
  for (const edit of edits.sort((left, right) => right.from - left.from)) {
    files.get(edit.file).lines.splice(edit.from, edit.to - edit.from + 1, ...edit.rows);
  }
  return { manifest: files.get('manifest').lines.join('\n'), attributes: files.get('attributes').lines.join('\n') };
}

function insertionPoint(lines, region, cursor) {
  const content = lines.map(contentOf);
  const header =
    region.table === null
      ? content.length
      : content.findIndex((line) => line.trim() === `[${region.table}]`) + 1;
  if (header <= 0) {
    throw new Error(`host manifest: ${region.key} stands nowhere and its table [${region.table}] is not in the file`);
  }
  const close = nextHeader(content, header);
  if (region.array === null) return { kind: 'at', at: Math.max(cursor, close - 1) };
  const opened = arrayAnchor(content, region.array, header);
  if (opened === -1) {
    throw new Error(`host manifest: ${region.key} stands nowhere and its array ${region.array} = [ is not in the table`);
  }
  const shut = closingBracket(content, opened, close);
  if (shut === -1) {
    throw new Error(`host manifest: ${region.array} = [ is never closed in the table`);
  }
  const at = Math.max(cursor, shut - 1);
  return { kind: 'at', at };
}

function closingBracket(content, from, limit) {
  let depth = 0;
  for (let i = from; i < limit; i += 1) {
    const line = content[i];
    for (const character of line) {
      if (character === '[') depth += 1;
      if (character === ']') {
        depth -= 1;
        if (depth === 0) return i;
      }
    }
  }
  return -1;
}

function nextHeader(content, from) {
  for (let i = from; i < content.length; i += 1) {
    if (content[i].trim().startsWith('[')) return i;
  }
  return content.length;
}

function generatedNote(snapshotName) {
  return [
    `# Generated by upstream-sync from ${snapshotName}.`,
    '# The host workspace root adopts these entries verbatim. Do not edit by hand:',
    '# the next take regenerates the segment and any edit here is lost.',
  ];
}

function dataNote(snapshotNames) {
  return [
    '# Dependency entries the absorbed crates resolve through `workspace = true`.',
    '# Only keys the host root does not already define are listed, and one key stands once:',
    '# where two trees both name it, the entry below stands only when one range admits both,',
    '# and a note above the entry says which trees it came from.',
    `# Generated by upstream-sync from ${snapshotNames.join(' and ')}.`,
  ];
}

function arrayAnchor(content, name, from) {
  const open = new RegExp(`^\\s*${name}\\s*=\\s*\\[`);
  const limit = nextHeader(content, from);
  for (let i = from; i < limit; i += 1) {
    if (open.test(content[i])) return i;
  }
  return -1;
}

function sectionBlock(section) {
  const { marker, body } = section;
  return body === null
    ? [marker.absent]
    : [
      marker.begin,
      ...body,
      ...Array(section.padding).fill(''),
      marker.end,
    ];
}

function staleSnapshotEntries(snapshotText, sourceName, treeRoot) {
  const stale = [];
  if (!snapshotDeclaresWorkspaceText(snapshotText)) return stale;
  for (const array of ['members', 'exclude']) {
    for (const entry of readStringArray(snapshotText, array, sourceName)) {
      const target = join(treeRoot, ...entry.split('/'));
      let present = false;
      try {
        present = statSync(target).isDirectory();
      } catch {
        present = false;
      }
      if (!present) stale.push({ array, entry });
    }
  }
  return stale;
}

function pruneSnapshot(snapshotText, stale, sourceName) {
  const gone = new Set(stale.map(({ array, entry }) => `${array}\u0000${entry}`));
  const lines = splitLines(snapshotText);
  const out = [];
  let array = null;
  for (const line of lines) {
    const opened = line.match(/^\s*(members|exclude)\s*=\s*\[/);
    if (opened) {
      array = opened[1];
      out.push(line);
      continue;
    }
    if (array !== null && /^\s*\]/.test(line)) {
      array = null;
      out.push(line);
      continue;
    }
    if (array !== null) {
      const quoted = contentOf(line).trim().match(/^"([^"]+)"\s*,?$/);
      if (quoted && gone.has(`${array}\u0000${quoted[1]}`)) continue;
    }
    out.push(line);
  }
  const text = out.join('\n');
  if (readStringArray(text, 'members', sourceName).length === 0) {
    throw new Error(
      `${sourceName}: pruning every stale entry would leave the members array empty. That describes a tree with no crates at all`,
    );
  }
  return text;
}

function buildByteFidelityLines(project) {
  return [`${project.root}/** -text`];
}

function tableLinesWithoutComments(text, table) {
  return tableLines(text, table).filter((line) => !/^\s*#/.test(line) && line.trim() !== '');
}

function rewritePathValues(line, replacer) {
  const valuePath = new RegExp(`("|')([^"']+)("|')`, 'g');
  return line.replace(valuePath, (match, open, target, close) => replacer(open, target, close, line));
}

function reRootPathValue(open, target, close, line, base) {
  const normalized = target.replace(/\\/g, '/');
  const local = isFilePath(line) && !/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(normalized) && !normalized.startsWith('$');
  if (!local) return `${open}${normalized}${close}`;
  return normalized.startsWith(`${base}/`) || normalized === base
    ? `${open}${normalized}${close}`
    : `${open}${base}/${normalized}${close}`;
}

function isFilePath(line) {
  return /(^|[\s{,])(path|members|exclude|default-members)\s*=/.test(line);
}

function crateBase(project) {
  return [project.root, packageDirectory(project)].filter((segment) => segment !== '').join('/');
}

function entryReachesShell(project) {
  const fronts = frontendRoots(project).map((root) => (root === '.' ? project.root : `${project.root}/${root}`));
  for (const entry of crateTargets(project)) {
    for (const front of fronts) {
      if (entry === front || entry.startsWith(`${front}/`)) return true;
    }
  }
  return false;
}

function crateTargets(project) {
  const root = projectRoot(project).replace(/\\/g, '/');
  const entries = [];
  let holdsTargets = false;
  for (const line of splitLines(snapshotMemberText(project)).map(contentOf)) {
    const trimmed = line.trim();
    const header = trimmed.match(/^\[([^\]]+)\]$/);
    if (header !== null) {
      holdsTargets = /(?:^|\.)(?:package|lib|bin|example|test|bench)$/.test(header[1].trim());
      continue;
    }
    if (!holdsTargets || trimmed === '' || trimmed.startsWith('#')) continue;
    const assigned = trimmed.match(/^([A-Za-z0-9_-]+)\s*=/);
    if (assigned === null || assigned[1] !== 'path') continue;
    const quoted = trimmed.slice(assigned[0].length).trim().match(/^"([^"]*)"$/);
    if (quoted === null) continue;
    const target = quoted[1].replace(/\\/g, '/');
    if (/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(target) || target.startsWith('$')) continue;
    const resolved = target.startsWith('/') ? target : `${root}/${target}`;
    const inside = [];
    for (const part of resolved.split('/')) {
      if (part === '' || part === '.') continue;
      if (part === '..') inside.pop();
      else inside.push(part);
    }
    const at = inside.join('/');
    if (!at.startsWith(`${root}/`)) continue;
    entries.push(at.slice(root.length + 1));
  }
  return entries;
}

function buildPatchLines(absorbedText, sourceName, project) {
  const rows = tableLines(absorbedText, 'patch.crates-io');
  if (rows.every((line) => line.trim() === '' || line.trim().startsWith('#'))) return null;
  const base = crateBase(project);
  const rerooted = [];
  const open = [];
  let treeLocal = 0;
  let unpinned = 0;
  for (const line of rows) {
    if (line.trim() === '') continue;
    if (line.trim().startsWith('#')) {
      open.push(line);
      continue;
    }
    const next = rewritePathValues(line, (quote, target, close, current) =>
      reRootPathValue(quote, target, close, current, base));
    if (next !== line) treeLocal += 1;
    const released = unpinnedRest(next);
    if (released !== next) unpinned += 1;
    rerooted.push(...open.splice(0), released);
  }
  if (treeLocal === 0 && unpinned === 0) {
    throw new Error(
      `${sourceName}: [patch.crates-io] carries no tree-local path entry to re-root and no git entry to release from a pin. ` +
        'Without either rewrite this segment would be a copy',
    );
  }
  return rerooted;
}

function profileTables(absorbedText) {
  const lines = splitLines(absorbedText).map(contentOf);
  const tables = [];
  let current = null;
  for (const line of lines) {
    const header = line.match(/^\s*(\[\[?)([^\]]+)(\]\]?)\s*$/);
    if (header) {
      current = header[1] === '[' && header[2].trim().startsWith('profile.') ? { header: line, body: [] } : null;
      if (current !== null) tables.push(current);
      continue;
    }
    if (current !== null) current.body.push(line);
  }
  const blocks = [];
  for (const table of tables) {
    const body = [];
    let comment = [];
    for (const line of table.body) {
      if (/^\s*#/.test(line)) {
        comment.push(line);
        continue;
      }
      if (line.trim() === '') {
        comment = [];
        continue;
      }
      body.push(...comment, line);
      comment = [];
    }
    if (body.length > 0) blocks.push([table.header, ...body]);
  }
  return blocks;
}

function bare(value) {
  const text = String(value);
  const quoted = text.match(/^"([^"]*)"$/) ?? text.match(/^'([^']*)'$/);
  return quoted === null ? text : quoted[1];
}

function profileHeader(line) {
  const header = line.trim().match(/^\[(profile\..+)\]$/);
  if (header === null) {
    throw new Error(`profile: ${JSON.stringify(line)} is not a profile table header this step can carry`);
  }
  const segments = [];
  let segment = '';
  let quote = null;
  for (const character of header[1]) {
    if (quote !== null) {
      if (character === quote) {
        quote = null;
        continue;
      }
      segment += character;
      continue;
    }
    if (character === '"' || character === "'") {
      if (segment.trim() !== '') {
        throw new Error(`profile: ${JSON.stringify(line)} opens a quote inside a segment, which this step cannot carry`);
      }
      quote = character;
      continue;
    }
    if (character === '.') {
      segments.push(segment);
      segment = '';
      continue;
    }
    segment += character;
  }
  if (quote !== null) {
    throw new Error(`profile: ${JSON.stringify(line)} leaves a quote unclosed`);
  }
  segments.push(segment);
  const rebuilt = segments.map((entry) => {
    const name = entry.trim();
    if (name === '') {
      throw new Error(`profile: ${JSON.stringify(line)} carries an empty segment, which names no table`);
    }
    return /^[A-Za-z0-9_-]+$/.test(name) ? name : JSON.stringify(name);
  });
  return `[${rebuilt.join('.')}]`;
}

const PROFILE_ORDER = ['opt-level', 'debug', 'strip', 'debug-assertions', 'overflow-checks', 'lto', 'panic', 'incremental', 'codegen-units', 'rpath'];

const PROFILE_STRICTER = {
  'opt-level': (strict) => {
    const rank = (value) => {
      const plain = bare(value);
      const named = { z: -1, s: 0, 0: 1, 1: 2, 2: 3, 3: 4 };
      if (Object.prototype.hasOwnProperty.call(named, plain)) return named[plain];
      return Number.isNaN(Number(plain)) ? undefined : Number(plain) + 1;
    };
    const left = rank(strict.left);
    const right = rank(strict.right);
    if (left === undefined || right === undefined) {
      throw new Error(`profile: \`opt-level\` is ${JSON.stringify(strict.left)} in one tree and ${JSON.stringify(strict.right)} in the other, and this step knows no order between them`);
    }
    return left >= right ? strict.left : strict.right;
  },
  lto(strict) {
    const rank = { false: 0, off: 0, 'thin': 1, thin: 1, 'fat': 2, fat: 2, true: 2, on: 2 };
    const left = rank[bare(strict.left)];
    const right = rank[bare(strict.right)];
    if (left === undefined || right === undefined) {
      throw new Error(`profile: \`lto\` is ${JSON.stringify(strict.left)} in one tree and ${JSON.stringify(strict.right)} in the other, and neither is stricter than the other`);
    }
    return left >= right ? strict.left : strict.right;
  },
  inherits(strict) {
    if (bare(strict.left) !== bare(strict.right)) {
      throw new Error(`profile: the two trees inherit this profile from ${JSON.stringify(strict.left)} and ${JSON.stringify(strict.right)}, and two parents are not one profile`);
    }
    return strict.left;
  },
  'codegen-units': (strict) => {
    const left = Number(bare(strict.left));
    const right = Number(bare(strict.right));
    if (Number.isNaN(left) || Number.isNaN(right)) {
      throw new Error(`profile: \`codegen-units\` is ${JSON.stringify(strict.left)} in one tree and ${JSON.stringify(strict.right)} in the other, and this step knows no order between them`);
    }
    return left <= right ? strict.left : strict.right;
  },
  debug(strict) {
    const rank = { false: 0, off: 0, 0: 0, 'line-tables-only': 1, limited: 2, true: 3, full: 4, 2: 4 };
    const left = rank[bare(strict.left)];
    const right = rank[bare(strict.right)];
    if (left === undefined || right === undefined) {
      throw new Error(`profile: \`debug\` is ${JSON.stringify(strict.left)} in one tree and ${JSON.stringify(strict.right)} in the other, and neither is stricter than the other`);
    }
    return left >= right ? strict.left : strict.right;
  },
};

function parseBoolean(value) {
  const plain = bare(value);
  if (plain === 'true') return true;
  if (plain === 'false') return false;
  return null;
}

function strictlyBetter(key, left, right) {
  if (bare(left) === bare(right)) return left;
  if (key === 'panic') {
    throw new Error(`profile: \`panic\` is ${JSON.stringify(left)} in one tree and ${JSON.stringify(right)} in the other, and no value is stricter than the other`);
  }
  const rule = PROFILE_STRICTER[key];
  if (rule !== undefined) return rule({ left, right });
  const leftIsBoolean = parseBoolean(left) !== null;
  const rightIsBoolean = parseBoolean(right) !== null;
  if (leftIsBoolean && rightIsBoolean) {
    return parseBoolean(left) ? left : right;
  }
  if (key === 'incremental') {
    return parseBoolean(left) === false ? left : right;
  }
  throw new Error(
    `profile: \`${key}\` is ${JSON.stringify(left)} in one tree and ${JSON.stringify(right)} in the other, and this step knows no order between them`,
  );
}

function profileValue(line) {
  const at = line.indexOf('=');
  return { key: line.slice(0, at).trim(), value: line.slice(at + 1).trim() };
}

function mergeProfileTables(collected) {
  const merged = new Map();
  for (const { snapshotName, blocks } of collected) {
    for (const block of blocks) {
      const header = profileHeader(block[0]);
      const entries = block
        .slice(1)
        .map((line) => rewritePathValues(line, (open, target, close, current) => reRootPathValue(open, target, close, current, '')))
        .filter((line) => !/^\s*#/.test(line) && line.trim() !== '');
      if (!merged.has(header)) {
        merged.set(header, { trees: [snapshotName], entries: new Map() });
      }
      const table = merged.get(header);
      if (!table.trees.includes(snapshotName)) table.trees.push(snapshotName);
      for (const line of entries) {
        const { key, value } = profileValue(line);
        if (!table.entries.has(key)) {
          table.entries.set(key, { value, from: snapshotName });
          continue;
        }
        const held = table.entries.get(key);
        const better = strictlyBetter(key, held.value, value);
        if (better === null) {
          throw new Error(
            `profile: ${header} \`${key}\` is ${JSON.stringify(held.value)} in ${held.from} and ${JSON.stringify(value)} in ` +
              `${snapshotName}, and neither is the stricter value, so the two trees cannot be one profile`,
          );
        }
        table.entries.set(key, { value: better, from: held.from });
      }
    }
  }
  const order = (key) => {
    const at = PROFILE_ORDER.indexOf(key);
    return at === -1 ? PROFILE_ORDER.length : at;
  };
  const rows = [];
  for (const [header, table] of merged) {
    const keys = [...table.entries.keys()].sort((a, b) => order(a) - order(b));
    const sources = table.trees.length === 1
      ? `carried by ${table.trees[0]}, the only tree that declares it`
      : `${table.trees.join(' and ')} both declare it: every key both declare holds the stricter of the two values, and a key only one declares stands as it is`;
    rows.push('', header, `# [upstream-sync] ${sources}`);
    for (const key of keys) rows.push(`${key} = ${table.entries.get(key).value}`);
  }
  return rows;
}

function buildProfileLines(collected) {
  const withBlocks = collected.filter(({ blocks }) => blocks.length > 0);
  if (withBlocks.length === 0) return null;
  const rows = mergeProfileTables(withBlocks);
  return [
    '# Profiles the absorbed crates declare: a profile is a workspace-wide setting, so',
    '# a crate root cargo would ignore carries each one here instead, and two trees that',
    '# declare the same profile become one table rather than two answers to one setting.',
    ...rows,
  ];
}

function ownDependencyEntries(absorbedText, hostOwn, snapshotName) {
  if (!/^[ \t]*\[workspace\.dependencies\]/m.test(absorbedText)) return [];
  const declared = tableEntries(absorbedText, 'workspace.dependencies').filter((entry) => !hostOwn.has(entry.key));
  const demanded = new Set(workspaceTrueDemands());
  const reached = declared.filter((entry) => demanded.has(entry.key) || demanded.has(renamedPackage(entry.value)));
  for (const entry of declared) {
    if (reached.includes(entry)) continue;
    console.log(
      `[sync-absorbed-members] ${snapshotName}: [workspace.dependencies] \`${entry.key}\` stands reached by no member of any ` +
        'absorbed tree as `workspace = true`, so the mirror generates no entry for it and the key stays in the manifest that ' +
        'declares it',
    );
  }
  return reached;
}

function renamedPackage(value) {
  const named = value.match(/package\s*=\s*"([^"]*)"/);
  return named === null ? '' : named[1];
}

function memberTexts(project) {
  const held = snapshotSections(project);
  const texts = [];
  for (const [where, section] of held) {
    if (where.split('/').pop() !== TREE_MANIFEST) continue;
    texts.push(section.text);
  }
  return texts;
}

function workspaceTrueDemands() {
  const demanded = new Set();
  for (const project of absorbedProjects()) {
    for (const text of memberTexts(project)) {
      for (const line of splitLines(text).map(contentOf)) {
        const match = line.match(/^\s*([A-Za-z0-9_-]+)\s*=\s*\{\s*workspace\s*=\s*true/);
        if (match !== null) demanded.add(match[1]);
      }
    }
  }
  return demanded;
}

function dependencyValue(entry) {
  const version = entry.value.match(/version\s*=\s*"([^"]*)"/);
  if (version !== null) {
    const rest = entry.value
      .replace(/^\{/, '')
      .replace(/\}$/, '')
      .replace(/version\s*=\s*"[^"]*"\s*,?/, '')
      .replace(/,\s*$/, '')
      .trim();
    return { kind: 'table', version: version[1], rest };
  }
  const bare = entry.value.match(/^"([^"]*)"$/);
  if (bare !== null) return { kind: 'bare', version: bare[1], rest: '' };
  return { kind: 'opaque', version: null, rest: entry.value };
}

function unpinnedRest(rest) {
  if (!/\bgit\s*=\s*["']/.test(rest)) return rest;
  const pin = String.raw`\b(?:rev|tag)\s*=\s*['"][^'"]*['"]`;
  const released = rest
    .replace(new RegExp(`${pin}\\s*,\\s*`), '')
    .replace(new RegExp(`,\\s*${pin}`), '')
    .replace(new RegExp(`\\{\\s*${pin}\\s*\\}`), '{}')
    .trim();
  if (new RegExp(pin).test(released)) {
    throw new Error(
      `workspace data: ${JSON.stringify(rest)} pins a git dependency in a place this step cannot lift out of one line, ` +
        'and the tree it stands in is read as it is',
    );
  }
  return released.trim();
}

function renderDependency(key, merged) {
  const { kind, version, rest, lines, rewritten } = merged;
  if (!rewritten) return lines;
  if (kind === 'bare') return [`${key} = "${version}"`];
  const parts = [`version = "${version}"`];
  if (rest !== '') parts.push(rest);
  return [`${key} = { ${parts.join(', ')} }`];
}

function renderDependencyRows(merged) {
  if (merged === null) return null;
  for (const [key, entry] of merged) {
    if (unpinnedRest(entry.rest) !== entry.rest) {
      throw new Error(
        `workspace data: \`${key}\` stands in the mirror from a git entry, and the pin on it is not something this step ` +
          'lifts out of one line, so no copy of that edge reaches the host root',
      );
    }
  }
  const rows = [];
  for (const [key, entry] of merged) {
    if (entry.trees.length > 1) {
      rows.push(
        `# [upstream-sync] ${key} is named by ${entry.trees.join(' and ')}: the entry below states ` +
          `${JSON.stringify(entry.version)}`,
      );
    }
    rows.push(...renderDependency(key, entry));
  }
  return rows;
}

function resolveMirrorGraph() {
  const result = spawnSync('cargo', ['metadata', '--offline', '--format-version', '1'], {
    cwd: HOST_ROOT,
    encoding: 'utf8',
    maxBuffer: 512 * 1024 * 1024,
    env: process.env,
  });
  if (result.status !== 0) {
    const said = `${result.stderr ?? ''}${result.stdout ?? ''}`.trim().split('\n').slice(0, 8).join('\n');
    throw new Error(
      `[sync-absorbed-members] the mirror graph does not resolve, so this step cannot tell whether the two trees ` +
        `share one answer:\n${said}`,
    );
  }
  const meta = JSON.parse(result.stdout);
  const versionOf = new Map();
  const holders = new Map();
  const manifestOf = new Map();
  for (const pkg of meta.packages) {
    const manifest = String(pkg.manifest_path ?? '').replace(/\\/g, '/');
    manifestOf.set(`${pkg.name} ${pkg.version}`, manifest);
    if (pkg.source === null || pkg.source === undefined) continue;
    if (!versionOf.has(pkg.name)) versionOf.set(pkg.name, new Set());
    versionOf.get(pkg.name).add(pkg.version);
  }
  const roots = absorbedProjects().map((project) => ({
    id: project.id,
    path: `${HOST_ROOT.replace(/\\/g, '/')}/${project.root}/`,
  }));
  const treeOf = (path) => roots.find((root) => path === root.path.replace(/\/$/, '') || path.startsWith(root.path))?.id ?? null;
  for (const pkg of meta.packages) {
    for (const dep of pkg.dependencies ?? []) {
      if (!versionOf.has(dep.name)) continue;
      if (!holders.has(dep.name)) holders.set(dep.name, []);
      holders.get(dep.name).push({
        from: `${pkg.name} ${pkg.version}`,
        req: dep.req ?? '',
        path: resolve(String(pkg.manifest_path ?? '').replace(/\\/g, '/'), '..').replace(/\\/g, '/'),
        tree: treeOf(resolve(String(dep.path ?? pkg.manifest_path ?? '').replace(/\\/g, '/'), '..').replace(/\\/g, '/')),
      });
    }
  }
  const pending = new Map();
  for (const pkg of meta.packages) {
    for (const dep of pkg.dependencies ?? []) {
      if (!versionOf.has(dep.name)) continue;
      const path = resolve(String(pkg.manifest_path ?? '').replace(/\\/g, '/'), '..').replace(/\\/g, '/');
      for (const root of roots) {
        const prefix = root.path.replace(/\/$/, '');
        if (path !== prefix && !path.startsWith(`${prefix}/`)) continue;
        pending.set(`${prefix}\u0000${dep.name}`, dep.req ?? '');
      }
    }
  }
  return { versionOf, holders, manifestOf, pending };
}

function sharedRange(key, stated) {
  const admitted = stated.filter((entry) => entry.range !== null);
  const unknown = stated.filter((entry) => entry.range === null);
  if (unknown.length !== 0) {
    throw new Error(
      `[sync-absorbed-members] \`${key}\` is named by ${stated.map((entry) => entry.snapshotName).join(' and ')}, and ` +
        `${unknown.map((entry) => `${entry.snapshotName} states ${JSON.stringify(entry.version)}`).join(', ')}, which this step ` +
        'cannot read as one set of versions',
    );
  }
  let landed = admitted[0].range;
  for (const entry of admitted.slice(1)) {
    landed = overlapOf(landed, entry.range);
    if (landed === null) {
      throw new Error(
        `[sync-absorbed-members] \`${key}\` is named by ` +
          `${stated.map((each) => `${each.snapshotName} as ${JSON.stringify(each.version)}`).join(' and ')}, and no one set of ` +
          'versions is what both name, so no one range can stand for the two',
      );
    }
  }
  return rangeText(landed, stated);
}

function declaredRanges(collected, key) {
  const ranges = [];
  for (const { snapshotName, entries } of collected) {
    for (const entry of entries ?? []) {
      if (entry.key !== key) continue;
      const value = dependencyValue(entry);
      if (value.version === null) continue;
      ranges.push({ snapshotName, version: value.version });
    }
  }
  return ranges;
}

function isDeclaringTree(collected, treeId) {
  const project = absorbedProjects().find((candidate) => candidate.id === treeId);
  if (project === undefined) return false;
  return collected.some(({ snapshotName }) => snapshotName === project.snapshotFile);
}

function rangesOver(collected, key, versions, holders) {
  const ranges = [];
  for (const demand of holders ?? []) {
    ranges.push({ tree: demand.from, range: demand.req });
  }
  for (const entry of declaredRanges(collected, key)) {
    const parsed = caretRange(String(entry.version).replace(/^\^?/, ''));
    if (parsed === null) continue;
    const landed = [...versions].filter((version) => caretAllows(parsed, version));
    if (landed.length !== 1) continue;
    ranges.push({ tree: entry.snapshotName, range: entry.version });
  }
  return ranges;
}

const SINGLE_INSTANCE_CRATES = new Set([
  'tauri',
  'tauri-runtime',
  'tauri-runtime-wry',
  'tauri-utils',
  'tauri-build',
  'wry',
  'webview2-com',
  'webview2-com-sys',
  'windows',
  'windows-core',
  'libsqlite3-sys',
]);

function convergedLands(declaration, versionOf, key) {
  const held = declaration.converged.get(key);
  if (held === undefined) return;
  const versions = versionOf.get(key);
  if (versions === undefined) {
    throw new Error(`[sync-absorbed-members] ${CONVERGENCE} converges \`${key}\`, and the mirror graph carries no such crate to converge`);
  }
  const landed = [...versions].filter((version) => caretAllows(caretRange(held.into), version));
  if (landed.length !== 1) {
    throw new Error(
      `[sync-absorbed-members] ${CONVERGENCE} converges \`${key}\` into ${JSON.stringify(held.into)}, and the mirror graph ` +
        `holds ${[...versions].sort().join(', ')}: ${landed.length === 0 ? 'none of them' : 'more than one of them'} ` +
        'answers to that value, so the value written is not the one resolved',
    );
  }
  const reached = `${caretRange(held.into).major}`;
  const foreign = [...versions].filter((version) => `${caretRange(version).major}` !== reached);
  if (held.holds === 'sole' && foreign.length !== 0) {
    throw new Error(
      `[sync-absorbed-members] ${CONVERGENCE} holds \`${key}\` sole in the graph, and it also resolves to ` +
        `${foreign.sort().join(', ')}: a crate that must stand once stands once`,
    );
  }
}

function assertOneAnswerPerCrate(collected, declaration, graph) {
  const { versionOf, holders } = graph;
  const converged = new Set();
  for (const key of SINGLE_INSTANCE_CRATES) {
    convergedLands(declaration, versionOf, key);
    if (declaration.converged.has(key)) converged.add(key);
  }
  const failures = [];
  for (const key of SINGLE_INSTANCE_CRATES) {
    const versions = versionOf.get(key);
    if (versions === undefined || versions.size < 2) continue;
    const ranges = rangesOver(collected, key, versions, holders.get(key));
    if (ranges.length === 0) continue;
    const admitted = (range) => {
      const parsed = caretRange(String(range).replace(/^\^?/, ''));
      if (parsed === null) return null;
      const landed = [...versions].filter((version) => caretAllows(parsed, version));
      return landed.length === 1 ? landed[0] : null;
    };
    const sides = ranges.map((entry) => ({ ...entry, landsOn: admitted(entry.range) })).filter((entry) => entry.landsOn !== null);
    if (sides.length === 0) continue;
    const held = new Set(sides.map((entry) => entry.landsOn));
    const beyond = [];
    for (const demand of holders.get(key) ?? []) {
      if (!isDeclaringTree(collected, demand.path)) continue;
      const landed = admitted(demand.req);
      if (landed === null) continue;
      if (held.has(landed)) continue;
      if (converged.has(key)) continue;
      beyond.push({ from: demand.from, req: demand.req, landsOn: landed });
    }
    if (beyond.length === 0) continue;
    failures.push({ key, versions: [...versions].sort(), sides, beyond });
  }
  if (failures.length === 0) return [];
  const lines = failures.map((failure) => {
    const sides = failure.sides
      .map((side) => `${side.tree} declares ${JSON.stringify(side.range)}, landing on ${side.landsOn}`)
      .join('; ');
    const who = [...new Set(failure.beyond.map((entry) => `${entry.from} asks ${JSON.stringify(entry.req)}, landing on ${entry.landsOn}`))].join('; ');
    return `  \`${failure.key}\` resolves to ${failure.versions.join(' and ')} in the mirror graph: ${sides}, while ${who}.`;
  });
  throw new Error(
    '[sync-absorbed-members] the mirror graph carries two answers for one crate name, and the two trees that are ' +
      `supposed to be one workspace do not agree on which is theirs:\n${lines.join('\n')}\n` +
      'Both trees' +
      ' back ends compile into one process and a crate like this one must be a single instance, so the graph is ' +
      'resolved once here rather than half-way through a build. Settle which range the mirror carries, then run this ' +
      'step again.',
  );
}

function caretRange(version) {
  const match = String(version).trim().match(/^\^?(\d+)(?:\.(\d+))?(?:\.(\d+))?(?:-([0-9A-Za-z.-]+))?$/);
  if (match === null) return null;
  return {
    major: Number(match[1]),
    minor: match[2] === undefined ? null : Number(match[2]),
    patch: match[3] === undefined ? null : Number(match[3]),
    pre: match[4] ?? null,
  };
}

function caretAllows(range, version) {
  const other = caretRange(version);
  if (other === null) return false;
  if (range.major !== 0 || other.major !== 0) {
    if (range.major !== other.major) return false;
    if (range.minor !== null && other.minor !== null && range.minor > other.minor) return false;
    if (range.minor !== null && other.minor !== null && range.minor === other.minor && range.patch !== null && other.patch !== null && range.patch > other.patch) return false;
    return true;
  }
  if (range.minor === null) return true;
  if (other.minor !== range.minor) return false;
  if (range.patch === null || other.patch === null) return true;
  return range.patch <= other.patch;
}

function inserts(range) {
  return range.minor === null ? null : { minor: range.minor, patch: range.patch ?? 0 };
}

function overlapOf(left, right) {
  if (left.major !== right.major) return null;
  const a = inserts(left);
  const b = inserts(right);
  if (a === null || b === null) return null;
  if (a.minor !== b.minor) return null;
  return { major: left.major, minor: a.minor, patch: Math.max(a.patch, b.patch), pre: null };
}

function statedRange(text) {
  const plain = String(text).trim();
  if (plain.startsWith('=')) {
    const exact = caretRange(plain.slice(1));
    if (exact === null || exact.minor === null || exact.patch === null) return null;
    return { major: exact.major, minor: exact.minor, patch: exact.patch, pre: exact.pre };
  }
  if (plain.startsWith('~')) {
    const tilde = caretRange(plain.slice(1));
    if (tilde === null || tilde.minor === null) return null;
    return { major: tilde.major, minor: tilde.minor, patch: tilde.patch ?? 0, pre: tilde.pre };
  }
  if (plain.startsWith('^')) return caretRange(plain.slice(1));
  return caretRange(plain);
}

function rangeText(range, stated) {
  const head = `${range.major}.${range.minor}`;
  if (stated.every((entry) => entry.range.patch === null)) return head;
  return `${head}.${range.patch}`;
}

function mergeDependencyVersions(key, held, incoming) {
  const left = caretRange(held.version);
  const right = caretRange(incoming.version);
  if (left === null || right === null) {
    if (held.version === incoming.version) {
      return { version: held.version, by: incoming.version === held.version ? 'both' : null };
    }
    throw new Error(
      `workspace data: \`${key}\` is ${JSON.stringify(held.version)} in ${held.from} and ${JSON.stringify(incoming.version)} in ` +
        `${incoming.from}, and neither states a caret range this step can compare, so one shared entry cannot be written`,
    );
  }
  const leftCovers = caretAllows(left, incoming.version);
  const rightCovers = caretAllows(right, held.version);
  if (leftCovers && rightCovers) return { version: held.version, by: 'both' };
  if (leftCovers) return { version: held.version, by: held.from };
  if (rightCovers) return { version: incoming.version, by: incoming.from };
  throw new Error(
    `workspace data: no single range satisfies both trees for \`${key}\`: ${held.from} names ${JSON.stringify(held.version)} ` +
      `and ${incoming.from} names ${JSON.stringify(incoming.version)}, and neither range accepts the other's version. ` +
      `The two trees cannot share one graph through one entry, and ${CONVERGENCE} does not settle it: a version written ` +
      'there would be one tree\'s declaration standing over the other\'s, which is the difference this repository does not ' +
      'paper over. Settle it in the tree that is wrong, or name the crate as one whose instance a path holds.',
  );
}

function buildDependencyLines(collected) {
  const merged = new Map();
  for (const { snapshotName, entries } of collected) {
    for (const entry of entries) {
      const incoming = { ...dependencyValue(entry), from: snapshotName, lines: entry.lines };
      const held = merged.get(entry.key);
      if (held === undefined) {
        merged.set(entry.key, { ...incoming, trees: [snapshotName] });
        continue;
      }
      if (held.kind !== incoming.kind) {
        if (held.version !== incoming.version) {
          throw new Error(
            `workspace data: \`${entry.key}\` is a ${held.kind} entry in ${held.from} and a ${incoming.kind} entry in ` +
              `${incoming.from}, and one key holds one shape unless both trees name one version`,
          );
        }
        merged.set(entry.key, {
          kind: held.kind,
          version: held.version,
          rest: held.rest,
          from: held.from,
          trees: [...new Set([...held.trees, snapshotName])],
          lines: held.lines,
          rewritten: false,
        });
        continue;
      }
      const pick = mergeDependencyVersions(entry.key, held, incoming);
      const rest = held.rest !== incoming.rest && incoming.rest !== '' ? incoming.rest : held.rest;
      merged.set(entry.key, {
        kind: held.kind,
        version: pick.version,
        rest,
        from: pick.by === incoming.from ? incoming.from : held.from,
        trees: [...new Set([...held.trees, snapshotName])],
        lines: held.lines,
        rewritten: pick.version !== held.version || rest !== held.rest,
      });
    }
  }
  if (merged.size === 0) return null;
  return merged;
}

function hostLinesOutsideSegments(text, table) {
  const keys = new Set();
  let inside = false;
  for (const line of tableLines(text, table)) {
    const trimmed = line.trim();
    if (trimmed.startsWith(SENTINEL_PREFIX) && !trimmed.endsWith('end')) {
      inside = true;
      continue;
    }
    if (inside) {
      if (trimmed.startsWith(SENTINEL_PREFIX) && trimmed.endsWith('end')) inside = false;
      continue;
    }
    const match = line.match(/^\s*([A-Za-z0-9_-]+)\s*=/);
    if (match) keys.add(match[1]);
  }
  return keys;
}

function nameOf(text) {
  const lines = splitLines(text).map(contentOf);
  const start = lines.findIndex((line) => line.trim() === '[package]');
  for (let i = start + 1; start !== -1 && i < lines.length; i += 1) {
    if (/^\s*\[/.test(lines[i])) break;
    const quoted = lines[i].match(/^\s*name\s*=\s*"([^"]+)"\s*$/);
    if (quoted !== null) return quoted[1];
  }
  return null;
}

function withoutStripMatches(treeRoot, entry, face) {
  const segments = entry.split('/').filter((part) => part !== '' && part !== '.');
  let at = treeRoot;
  let held = null;
  for (const segment of segments) {
    let entryAt;
    try {
      entryAt = readdirSync(at, { withFileTypes: true }).find((candidate) => candidate.name === segment);
    } catch {
      return null;
    }
    if (entryAt === undefined) return null;
    if (face.within(segment) && !face.spares(entryAt, treeRoot)) return null;
    held = entryAt;
    at = join(at, segment);
  }
  return held;
}

function strippedCardIn(project, entry) {
  const underneath = entry === '.' ? '' : `${entry}/`;
  const held = snapshotSections(project);
  return [...held.keys()].some((where) => where.startsWith(underneath) && where.split('/').pop() === TREE_MANIFEST);
}

function assertEveryEntryNamesADirectory(project, members, excludes) {
  const treeRoot = projectRoot(project);
  const face = stripRules(project);
  const stands = (entry) => {
    const held = withoutStripMatches(treeRoot, entry, face);
    if (held === null) return strippedCardIn(project, entry);
    return held.isDirectory() || held.isSymbolicLink();
  };
  const excluded = new Set(excludes);
  for (const [array, entries] of [['members', members], ['exclude', excludes]]) {
    for (const entry of entries) {
      if (!stands(entry) || (array === 'members' && excluded.has(entry))) {
        throw new Error(
          `[sync-absorbed-members] ${project.id}: the ${array} entry "${entry}" stands nowhere in the tree, or stands in ` +
            'exclude as well: a member list read after the strip points at no missing directory',
        );
      }
    }
  }
}

function assertNoEntryReachesShell(project, members) {
  const fronts = frontendRoots(project).map((root) => (root === '.' ? project.root : `${project.root}/${root}`));
  for (const entry of members) {
    const at = `${project.root}/${entry === '.' ? '' : entry}`.replace(/\/+$/, '');
    const shell = `${at}/src-tauri`;
    for (const front of fronts) {
      if (front !== shell && !front.startsWith(`${shell}/`)) continue;
      throw new Error(
        `[sync-absorbed-members] ${project.id}: the members entry "${entry}" makes the closure reach ${shell}, and the ` +
          `tree's frontend root ${JSON.stringify(front)} stands under it: 前端壳面误入闭包`,
      );
    }
  }
}

function buildExpectedSegments(project, declaration, graph, snapshotMemberTextValue) {
  const snapshotName = project.snapshotFile;
  const projectStale = staleSnapshotEntries(snapshotMemberTextValue, snapshotName, projectRoot(project));
  const memberText = projectStale.length === 0
    ? snapshotMemberTextValue
    : pruneSnapshot(snapshotMemberTextValue, projectStale, snapshotName);
  const commands = mechanism(project) === 'cargo-workspace';
  const depended = (crateRoot) => convergedDependency(
    ownDependencyEntries(
      memberText,
      new Set(hostLinesOutsideSegments(readFileSync(HOST_MANIFEST, 'utf8'), 'workspace.dependencies')),
      snapshotName,
    ),
    convergedShared(declaration, [convergedElements(declaration, graph, project)]),
  );
  const patched = () => {
    const lines = buildPatchLines(memberText, snapshotName, project);
    return lines === null ? null : patchGroupsOf(lines, snapshotName);
  };

  if (commands) {
    if (!snapshotDeclaresWorkspaceText(memberText)) {
      throw new Error(
        `${snapshotName}: ${project.id} is read as a cargo-workspace tree, and its snapshot declares no [workspace], ` +
          'so the members the mirror carries stand written nowhere',
      );
    }
    const members = readStringArray(memberText, 'members', snapshotName);
    const upstreamExcludes = readStringArray(memberText, 'exclude', snapshotName);
    if (members.length === 0) {
      throw new Error(`${snapshotName}: members array is empty`);
    }
    assertEveryEntryNamesADirectory(project, members, upstreamExcludes);
    assertNoEntryReachesShell(project, members);
    const note = generatedNote(snapshotName);
    return {
      members: [...note, ...members.map((entry) => `"${project.root}/${entry}",`)],
      excludes: [...note, ...upstreamExcludes.map((entry) => `"${project.root}/${entry}",`)],
      data: null,
      dependencies: depended(project.root),
      patches: patched() ?? [],
      profiles: profileTables(memberText),
      attributes: buildByteFidelityLines(project),
      upstreamCount: members.length,
    };
  }

  assertMechanismPairing(project);
  const crateDirectory = packageDirectory(project);
  const crateRoot = crateBase(project);
  const crateRole = 'This entry is the backend crate the extension mechanism hands in, never the shell the deshell step takes off: ';
  const crateName = project.sync?.backendPackage;
  const stripped = strippedAbsence(project);
  if (crateName === undefined) {
    throw new Error(
      `[sync-absorbed-members] ${project.id} names no \`sync.backendPackage\`, so the one member the mirror carries for it ` +
        `stands at ${JSON.stringify(crateRoot)}, and that path is this step's own reading rather than anything the tree states. ` +
        crateRole +
        'a member the mirror cannot name is the shell as readily as it is the crate',
    );
  }
  if (stripped === null && entryReachesShell(project)) {
    throw new Error(
      `[sync-absorbed-members] the crate ${project.id} declares \`sync.backendPackage\` as ${JSON.stringify(crateName)} stands at ` +
        `${JSON.stringify(crateDirectory)}, and one of its own crate targets reads the frontend the host chain starts that tree ` +
        `from at ${JSON.stringify(project.root)}, so the member the mirror carries would carry the tree's shell as well. ` +
        crateRole +
        'that tree is the shell the deshell step takes off, and its frontend enters by the MiniApp landing',
    );
  }
  if (stripped !== null && entryReachesShell(project)) {
    throw new Error(
      `[sync-absorbed-members] ${project.id} names \`sync.strippedPackageDirectory\`, so the shell is not in the tree this step ` +
        `reads, and the card at ${JSON.stringify(stripped.segment('Cargo.toml'))} still reaches the frontend at ` +
        `${JSON.stringify(project.root)}. ` +
        crateRole +
        'a card that reads the frontend once the shell has left is a card the deshell face does not cover',
    );
  }
  if (stripped !== null && nameOf(memberText) !== crateName) {
    throw new Error(
      `[sync-absorbed-members] ${project.id} declares \`sync.backendPackage\` as ${JSON.stringify(crateName)}, and the card ` +
        `this step reads it from is the one at ${stripped.segment('Cargo.toml')}, which names itself ${JSON.stringify(nameOf(memberText))}: ` +
        'the deshell face and this member are two different crates',
    );
  }
  if (stripped === null && nameOf(memberText) !== crateName) {
    throw new Error(
      `[sync-absorbed-members] ${project.id} declares \`sync.backendPackage\` as ${JSON.stringify(crateName)} and the crate at ` +
        `${crateDirectory} names itself ${JSON.stringify(nameOf(memberText))}, so the member the mirror carries is not ` +
        'the crate the extension mechanism hands in',
    );
  }
  const note = generatedNote(snapshotName);
  return {
    members: [...note, `"${crateRoot}",`],
    excludes: [...note],
    data: null,
    dependencies: depended(crateRoot),
    patches: patched() ?? [],
    profiles: profileTables(memberText),
    attributes: buildByteFidelityLines(project),
    upstreamCount: 1,
  };
}

function withinRange(range, version) {
  const other = caretRange(version);
  if (range === null || other === null) return false;
  return caretAllows(
    { major: range.major, minor: range.minor, patch: range.patch, pre: null },
    `${other.major}.${other.minor}.${other.patch}`,
  );
}

function lockSections(text) {
  const sections = [];
  const head = [];
  for (const line of splitLines(text)) {
    if (/^\s*\[\[[^\]]+\]\]\s*$/.test(line)) {
      sections.push({ header: line.trim(), lines: [line] });
      continue;
    }
    if (sections.length === 0) head.push(line);
    else sections[sections.length - 1].lines.push(line);
  }
  return { head, sections };
}

function lockFields(section) {
  const fields = {};
  for (const line of section.lines.slice(1)) {
    const match = line.match(/^\s*([A-Za-z0-9_-]+)\s*=\s*"([^"]*)"\s*$/);
    if (match !== null) fields[match[1]] = match[2];
  }
  return fields;
}

function settledVersions(sections, name) {
  const held = new Set();
  for (const section of sections) {
    if (section.header !== '[[package]]') continue;
    const fields = lockFields(section);
    if (fields.name === name && fields.version !== undefined) held.add(fields.version);
  }
  return held;
}

export function releaseLockVersions(text, settledAt) {
  const { head, sections } = lockSections(text);
  const kept = [];
  const dropped = [];
  for (const section of sections) {
    const fields = lockFields(section);
    const convergedAt = fields.name === undefined ? undefined : settledAt(fields.name);
    if (convergedAt !== undefined && fields.version !== convergedAt) {
      dropped.push(`${fields.name} ${fields.version}`);
      continue;
    }
    kept.push(section);
  }
  return { text: [...head, ...kept.flatMap((section) => [...section.lines, ''])].join('\n'), dropped };
}

function assertOneLockCarriesTheMergedResolution(declaration) {
  const { sections } = lockSections(readFileSync(HOST_LOCK, 'utf8'));
  const absent = [];
  for (const [name, held] of declaration.converged) {
    const versions = settledVersions(sections, name);
    if (versions.size === 0) continue;
    const landed = [...versions].filter((version) => caretAllows(statedRange(held.into), version));
    if (landed.length === 0) {
      absent.push(
        `\`${name}\` stands at ${[...versions].sort().join(', ')} in ${HOST_LOCK}, and ${CONVERGENCE} settles it at ` +
          `${JSON.stringify(held.into)}, which none of them answers to: the one lock carries the merged resolution, and a ` +
          'version that leaves it takes the merge with it',
      );
    }
  }
  if (absent.length !== 0) {
    throw new Error(
      `[sync-absorbed-members] ${HOST_LOCK} no longer carries the merged resolution the two trees settle:\n` +
        `${absent.join('\n')}\n` +
        'One lock carries the merged result, and a version that leaves it takes the merge with it.',
    );
  }
}

function assertMirrorGraph(declaration, graph) {
  for (const [name, held] of declaration.converged) {
    const versions = graph.versionOf.get(name);
    if (versions === undefined) {
      throw new Error(
        `[sync-absorbed-members] ${CONVERGENCE} converges \`${name}\`, and the mirror graph carries no such crate to converge`,
      );
    }
    if (![...versions].some((version) => withinRange(caretRange(held.into), version))) {
      throw new Error(
        `[sync-absorbed-members] ${CONVERGENCE} holds \`${name}\` at ${JSON.stringify(held.into)}, and the mirror graph resolves ` +
          `${[...versions].sort().join(', ')}: the range the generated entry names admits none of them`,
      );
    }
  }
  assertOneAnswerPerCrate([], declaration, graph);
  return graph;
}

export function syncAbsorbedMembers({ check = false, quiet = false } = {}) {
  const declared = absorbedProjects();
  const declaration = convergenceDeclaration();

  const stale = [];
  const expected = [];
  const graph = resolveMirrorGraph();
  let upstreamCount = 0;
  for (const project of declared) {
    const sourceName = project.snapshotFile;
    const memberText = snapshotMemberText(project);
    const projectStale = staleSnapshotEntries(memberText, sourceName, projectRoot(project));
    stale.push(...projectStale);
    if (projectStale.length > 0 && check) {
      const named = projectStale.map(({ array, entry }) => `  ${array}: "${entry}"`).join('\n');
      throw new Error(
        `[sync-absorbed-members] STALE SNAPSHOT: ${projectStale.length} entr${projectStale.length === 1 ? 'y names' : 'ies name'} ` +
          `a directory the absorbed tree ${project.id} does not have:\n${named}\n` +
          'The host manifest is regenerated from this snapshot, so it would carry the same dead entry. ' +
          'The snapshot keeps the taken manifests as they were, so only a new take rewrites them.',
      );
    }
    if (projectStale.length > 0 && !quiet) {
      console.log(
        `[sync-absorbed-members] held back ${projectStale.length} stale snapshot entr` +
          `${projectStale.length === 1 ? 'y' : 'ies'} of ${project.id} from the host manifest: ${projectStale.map(({ entry }) => entry).join(', ')}`,
      );
    }
    const segments = buildExpectedSegments(project, declaration, graph, memberText);
    expected.push({ project, segments });
    upstreamCount += segments.upstreamCount;
  }

  const noted = (lines) => (lines ?? []).filter((line) => !line.startsWith('#')).length;
  assertConvergenceCarried(declaration, expected, graph);

  const profiles = buildProfileLines(expected.map(({ project, segments }) => ({
    snapshotName: project.snapshotFile,
    blocks: segments.profiles ?? [],
  })));

  const data = buildDependencyLines(expected.map(({ project, segments }) => ({
    snapshotName: project.snapshotFile,
    entries: segments.dependencies ?? [],
  })));
  const dataRows = data === null
    ? null
    : [...dataNote(expected.map(({ project }) => project.snapshotFile)), ...renderDependencyRows(data)];

  const hostText = readFileSync(HOST_MANIFEST, 'utf8');

  const once = {
    'seam members': seamMemberLines(),
    'absorbed excludes': expected[0].segments.excludes,
    'absorbed workspace data': dataRows,
    'absorbed profiles': profiles,
  };
  const sections = REGIONS.flatMap((region) => (region.holder === 'tree'
    ? expected.map(({ project, segments }) => ({
      region,
      holder: MIRROR_MEMBERS.has(region.key) ? project.root : project.id,
      body: {
        'absorbed members': segments.members,
        'absorbed patches': segments.patches,
        'absorbed byte fidelity': segments.attributes,
      }[region.key],
    }))
    : [{ region, holder: '', body: once[region.key] }]));
  const written = new Map();
  for (const { region, holder, body } of sections) {
    const at = `${region.key}\u0000${holder}`;
    if (written.has(at)) {
      throw new Error(`host manifest: ${region.key} under ${JSON.stringify(holder)} is written twice, and one region writes once`);
    }
    written.set(at, {
      key: region.key,
      file: region.file,
      table: region.table,
      array: region.array,
      padding: region.padding,
      holder,
      marker: markers(holder, region.key),
      body: Array.isArray(body) ? body : null,
    });
  }
  const ordered = sections.map(({ region, holder }) => written.get(`${region.key}\u0000${holder}`));
  const unseen = expected.length - ordered.filter((entry) => entry.key === 'absorbed patches' && entry.body !== null).length;
  if (unseen !== 0) {
    throw new Error(
      `host manifest: ${unseen} of ${expected.length} absorbed tree(s) carries no absorbed patches segment, and an edge ` +
        'no tree speaks for stands in no table the mirror reads',
    );
  }
  const hostAttributesText = readFileSync(HOST_ATTRIBUTES, 'utf8');
  const previous = declared.map((project) => project.id);
  const rewritten = writeRegions(hostText, hostAttributesText, ordered, previous);
  const updatedText = rewritten.manifest;
  const updatedAttributesText = rewritten.attributes;
  const attributesChanged = updatedAttributesText !== hostAttributesText;

  const changed = updatedText !== hostText || attributesChanged;
  const counts = {
    members: expected.reduce((sum, { segments }) => sum + noted(segments.members), 0),
    excludes: expected.reduce((sum, { segments }) => sum + noted(segments.excludes), 0),
    data: noted(dataRows),
    patches: expected.reduce((sum, { segments }) => sum + noted(segments.patches), 0),
    profiles: noted(profiles),
    byteRules: expected.reduce((sum, { segments }) => sum + noted(segments.attributes), 0),
  };
  const summary = `${counts.members} members, ${counts.excludes} excludes, `
    + `${counts.data} workspace-data lines, ${counts.patches} patch lines, `
    + `${counts.profiles} profile lines, ${counts.byteRules} byte-fidelity rules `
    + `(from ${upstreamCount} upstream entries of ${expected.length} cargo tree(s))`;

  const graphInput = expected.map(({ project, segments }) => ({
    snapshotName: project.snapshotFile,
    entries: segments.dependencies ?? [],
  }));

  if (!changed) {
    if (!quiet) console.log(`[sync-absorbed-members] up to date: ${summary}`);
    assertOneAnswerPerCrate(graphInput, declaration, graph);
    assertMirrorGraph(declaration, graph);
    assertOneLockCarriesTheMergedResolution(declaration);
    return { ...counts, upstreamCount, changed: stale.length > 0, stale };
  }

  if (check) {
    throw new Error(
      `[sync-absorbed-members] OUT OF DATE: host manifest does not match the absorbed manifests (${summary}). ` +
        'Run `node scripts/sync-absorbed-members.mjs` to rewrite it.',
    );
  }

  if (updatedText !== hostText) writeFileSync(HOST_MANIFEST, updatedText, 'utf8');
  if (attributesChanged) writeFileSync(HOST_ATTRIBUTES, updatedAttributesText, 'utf8');
  if (!quiet) {
    console.log(`[sync-absorbed-members] rewrote host manifest: ${summary}`);
  }
  assertMirrorGraph(declaration, graph);
  assertOneLockCarriesTheMergedResolution(declaration);
  return { ...counts, upstreamCount, changed: true, stale };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.includes('--help')) {
    const header = readFileSync(fileURLToPath(import.meta.url), 'utf8');
    const block = header.indexOf('/**');
    console.log(block === -1 ? 'sync-absorbed-members: no header block' : header.slice(block, header.indexOf('*/', block) + 2));
    process.exit(0);
  }
  try {
    const report = syncAbsorbedMembers({ check: process.argv.includes('--check') });
    process.exit(report.changed ? 0 : 3);
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}
