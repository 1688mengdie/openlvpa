#!/usr/bin/env node
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { REPO_ROOT } from './desktop-config.mjs';

const STEP_SOURCE = join(REPO_ROOT, 'crates', 'wiring', 'shape', 'src', 'step.rs');
const VALUE = '([A-Z][A-Za-z0-9]*)';

const ARM_VALUES = 'Step::[A-Z][A-Za-z0-9]*|Entrance::[A-Z][A-Za-z0-9]*';

const DEFINED = (head) => new RegExp(`^\\s*${head.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}`, 'm');

export const ENTRANCES = ['dev', 'build'];

function unreadable(said) {
  process.stderr.write(`[chain-order] ${STEP_SOURCE}: ${said}\n`);
  process.exit(1);
}

function openingAt(text, head) {
  const found = DEFINED(head).exec(text);
  if (found === null) unreadable(`carries no \`${head}\`, and that is the declaration this chain reads`);
  const opens = found.index + found[0].length - 1;
  if (text[opens] !== '{' && text[opens] !== '[') unreadable(`states \`${head}\` and opens no block after it`);
  return opens;
}

function closeOf(text, opens) {
  let depth = 0;
  for (let at = opens; at < text.length; at += 1) {
    if (text[at] === '{' || text[at] === '[' || text[at] === '(') depth += 1;
    if (text[at] === '}' || text[at] === ']' || text[at] === ')') {
      depth -= 1;
      if (depth === 0) return at;
    }
  }
  return -1;
}

function blockAt(text, head) {
  const opens = typeof head === 'number' ? head : openingAt(text, head);
  const closes = closeOf(text, opens);
  if (closes === -1) unreadable(`opens \`${head}\` and never closes it`);
  return text.slice(opens + 1, closes);
}

function bodiesOf(text, head) {
  const opens = typeof head === 'number' ? head : openingAt(text, head);
  const closes = closeOf(text, opens);
  if (closes === -1) unreadable(`opens \`${head}\` and never closes it`);
  const body = text.slice(opens + 1, closes);
  const heads = [...body.matchAll(new RegExp(`(?:^|\\s)(${ARM_VALUES}) =>`, 'g'))];
  const held = new Map();
  heads.forEach((arm, order) => {
    let at = arm.index + arm[0].length;
    while (at < body.length && /\s/.test(body[at])) at += 1;
    const from = at;
    let depth = 0;
    for (; at < body.length; at += 1) {
      const character = body[at];
      if (character === '{' || character === '[' || character === '(') depth += 1;
      if (character === '}' || character === ']' || character === ')') {
        depth -= 1;
        if (depth === 0) break;
      }
      if (character === ',' && depth === 0) break;
    }
    held.set(arm[1], body.slice(from, at));
  });
  return held;
}

function quoted(body) {
  return /^\s*"([^"]*)"/.exec(body)?.[1];
}

function guaranteeFor(text, step) {
  const body = bodiesOf(blockAt(text, 'pub fn guarantee(self) -> Guarantee {'), 'match self {').get(`Step::${step}`);
  if (body === undefined) unreadable(`names no guarantee for Step::${step}`);
  const held = new Map();
  for (const name of ['on_disk', 'leaves']) {
    const at = body.indexOf(`${name}: &[`);
    if (at === -1) unreadable(`names the guarantee for Step::${step} without \`${name}\``);
    const opens = at + `${name}: &[`.length - 1;
    const closes = closeOf(body, opens);
    if (closes === -1) unreadable(`opens \`${name}\` for Step::${step} and never closes it`);
    held.set(name, [...body.slice(opens + 1, closes).matchAll(/"([^"]*)"/g)].map((found) => found[1]));
  }
  return { onDisk: held.get('on_disk'), leaves: held.get('leaves') };
}

let shape = null;

function shapeOf() {
  if (shape !== null) return shape;
  const text = readFileSync(STEP_SOURCE, 'utf8');
  const order = [...blockAt(text, 'pub static ORDER: &[Step] = &[').matchAll(new RegExp(`Step::${VALUE}`, 'g'))].map(
    (found) => found[1],
  );
  if (order.length === 0) unreadable('names no step in `pub static ORDER`, and that is where the chain reads its order');
  const twice = order.filter((name, at) => order.indexOf(name) !== at);
  if (twice.length !== 0) unreadable(`names ${twice.join(', ')} twice in \`pub static ORDER\`, and one step stands in the chain once`);
  const terminals = new Map();
  for (const [arm, body] of bodiesOf(blockAt(text, 'pub fn name(self) -> &\'static str {'), 'match self {')) {
    const said = quoted(body);
    if (said === undefined) unreadable(`names \`${arm}\` in \`pub fn name\` and says nothing it is called`);
    terminals.set(arm, said);
  }
  const shaped = blockAt(text, 'impl Entrance {');
  const spoken = bodiesOf(blockAt(shaped, 'pub fn name(self) -> &\'static str {'), 'match self {');
  const ended = bodiesOf(blockAt(shaped, 'pub fn terminus(self) -> Step {'), 'match self {');
  const entrances = new Map();
  for (const [arm, body] of spoken) {
    const said = quoted(body);
    if (said === undefined) unreadable(`names \`${arm}\` in \`pub fn name\` and says nothing it is called`);
    const terminus = ended.get(arm);
    if (terminus === undefined) unreadable(`names \`${arm}\` and no terminus step, so nothing says where that entrance ends`);
    const step = new RegExp(`Step::${VALUE}`).exec(terminus)?.[1];
    if (step === undefined) unreadable(`names \`${arm}\` as ending at nothing this chain carries`);
    entrances.set(said, step);
  }
  const table = new Map();
  for (const step of order) table.set(step, guaranteeFor(text, step));
  shape = { order, terminals, entrances, table };
  return shape;
}

function named(step) {
  const said = shapeOf().terminals.get(`Step::${step}`);
  if (said === undefined) unreadable(`names ${step} in \`pub static ORDER\` and gives it no name in \`pub fn name\``);
  return said;
}

export function stepNamed(name) {
  const found = shapeOf().order.find((step) => named(step) === name);
  if (found === undefined) unreadable(`is asked for the step \`${name}\`, which \`pub static ORDER\` does not carry`);
  return found;
}

export function entranceOf(step) {
  const asked = `Step::${stepNamed(step)}`;
  for (const [entrance, terminus] of shapeOf().entrances) {
    if (`Step::${terminus}` === asked) return entrance;
  }
  unreadable(`names \`${step}\` a step of this chain and no entrance in \`impl Entrance\` ends there`);
}

export function chainOrder(entrance) {
  const held = shapeOf();
  const terminus = held.entrances.get(entrance);
  if (terminus === undefined) {
    process.stderr.write(`[chain-order] \`${entrance}\` is not an entrance: ${ENTRANCES.join(', ')}\n`);
    process.exit(2);
  }
  const at = held.order.indexOf(terminus);
  if (at === -1) unreadable(`names ${entrance} as ending at ${terminus}, and \`pub static ORDER\` does not carry it`);
  return held.order.slice(0, at + 1).map(named);
}

export function shapeSteps() {
  return shapeOf().order.map(named);
}

export function chainGuaranteesOf() {
  return new Map(shapeOf().order.map((step) => [named(step), shapeOf().table.get(step)]));
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const asked = process.argv.slice(2);
  if (asked.length === 1) {
    for (const name of chainOrder(asked[0])) process.stdout.write(`${name}\n`);
  } else {
    process.stderr.write('[chain-order] name one entrance: dev, build\n');
    process.exit(2);
  }
}
