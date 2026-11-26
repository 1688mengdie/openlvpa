import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { targetPath } from './target-dir.mjs';

export const REPO_ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
export const SHELL_CRATE = join(REPO_ROOT, 'obf', 'src', 'apps', 'desktop');
export const TAURI_CONF = join(SHELL_CRATE, 'tauri.conf.json');
export const WORK_DIR = join(REPO_ROOT, '.openlvpa', 'run');
export const APP_ROOT = join(WORK_DIR, 'app');
export const BUNDLE_PATH = 'release/bundle';

const TREE_GENERATED = ['gen', 'target', '.tauri', 'dist'];

function inside(root, path) {
  const normalized = resolve(path);
  return normalized === resolve(root) || normalized.startsWith(`${resolve(root)}${sep}`);
}

export function knownTreeArtifacts() {
  const found = new Set();
  const walk = (dir) => {
    let entries;
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      if (!entry.isDirectory()) continue;
      const nested = join(dir, entry.name);
      if (entry.name === 'node_modules' || entry.name === '.git') continue;
      if (TREE_GENERATED.includes(entry.name)) {
        found.add(nested.split('\\').join('/'));
        continue;
      }
      walk(nested);
    }
  };
  walk(join(REPO_ROOT, 'obf'));
  walk(join(REPO_ROOT, 'openalgo-desktop'));
  return found;
}

export function treeArtifacts(before) {
  const now = knownTreeArtifacts();
  if (before === undefined) return [...now].sort();
  return [...now].filter((path) => !before.has(path)).sort();
}

export function assertOutsideTrees(path, what) {
  for (const tree of [join(REPO_ROOT, 'obf'), join(REPO_ROOT, 'openalgo-desktop')]) {
    if (inside(tree, path)) {
      throw new Error(
        `[desktop-config] ${what} is ${path}, which stands inside ${tree}: an absorbed tree is no build root, so nothing tauri ` +
          'writes lands there and this pass stops before it writes one',
      );
    }
  }
  return path;
}

export function bundleDir() {
  return targetPath(...BUNDLE_PATH.split('/'));
}

export const PRODUCT_NAME = 'OpenLVPA';
export const IDENTIFIER = 'com.openlvpa.desktop';

const CLI = '@tauri-apps/cli';

function lockImporters() {
  const lock = readFileSync(join(REPO_ROOT, 'pnpm-lock.yaml'), 'utf8').split(/\r?\n/);
  const opened = lock.findIndex((row) => /^importers:\s*$/.test(row));
  if (opened === -1) {
    throw new Error('[desktop-config] pnpm-lock.yaml carries no importers table, so no package resolves a dependency here');
  }
  const closed = lock.findIndex((row, at) => at > opened && /^[A-Za-z]/.test(row));
  return lock.slice(opened + 1, closed === -1 ? lock.length : closed);
}

function resolvedTauriVersion() {
  const body = lockImporters();
  const at = body.indexOf('  .:');
  if (at === -1) {
    throw new Error('[desktop-config] pnpm-lock.yaml resolves no package for the repository root, so nothing names a tauri to start');
  }
  let next = body.length;
  for (let index = at + 1; index < body.length; index += 1) {
    if (/^\S/.test(body[index])) {
      next = index;
      break;
    }
  }
  const key = `      '${CLI}':`;
  const entry = body.slice(at + 1, next).indexOf(key);
  if (entry === -1) {
    throw new Error(
      `[desktop-config] pnpm-lock.yaml resolves no ${CLI} for the repository root, so this chain has no tauri to start; ` +
        `declare ${CLI} at the repository root`,
    );
  }
  const resolved = body
    .slice(at + 1 + entry, next)
    .map((row) => /^\s+version:\s*'?([^\s']+)'?\s*$/.exec(row))
    .find((found) => found !== null);
  if (resolved === undefined) {
    throw new Error(`[desktop-config] pnpm-lock.yaml declares ${CLI} at the repository root and resolves no version for it`);
  }
  return resolved[1];
}

function launcher(directory) {
  const manifest = join(directory, 'package.json');
  return existsSync(manifest)
    ? JSON.parse(readFileSync(manifest, 'utf8')).bin?.tauri ?? null
    : null;
}

export function tauriCli() {
  const version = resolvedTauriVersion();
  const held = [
    join(REPO_ROOT, 'node_modules', ...CLI.split('/')),
    join(REPO_ROOT, 'node_modules', '.pnpm', `${CLI.replace('/', '+')}@${version}`, 'node_modules', ...CLI.split('/')),
  ];
  for (const directory of held) {
    const named = launcher(directory);
    if (named !== null) return join(directory, named);
  }
  throw new Error(
    `[desktop-config] pnpm-lock.yaml resolves ${CLI}@${version}, and this checkout holds no package.json for it under ` +
      `${held.join(' or ')}; the install step has not run against this checkout`,
  );
}

export function shellPackage() {
  const manifest = readFileSync(join(SHELL_CRATE, 'Cargo.toml'), 'utf8');
  const name = manifest.match(/^name\s*=\s*"([^"]+)"/m)?.[1];
  if (name === undefined) {
    throw new Error(`[desktop-config] ${join(SHELL_CRATE, 'Cargo.toml')} declares no package name`);
  }
  const literal = manifest.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  const inherited = /^version\.workspace\s*=\s*true/m.test(manifest);
  const version = literal ?? (inherited
    ? readFileSync(join(REPO_ROOT, 'Cargo.toml'), 'utf8')
      .match(/\[workspace\.package\]([\s\S]*?)(?=\n\[|$)/)?.[1]
      ?.match(/^version\s*=\s*"([^"]+)"/m)?.[1]
    : undefined);
  if (version === undefined) {
    throw new Error(`[desktop-config] ${name} declares a version this chain cannot resolve`);
  }
  return { name, version };
}

function absoluteResources(resources, from) {
  const named = {};
  for (const [source, target] of Object.entries(resources)) {
    named[resolve(from, source)] = target;
  }
  return named;
}

export const ROOT_CONFIG_NAME = 'tauri.conf.json';

export function rootConfigPath() {
  return join(APP_ROOT, ROOT_CONFIG_NAME);
}

const SKIPPED_IN_PLACE = new Set(['target', 'node_modules', '.git', 'gen', '.tauri']);

function copyTree(from, into) {
  mkdirSync(into, { recursive: true });
  for (const entry of readdirSync(from, { withFileTypes: true })) {
    if (SKIPPED_IN_PLACE.has(entry.name)) continue;
    const source = join(from, entry.name);
    const destination = join(into, entry.name);
    if (entry.isDirectory()) copyTree(source, destination);
    else if (entry.isFile()) writeFileSync(destination, readFileSync(source));
  }
}

export function appRoot() {
  assertOutsideTrees(APP_ROOT, 'the tauri application root');
  if (!existsSync(APP_ROOT)) copyTree(SHELL_CRATE, APP_ROOT);
  return APP_ROOT;
}

export function rebuildAppRoot() {
  assertOutsideTrees(APP_ROOT, 'the tauri application root');
  rmTree(APP_ROOT);
  copyTree(SHELL_CRATE, APP_ROOT);
  return APP_ROOT;
}

function rmTree(path) {
  if (!existsSync(path)) return;
  for (const entry of readdirSync(path, { withFileTypes: true })) {
    const nested = join(path, entry.name);
    if (entry.isDirectory()) rmTree(nested);
    else rmSync(nested, { force: true });
  }
  rmSync(path, { recursive: true, force: true });
}

const WEB_UI = ['obf', 'src', 'web-ui'];

export function webUiBench() {
  const bench = join(targetDir(), 'lvpa-web-ui');
  const from = join(REPO_ROOT, ...WEB_UI);
  if (!existsSync(from)) {
    throw new Error(`[desktop-config] ${from} stands nowhere, so the shell face has no frontend to build`);
  }
  rmTree(bench);
  copyTree(from, bench);
  return bench;
}

export function shellConfig() {
  const stripped = JSON.parse(readFileSync(TAURI_CONF, 'utf8'));
  const pkg = shellPackage();
  const root = rebuildAppRoot();
  const assets = resolve(root, stripped.build.frontendDist);
  const webUi = relative(REPO_ROOT, webUiBench()).split('\\').join('/');
  const config = {
    ...stripped,
    productName: PRODUCT_NAME,
    identifier: IDENTIFIER,
    version: pkg.version,
    mainBinaryName: pkg.name,
    build: {
      ...stripped.build,
      beforeDevCommand: `pnpm --dir ${webUi} run dev`,
      beforeBuildCommand: `pnpm --dir ${webUi} run build:desktop`,
      frontendDist: relative(root, assets).split('\\').join('/'),
    },
    bundle: {
      ...stripped.bundle,
      publisher: '1688mengdie',
      outDir: relative(assets, bundleDir()).split('\\').join('/'),
      resources: absoluteResources(stripped.bundle.resources, SHELL_CRATE),
    },
  };
  const path = rootConfigPath();
  writeFileSync(path, `${JSON.stringify(config, null, 2)}\n`);
  return { path, config };
}
