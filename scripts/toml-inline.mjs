/**
 * One entry's value read as the keys it holds and written back in one shape. A tree writes
 * a dependency table wherever it likes, across as many lines as it likes; the mirror writes
 * each value the tree states over one line each, so what a tree wrapped reaches the host
 * root whole.
 */

import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));

const DEPENDENCIES = join(SCRIPT_DIR, 'extended-dependencies.json');

/// The edges a tree states in a card of its own are the tree's own words, and an edge this
/// repository lands at another series is landed by the one mechanism there is: a table of
/// edges keyed by the crate that states them, read here and by the wrapper, which rewrites
/// the cards on disk before cargo reads the graph and puts them back after. The file is
/// written as JSON because it is a list of objects rather than one value the trees state:
/// the reader below takes it as such, and a note key is this file's own reading, so an
/// empty table is a pass that lands no edge.
let extended = null;

export function extendedDependencies() {
  if (extended !== null) return extended;
  const text = readFileSync(DEPENDENCIES, 'utf8').replace(/\r\n/g, '\n').replace(/\/\*[\s\S]*?\*\//gu, '');
  const read = JSON.parse(text);
  const edges = [];
  for (const [crateName, list] of Object.entries(read)) {
    if (crateName.startsWith('_')) continue;
    if (!Array.isArray(list)) {
      throw new Error(`[toml-inline] ${DEPENDENCIES}: \`${crateName}\` is neither a note nor one crate's list of written edges`);
    }
    for (const edge of list) {
      if (typeof edge !== 'object' || edge === null || typeof edge.package !== 'string' || typeof edge.version !== 'string') {
        throw new Error(
          `[toml-inline] ${DEPENDENCIES}: \`${crateName}\` carries an edge that names no version and no package, and a written edge names both`,
        );
      }
      edges.push({
        crate: typeof edge.crate === 'string' ? edge.crate : crateName,
        package: edge.package,
        version: edge.version,
        features: Array.isArray(edge.features) ? edge.features : [],
        scope: typeof edge.scope === 'string' ? edge.scope : null,
      });
    }
  }
  extended = edges;
  return edges;
}

/// A value split at the commas that stand outside every string, brace and bracket, with the
/// quote marks of each part lifted off.
function parts(text) {
  const out = [];
  let word = '';
  let quoted = false;
  let square = 0;
  let curly = 0;
  for (const mark of text) {
    if (mark === '"') {
      quoted = !quoted;
      word += mark;
      continue;
    }
    if (quoted) {
      word += mark;
      continue;
    }
    if (mark === '[') square += 1;
    if (mark === ']') square -= 1;
    if (mark === '{') curly += 1;
    if (mark === '}') curly -= 1;
    if (mark === ',' && square === 0 && curly === 0) {
      out.push(word.trim());
      word = '';
      continue;
    }
    word += mark;
  }
  const held = word.trim();
  if (held !== '') out.push(held);
  return out.filter((one) => one !== '');
}

function unquote(text) {
  return text.replace(/^"|"$/g, '').trim();
}

/// A string whose own quotes are whatever the tree wrote: a value arrives with its quotes
/// still on it, or with a second pair added over them, and both read as the one string.
function stringOf(text) {
  let held = text.trim();
  let guard = 0;
  while (held.startsWith('"') && held.endsWith('"') && held.length >= 2 && guard < 8) {
    held = held.slice(1, -1);
    guard += 1;
  }
  return held.split('""').join('"').trim();
}

/// One value read as the keys it holds: `fields` is every key in the order the tree wrote
/// them, and a key whose value is a list reads as the list.
export function readValue(text) {
  const flat = String(text).trim().replace(/\s+#[^\n"]*$/, '');
  if (/^"[^"]*"$/.test(flat)) return { kind: 'bare', version: stringOf(flat), fields: [] };
  if (!flat.startsWith('{')) return { kind: 'opaque', version: null, fields: [], said: flat };
  const body = flat.slice(1).replace(/\}\s*(?:#[^\n"]*)?$/, '');
  const fields = [];
  for (const part of parts(body)) {
    const at = part.indexOf('=');
    if (at === -1) {
      fields.push({ key: '', value: stringOf(part), list: null });
      continue;
    }
    const key = part.slice(0, at).trim();
    const value = part.slice(at + 1).trim();
    if (value.startsWith('[')) {
      const list = parts(value.slice(1).replace(/\]$/, '')).map(stringOf).filter((name) => name !== '');
      fields.push({ key, value: '', list });
      continue;
    }
    fields.push({ key, value: stringOf(value), list: null });
  }
  fields.comment = String(flat).trim().match(/\}\s*(#[^\n"]*)$/)?.[1] ?? '';
  const version = fields.find((field) => field.key === 'version');
  return { kind: 'table', version: version === undefined ? null : version.value, fields };
}

/// One value written back with the quotes this file gives every string: `version` first,
/// the rest in the order the tree wrote them, and a list kept on one line while it fits.
export function writeValue(key, read) {
  if (read.kind === 'bare') return [`${key} = "${read.version}"`];
  if (read.kind === 'opaque') return [`${key} = ${read.said}`];
  const written = [];
  for (const field of read.fields) {
    if (field.key === 'version') continue;
    if (field.list !== null) {
      written.push(`${field.key} = [${field.list.map((name) => `"${name}"`).join(', ')}]`);
      continue;
    }
    written.push(/^(?:true|false|-?\d+(?:\.\d+)?)$/.test(field.value) ? `${field.key} = ${field.value}` : `${field.key} = "${field.value}"`);
  }
  const version = read.version === undefined ? [] : [`version = "${read.version}"`];
  const note = read.fields.comment ?? '';
  const one = `${key} = { ${[...version, ...written].join(', ')} }${note === '' ? '' : ` ${note}`}`;
  if (one.length <= 100) return [one];
  const lists = written.filter((field) => field.includes(' = ['));
  const scalar = written.filter((field) => !field.includes(' = ['));
  if (lists.length !== 1 || !lists[0].startsWith('features = [')) return [one];
  const names = [...lists[0].matchAll(/"([^"]*)"/g)].map((found) => found[1]);
  const head = `${key} = { ${[...version, ...scalar].join(', ')}${version.length + scalar.length === 0 ? '' : ', '}features = [`;
  // a features list no name stands in is one empty list: a key split over three lines states that
  // key again at the one `]`, and a value read back under the key has to read as one line
  if (names.length === 0) return [`${head}] }`];
  return [`${head}`, ...names.map((name) => `    "${name}",`), '] }'];
}
