#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { REPO_ROOT, SHELL_CRATE, tauriCli } from './desktop-config.mjs';
import { absorbedProjects, buildConfigFiles, packageDirectoryPath, packageManifestPath, projectRoot } from './absorbed-sources.mjs';
import { chainOrder, chainGuarantees } from './chain-order.mjs';
import { probeCompiler, runTauriToExit } from './tauri-run.mjs';
import { targetDir } from './target-dir.mjs';

const here = dirname(fileURLToPath(import.meta.url));

function fail(step, message) {
  process.stderr.write(`[run-chain] FATAL at step ${step}: ${message}\n`);
  process.exit(1);
}
function run(command, args, step) {
  const result = spawnSync(command, args, {
    cwd: REPO_ROOT,
    env: process.env,
    stdio: 'inherit',
    shell: command.endsWith('.cmd'),
  });
  if (result.error) {
    fail(step, `${command} could not start: ${result.error.message}`);
  }
  if (result.status !== 0) {
    fail(step, `${command} ${args.join(' ')} exited ${result.status}`);
  }
}

function quiet(command, args) {
  return spawnSync(command, args, { cwd: REPO_ROOT, env: process.env, encoding: 'utf8' });
}

function wrapper() {
  run(process.execPath, [join(here, 'install-rustc-wrapper.mjs')], 'wrapper');
  const binary = wrapperBinary();
  if (!existsSync(binary) || statSync(binary).size === 0) {
    fail('wrapper', `${binary} is not on disk after the install`);
  }
  const beside = join(dirname(binary), 'lvpa-capability-panorama.exe');
  if (!existsSync(beside)) {
    fail('wrapper', `the install left no ${beside}`);
  }
  process.stdout.write(`[run-chain] wrapper: ${binary} is in place beside ${beside}\n`);
}

function wrapperBinary() {
  const config = join(REPO_ROOT, '.cargo', 'config.toml');
  if (!existsSync(config)) {
    fail('wrapper', `${config} does not exist, so no rustc-wrapper is declared`);
  }
  const line = readFileSync(config, 'utf8')
    .split(/\r?\n/)
    .find((row) => /^\s*rustc-wrapper\s*=/.test(row));
  if (!line) {
    fail('wrapper', `${config} declares no [build] rustc-wrapper`);
  }
  const declared = line.match(/=\s*"([^"]+)"/);
  if (!declared) {
    fail('wrapper', `${config} declares rustc-wrapper without a quoted path`);
  }
  return join(REPO_ROOT, ...declared[1].split('/'));
}

function take() {
  run(process.execPath, [join(here, 'sync-upstream.mjs')], 'take');
}

function absorb() {
  run(process.execPath, [join(here, 'sync-absorbed-members.mjs')], 'absorb');
}

function merge() {
  run(process.execPath, [join(here, 'sync-absorbed-frontend.mjs')], 'merge');
}

function cargoManifests(root) {
  const found = [];
  const walk = (dir) => {
    let entries;
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      if (entry.name === 'node_modules' || entry.name === 'target' || entry.name === '.git') continue;
      const path = join(dir, entry.name);
      if (entry.isDirectory()) walk(path);
      else if (entry.name === 'Cargo.toml') found.push(path);
    }
  };
  walk(root);
  return found;
}

function strip() {
  const roots = JSON.parse(readFileSync(join(here, 'absorbed-sources.json'), 'utf8')).projects.map(
    (project) => project.root,
  );
  const declared = [];
  for (const root of roots) {
    for (const path of cargoManifests(join(REPO_ROOT, ...root.split('/')))) {
      const text = readFileSync(path, 'utf8');
      if (/^\s*\[workspace[.\]]/m.test(text)) declared.push(path);
    }
  }
  if (declared.length > 0) {
    fail('strip', `a workspace table still stands inside the absorbed trees: ${declared.join(', ')}`);
  }
  for (const root of roots) {
    for (const name of ['Cargo.lock', 'pnpm-workspace.yaml', 'pnpm-lock.yaml', 'package-lock.json', 'yarn.lock']) {
      const path = join(REPO_ROOT, ...root.split('/'), name);
      if (existsSync(path)) {
        fail('strip', `${path} is a second member list or a second lock beside the one at the repository root`);
      }
    }
  }
  process.stdout.write(`[run-chain] strip: nothing inside ${roots.join(', ')} declares a workspace or holds a lock\n`);
}

function seam() {
  const panorama = join(dirname(wrapperBinary()), 'lvpa-capability-panorama.exe');
  if (!existsSync(panorama)) {
    fail('seam', `${panorama} is not on disk; the wrapper step has not run in this checkout`);
  }
  const read = (selector) => {
    const listed = quiet(panorama, [selector]);
    if (listed.status !== 0) {
      fail('seam', `${panorama} ${selector} exited ${listed.status}: ${listed.stderr}`);
    }
    return listed.stdout.split(/\r?\n/).filter((row) => row.trim().length > 0);
  };
  const covered = read('--covered-crates');
  if (covered.length === 0) {
    fail('seam', 'the rule table declares no covered crate, so nothing would carry a seam');
  }
  const rows = read('--seams');
  if (rows.length === 0) {
    fail('seam', 'the rule table declares no seam, so the covered crate would compile as upstream');
  }
  for (const row of rows) {
    const [module, host, landed] = row.split('\t');
    if (landed !== 'present') {
      fail('seam', `seam ${module} names ${host}, which is not a readable file in this checkout`);
    }
  }
  process.stdout.write(`[run-chain] seam: ${rows.length} seam(s) over ${covered.length} covered crate(s)\n`);
}

function patches() {
  const declarations = readdirSync(join(REPO_ROOT, 'patches')).filter((name) => name.endsWith('.patch.json'));
  if (declarations.length === 0) {
    fail('patches', 'no patch declaration is on disk, so the take would carry the absorbed trees as upstream wrote them');
  }
  let landed = 0;
  let already = 0;
  let replayed = 0;
  for (const name of declarations) {
    const declaration = JSON.parse(readFileSync(join(REPO_ROOT, 'patches', name), 'utf8'));
    if (declaration.landed !== undefined) {
      replayed += 1;
      continue;
    }
    landed += 1;
    const target = join(REPO_ROOT, ...declaration.file.split('/'));
    if (!existsSync(target)) {
      fail('patches', `${name} targets ${declaration.file}, which the taken tree does not carry`);
    }
    const replacements = declaration.replacements ?? [];
    if (replacements.length === 0) {
      fail('patches', `${name} names no replacement, so it changes no byte`);
    }
    const text = readFileSync(target, 'utf8');
    for (const replacement of replacements) {
      if (replacement.find === '') {
        fail('patches', `${name} spells a file whole in its replacement, and the take writes no file from nothing`);
      }
      const found = text.split(replacement.find).length - 1;
      if (found === 1) continue;
      const removed = replacement.replace === '';
      if (removed && declaration.retains === undefined) {
        fail(
          'patches',
          `${name} removes its anchor and names no \`retains\` text, so a spent anchor and a removed one look the same ` +
            `in ${declaration.file} and nothing here could tell them apart`,
        );
      }
      const retained = declaration.retains === undefined
        ? null
        : text.split(declaration.retains).length - 1;
      const done = removed ? retained === 1 : text.split(replacement.replace).length - 1 === 1;
      if (found === 0 && done) {
        already += 1;
        continue;
      }
      fail(
        'patches',
        `${name} finds its anchor ${found} time(s) in ${declaration.file} and its replaced text ${done ? (removed ? 'stands removed' : 'once') : 'not at all'}; ` +
          (retained === 0
            ? `the file no longer carries the text this patch leaves standing, so the anchor is spent and the declaration names nothing`
            : 'the anchor is spent, so the declaration no longer names where in that file it acts'),
      );
    }
  }
  process.stdout.write(
    `[run-chain] patches: ${landed} declaration(s) over the taken trees, ${already} of them already in place, ` +
      `${replayed} replayed over the landed MiniApp copy\n`,
  );
}

const RELEASE_ARTIFACTS = [
  {
    name: 'mobile-web',
    producer: ['run', 'prepare:mobile-web'],
    produces: 'obf/src/mobile-web/dist',
  },
  {
    name: 'extension-host',
    producer: ['run', 'plugin-host:build'],
    install: ['run', 'plugin-host:install'],
    produces: 'obf/src/apps/extension-host/dist/extension-host.js',
  },
  {
    name: 'ppt-live',
    producer: ['exec', 'src/crates/contracts/product-domains/src/miniapp/builtin/assets/ppt-live/build-openbitfun.mjs'],
    produces: 'obf/src/crates/contracts/product-domains/src/miniapp/builtin/assets/ppt-live/dist/ui.bundle.js',
  },
];

const ESBUILD_HOOK = `
import { existsSync, readdirSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
const STORE = '${join(REPO_ROOT, 'obf', 'node_modules', '.pnpm').split('\\').join('/')}';
export async function resolve(specifier, context, next) {
  if (specifier === 'esbuild') {
    let versions = [];
    try { versions = readdirSync(STORE).filter((d) => d.startsWith('esbuild@')).sort().reverse(); } catch { versions = []; }
    for (const version of versions) {
      const entry = STORE + '/' + version + '/node_modules/esbuild/lib/main.js';
      if (existsSync(entry)) return { url: pathToFileURL(entry).href, shortCircuit: true };
    }
  }
  return next(specifier, context);
}
`;

function esbuildRegister() {
  const dir = targetDir();
  const file = join(dir, 'lvpa-esbuild-hook.mjs');
  mkdirSync(dir, { recursive: true });
  writeFileSync(file, ESBUILD_HOOK);
  return `data:text/javascript,import { register } from 'node:module'; register(${JSON.stringify(pathToFileURL(file).href)});`;
}

const SHELL_FACE_FILES = ['src-tauri/tauri.conf.json'];

function install() {
  const projects = absorbedProjects();
  for (const project of projects) {
    const manifest = packageManifestPath(project);
    if (manifest !== '') {
      const path = join(projectRoot(project), ...manifest.split('/'));
      if (!existsSync(path)) {
        fail('install', `${path} is not on disk, so the tree ${project.id} declares has no manifest to read`);
      }
    }
    for (const name of buildConfigFiles(project)) {
      if (SHELL_FACE_FILES.includes(name)) continue;
      const path = join(projectRoot(project), ...name.split('/'));
      if (!existsSync(path)) {
        fail('install', `${path} is not on disk, and ${project.id} names it as a build input`);
      }
    }
  }
  run(process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm', ['install', '--frozen-lockfile'], 'install');
  const obfRoot = join(REPO_ROOT, 'obf');
  const needsReleaseLayer = RELEASE_ARTIFACTS.some((artifact) => !existsSync(join(REPO_ROOT, ...artifact.produces.split('/'))));
  if (needsReleaseLayer && !existsSync(join(obfRoot, 'node_modules'))) {
    const pnpm = process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm';
    const installed = spawnSync(pnpm, ['install', '--ignore-workspace', '--lockfile=false'], {
      cwd: obfRoot,
      stdio: 'inherit',
      shell: pnpm.endsWith('.cmd'),
    });
    if (installed.status !== 0) {
      fail(
        'install',
        `pnpm install --ignore-workspace exited ${installed.status} under ${obfRoot}, and the release layer's producers ` +
          'read that package set, so the three artifacts this build embeds cannot be made',
      );
    }
  }
  for (const artifact of RELEASE_ARTIFACTS) {
    const produced = join(REPO_ROOT, ...artifact.produces.split('/'));
    if (existsSync(produced)) continue;
    const [kind, target] = artifact.producer;
    const command = process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm';
    if (artifact.install !== undefined) {
      const [, installTarget] = artifact.install;
      const prepared = spawnSync(command, [installTarget], { cwd: obfRoot, stdio: 'inherit', shell: command.endsWith('.cmd') });
      if (prepared.status !== 0) {
        fail(
          'install',
          `${artifact.name} needs its own dependency set, and pnpm ${installTarget} exited ${prepared.status} under ${obfRoot}`,
        );
      }
    }
    const result = kind === 'run'
      ? spawnSync(command, [target], { cwd: obfRoot, stdio: 'inherit', shell: command.endsWith('.cmd') })
      : spawnSync(
        process.execPath,
        ['--import', esbuildRegister(), join(obfRoot, ...target.split('/'))],
        { cwd: obfRoot, stdio: 'inherit', shell: false },
      );
    if (result.status !== 0) {
      fail(
        'install',
        `${artifact.name} produced no ${artifact.produces}, and its producer ${kind === 'run' ? `pnpm ${target}` : target} ` +
          `exited ${result.status} under ${obfRoot}; the release layer this build reads is not on disk`,
      );
    }
    if (!existsSync(produced)) {
      fail('install', `${artifact.name} ran its producer and ${artifact.produces} still stands nowhere`);
    }
  }
}

const SHELL_DIRECTORIES = ['capabilities', 'icons', 'gen'];

function deshellCrate(directory) {
  const dropped = [];
  const conf = join(directory, 'tauri.conf.json');
  if (existsSync(conf)) {
    rmSync(conf, { force: true });
    dropped.push('tauri.conf.json');
  }
  for (const name of SHELL_DIRECTORIES) {
    const path = join(directory, name);
    if (!existsSync(path)) continue;
    rmSync(path, { recursive: true, force: true });
    dropped.push(`${name}/`);
  }
  return dropped;
}

function deshell() {
  const projects = absorbedProjects();
  for (const project of projects) {
    const directory = packageDirectoryPath(project);
    const dropped = deshellCrate(directory);
    if (dropped.length > 0) {
      process.stdout.write(
        `[run-chain] deshell: ${project.id} carries no Tauri shell in the mirror workspace; dropped ${dropped.join(', ')}\n`,
      );
    }
    const build = join(directory, 'build.rs');
    if (existsSync(build) && /tauri_build/.test(readFileSync(build, 'utf8'))) {
      fail(
        'deshell',
        `${build} still names tauri_build; the shell's build block is declared in patches/ and this step does not write ` +
          'that file, so the patches step has to run against it first',
      );
    }
  }
  process.stdout.write(`[run-chain] deshell: no member directory carries tauri.conf.json, capabilities/, icons/, gen/, or a tauri_build call\n`);
}

function frontend() {
  run(process.execPath, [join(here, 'sync-openalgo-frontend.mjs')], 'frontend');
}

function shell() {
  const cli = tauriCli();
  const conf = join(SHELL_CRATE, 'tauri.conf.json');
  if (!existsSync(conf)) {
    fail('dev', `${conf} is not on disk, so there is no shell to start`);
  }
  return { cli, conf, project: SHELL_CRATE };
}

function guard(step, guarantees) {
  const held = guarantees.get(step);
  if (held === undefined) {
    fail(step, `the shape declares no guarantee for ${step}, so this runner cannot say what ${step} refuses`);
  }
  for (const required of held.onDisk) {
    const path = join(REPO_ROOT, ...required.split('/'));
    if (!existsSync(path)) {
      fail(step, `${step} refuses to run until ${required} stands on disk, and ${path} is not there`);
    }
  }
}

function dev() {
  return launch('dev');
}

function build() {
  return launch('build');
}

const LAUNCHERS = {
  dev: () => runTauriToExit('dev'),
  build: () => runTauriToExit('build'),
};

function retireSeamRecords() {
  const dir = join(targetDir(), 'lvpa-seam-run');
  if (!existsSync(dir)) return [];
  const retired = readdirSync(dir).filter((name) => name.endsWith('.json') || name.endsWith('.lock'));
  for (const name of retired) {
    rmSync(join(dir, name), { force: true });
  }
  if (readdirSync(dir).length === 0) {
    rmSync(dir, { recursive: true, force: true });
  }
  return retired;
}

let retirementSwept = false;
function sweepOnce() {
  if (retirementSwept) return;
  retirementSwept = true;
  const retired = retireSeamRecords();
  if (retired.length > 0) {
    process.stdout.write(`[run-chain] retired ${retired.length} seam record(s) under ${targetDir()}\n`);
  }
}
process.on('exit', sweepOnce);

async function launch(command) {
  const { cli, conf, project } = shell();
  if (!existsSync(cli)) {
    fail(command, `${cli} is not on disk, so this chain has no tauri to start`);
  }
  if (!existsSync(conf)) {
    fail(command, `${conf} is not on disk, so there is no shell to start`);
  }
  process.stdout.write(`[run-chain] ${command}: ${cli} runs ${command} over ${project}, out of ${REPO_ROOT}\n`);
  const { code, error, log } = await LAUNCHERS[command]();
  if (code !== 0) {
    const named = /could not compile `([^`]+)`/.exec(log)?.[1];
    const detail = error ?? (named === undefined ? null : probeCompiler(named));
    fail(
      command,
      `tauri ${command} exited ${code} over ${project}` +
        (detail === null
          ? ', and neither its output nor a rebuild of the crate it named carried an error[...] line'
          : `:\n${detail}`),
    );
  }
}

const STEPS = { wrapper, take, absorb, merge, strip, deshell, seam, patches, install, frontend, dev, build };

export async function runChain(entrance) {
  const ordered = chainOrder(entrance);
  const guarantees = chainGuarantees(entrance);
  for (const name of ordered) {
    const step = STEPS[name];
    if (!step) {
      fail(name, `${name} is not a step this runner carries`);
    }
    guard(name, guarantees);
    await step();
  }
  return ordered;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const requested = process.argv.slice(2);
  if (requested.length === 1 && requested[0] === 'order') {
    process.stdout.write(`${chainOrder('dev').join('\n')}\n`);
  } else if (requested.length === 2 && requested[0] === 'order') {
    process.stdout.write(`${chainOrder(requested[1]).join('\n')}\n`);
  } else if (requested.length === 1) {
    const step = STEPS[requested[0]];
    if (!step) {
      process.stderr.write(
        `[run-chain] ${requested[0]} is not a step in the chain (${Object.keys(STEPS).join(', ')}); name one, or name order with an entrance\n`,
      );
      process.exit(2);
    }
    guard(requested[0], chainGuarantees('dev'));
    await step();
  } else {
    process.stderr.write('[run-chain] name one step, or `order dev` / `order build` to list one entrance\n');
    process.exit(2);
  }
}
