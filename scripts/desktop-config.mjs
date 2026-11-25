import { existsSync, mkdirSync, readFileSync, readdirSync, unlinkSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const REPO_ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
export const SHELL_CRATE = join(REPO_ROOT, 'obf', 'src', 'apps', 'desktop');
export const TAURI_CONF = join(SHELL_CRATE, 'tauri.conf.json');
export const WORK_DIR = join(REPO_ROOT, '.openlvpa');

export const PRODUCT_NAME = 'OpenLVPA';
export const IDENTIFIER = 'com.openlvpa.desktop';

const STORE = join(REPO_ROOT, 'node_modules', '.pnpm');
const CLI = '@tauri-apps+cli';

export function tauriCli() {
  const cli = readdirSync(STORE)
    .filter((entry) => entry.startsWith(`${CLI}@`))
    .sort()
    .pop();
  if (cli === undefined) {
    throw new Error(
      `[desktop-config] ${STORE} holds no @tauri-apps/cli, so this chain has no tauri to start; declare @tauri-apps/cli at the repository root`,
    );
  }
  const place = join(STORE, cli, 'node_modules', '@tauri-apps', 'cli');
  const manifest = JSON.parse(readFileSync(join(place, 'package.json'), 'utf8'));
  const entry = join(place, manifest.bin.tauri);
  if (!existsSync(entry)) {
    throw new Error(`[desktop-config] ${cli} names its launcher ${entry}, which is not on disk`);
  }
  return entry;
}

export function shellConfig() {
  return JSON.parse(readFileSync(TAURI_CONF, 'utf8'));
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

export function rootConfigPath() {
  return join(WORK_DIR, 'tauri.root.conf.json');
}

export function writeRootConfig(command) {
  const path = rootConfigPath();
  if (existsSync(path)) unlinkSync(path);
  const shell = shellConfig();
  const pkg = shellPackage();
  const config = {
    ...shell,
    productName: PRODUCT_NAME,
    identifier: IDENTIFIER,
    version: pkg.version,
    mainBinaryName: pkg.name,
    build: {
      ...shell.build,
      beforeDevCommand: 'pnpm --dir obf/src/web-ui run dev',
      beforeBuildCommand: 'pnpm --dir obf/src/web-ui run build:desktop',
      frontendDist: resolve(SHELL_CRATE, shell.build.frontendDist),
    },
    bundle: {
      ...shell.bundle,
      publisher: '1688mengdie',
      resources: absoluteResources(shell.bundle.resources, SHELL_CRATE),
    },
  };
  mkdirSync(WORK_DIR, { recursive: true });
  writeFileSync(path, `${JSON.stringify(config, null, 2)}\n`);
  return { path, command };
}

export function removeRootConfig() {
  const path = rootConfigPath();
  if (!existsSync(path)) return;
  let held;
  try {
    held = JSON.parse(readFileSync(path, 'utf8'));
  } catch {
    unlinkSync(path);
    return;
  }
  if (held.productName !== PRODUCT_NAME) return;
  unlinkSync(path);
}
