#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { REPO_ROOT, TAURI_CONF, bundleDir, rootConfigPath, shellConfig, tauriCli } from './desktop-config.mjs';
import {
  absorbedProjects,
  buildConfigFiles,
  hostDerivedPath,
  packageManifestPath,
  projectRoot,
} from './absorbed-sources.mjs';
import { ENTRANCES, chainOrder, shapeSteps } from './chain-order.mjs';
import {
  SEAM_RUN_RECORD_DIR,
  beforeOf,
  guaranteesOf,
  guardOf,
  held,
  landedOf,
  leavesOf,
  settleTrees,
  stepRan,
} from './step-guarantees.mjs';
import { buildWrapper, panoramaBinary, wrapperArguments, wrapperBinary, wrapperEnvironment } from './install-rustc-wrapper.mjs';
import { probeCompiler, removeRootConfig, runTauriToExit, treeDidNotGrow } from './tauri-run.mjs';
import { targetDir } from './target-dir.mjs';

const here = dirname(fileURLToPath(import.meta.url));

function fail(step, message) {
  process.stderr.write(`[run-chain] FATAL at step ${step}: ${message}\n`);
  process.exit(1);
}
function run(command, args, step, environment = process.env) {
  const result = spawnSync(command, args, {
    cwd: REPO_ROOT,
    env: environment,
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

function wrapper() {
  buildWrapper();
  const binary = wrapperBinary();
  if (!existsSync(binary) || statSync(binary).size === 0) {
    fail('wrapper', `${binary} is not on disk after the install`);
  }
  const beside = panoramaBinary();
  if (!existsSync(beside)) {
    fail('wrapper', `the install left no ${beside}`);
  }
  process.stdout.write(`[run-chain] wrapper: ${binary} is in place beside ${beside}\n`);
}

function step(names, name) {
  run(process.execPath, [join(here, 'sync-upstream.mjs'), ...names], name, wrapperEnvironment());
}

function absorb() {
  step(['take'], 'absorb');
}

function merge() {
  step(['take', 'merge'], 'merge');
}

function stripStep() {
  step(['strip'], 'strip');
}

function deshellStep() {
  step(['deshell'], 'deshell');
}

function patches() {
  step(['take', 'merge', 'patches'], 'patches');
}

function seam() {
  const panorama = panoramaBinary();
  if (!existsSync(panorama)) {
    fail('seam', `${panorama} is not on disk; the wrapper step has not run in this checkout`);
  }
  const read = (selector) => {
    const listed = spawnSync(panorama, [selector], { cwd: REPO_ROOT, env: process.env, encoding: 'utf8' });
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
  const recordDir = join(targetDir(), SEAM_RUN_RECORD_DIR);
  mkdirSync(recordDir, { recursive: true });
  writeFileSync(join(recordDir, 'refused.json'), 'no covered crate compiled as upstream on this pass\n', 'utf8');
}

const RELEASE_ARTIFACTS = [
  {
    name: 'mobile-web',
    package: 'obf/src/mobile-web',
    producer: ['run', 'build'],
    produces: 'dist',
  },
  {
    name: 'extension-host',
    package: 'obf/src/apps/extension-host',
    producer: ['run', 'build'],
    produces: 'dist/extension-host.js',
  },
  {
    name: 'ppt-live',
    package: 'obf/src/crates/contracts/product-domains/src/miniapp/builtin/assets/ppt-live',
    producer: ['exec', 'build-openbitfun.mjs'],
    produces: 'dist/ui.bundle.js',
  },
];

const ESBUILD_HOOK = `
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
const STORE = '${join(targetDir(), 'pnpm-store', 'virtual').split('\\').join('/')}';
const MODULES = '${join(REPO_ROOT, 'node_modules').split('\\').join('/')}';
const LOCKS = '${join(REPO_ROOT, 'pnpm-lock.yaml').split('\\').join('/')}';
function lockedVersion() {
  if (!existsSync(LOCKS)) return null;
  const line = readFileSync(LOCKS, 'utf8').split(/\\r?\\n/).find((row) => /^\\s{2}esbuild@/.test(row) || /esbuild@\\d/.test(row));
  const found = line === undefined ? null : line.match(/esbuild@(\\d+\\.\\d+\\.\\d+)/);
  return found === null ? null : found[1];
}
export async function resolve(specifier, context, next) {
  if (specifier === 'esbuild') {
    const version = lockedVersion();
    if (version !== null) {
      const entry = MODULES + '/.pnpm/esbuild@' + version + '/node_modules/esbuild/lib/main.js';
      if (existsSync(entry)) return { url: pathToFileURL(entry).href, shortCircuit: true };
    }
    let versions = [];
    try { versions = readdirSync(STORE).filter((d) => d.startsWith('esbuild@')).sort().reverse(); } catch { versions = []; }
    for (const held of versions) {
      const entry = STORE + '/' + held + '/node_modules/esbuild/lib/main.js';
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

const BUILD_BENCH = 'lvpa-build-bench';

function install() {
  const projects = absorbedProjects();
  for (const project of projects) {
    const manifest = packageManifestPath(project);
    if (manifest !== '') {
      const path = hostDerivedPath(project);
      if (!existsSync(path)) {
        fail('install', `${path} is not on disk, so the tree ${project.id} declares has no manifest to read`);
      }
    }
    for (const name of buildConfigFiles(project)) {
      const path = join(projectRoot(project), ...name.split('/'));
      if (existsSync(path)) continue;
      if (name.endsWith('build.rs')) continue;
      fail('install', `${path} is not on disk, and ${project.id} names it as a build input`);
    }
  }
  const outside = join(targetDir(), 'pnpm-store');
  mkdirSync(outside, { recursive: true });
  const environment = { ...process.env, npm_config_store_dir: outside, PNPM_HOME: join(outside, 'home') };
  run(process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm', ['install', '--frozen-lockfile'], 'install', environment);
  const bench = join(targetDir(), BUILD_BENCH);
  rmSync(bench, { recursive: true, force: true });
  const command = process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm';
  for (const artifact of RELEASE_ARTIFACTS) {
    const packageRoot = join(REPO_ROOT, ...artifact.package.split('/'));
    const built = join(bench, ...artifact.package.split('/'));
    const spared = join(packageRoot, ...artifact.produces.split('/'));
    const wasSpared = existsSync(spared) ? digestOfTree(spared) : null;
    copyInputs(packageRoot, built);
    const [kind, target] = artifact.producer;
    const ran = kind === 'run'
      ? spawnSync(command, ['-C', built, 'run', target], {
        cwd: REPO_ROOT,
        stdio: 'inherit',
        shell: command.endsWith('.cmd'),
        env: environment,
      })
      : spawnSync(process.execPath, ['--import', esbuildRegister(), join(built, ...target.split('/'))], {
        cwd: built,
        stdio: 'inherit',
        shell: false,
        env: environment,
      });
    if (ran.status !== 0) {
      fail(
        'install',
        `${artifact.name} produced no ${artifact.produces}, and its producer ${kind === 'run' ? `pnpm run ${target}` : target} ` +
          `exited ${ran.status} over ${built}; nothing is built inside an absorbed tree, so this step reads only the bench`,
      );
    }
    const produced = join(built, ...artifact.produces.split('/'));
    if (!existsSync(produced)) {
      fail(
        'install',
        `${artifact.name} ran its producer over ${built} and ${artifact.produces} stands nowhere there, so the release ` +
          'layer this build reads is not on disk',
      );
    }
    const nowSpared = existsSync(spared) ? digestOfTree(spared) : null;
    if (nowSpared !== wasSpared) {
      fail(
        'install',
        `${artifact.name} ran its producer and ${spared} changed under it: this step writes that layer at ${built} and leaves ` +
          'the tree one alone, and a tree that gains it carries an artifact where a source input stood',
      );
    }
  }
  const left = treeDidNotGrow();
  if (left.length > 0) {
    fail(
      'install',
      `${left.join(', ')} stands inside an absorbed tree and stood nowhere before this pass, and this step writes every artifact at ` +
        `${bench}`,
    );
  }
}

function digestOfTree(path) {
  const read = (at) => {
    if (!statSync(at).isDirectory()) return `${at}:${statSync(at).size}`;
    return readdirSync(at).sort().map((name) => read(join(at, name))).join('|');
  };
  return read(path);
}

const INPUT_SKIP = new Set(['node_modules', 'dist', 'build', 'target', '.turbo', 'coverage', '.git']);

function copyInputs(from, into) {
  if (!existsSync(from)) {
    fail('install', `${from} is not on disk, and it is the package whose build this step reads`);
  }
  rmSync(into, { recursive: true, force: true });
  const walk = (at, to) => {
    mkdirSync(to, { recursive: true });
    for (const entry of readdirSync(at, { withFileTypes: true })) {
      if (INPUT_SKIP.has(entry.name) || entry.name.startsWith('.vite')) continue;
      const source = join(at, entry.name);
      const destination = join(to, entry.name);
      if (entry.isDirectory()) {
        walk(source, destination);
        continue;
      }
      if (entry.isFile()) cpSync(source, destination);
    }
  };
  walk(from, into);
}

function relativeToRepo(dir) {
  return dir.slice(REPO_ROOT.length + 1).split('\\').join('/');
}

function frontend() {
  run(process.execPath, [join(here, 'sync-openalgo-frontend.mjs')], 'frontend');
}

function shell() {
  const cli = tauriCli();
  if (!existsSync(TAURI_CONF)) {
    fail('dev', `${TAURI_CONF} is not on disk, so the take has landed no shell face and there is none to start`);
  }
  return cli;
}
function guaranteeOf(step) {
  try {
    leavesOf(step);
  } catch (error) {
    fail(step, error.message);
  }
}

function before(step, leaves) {
  try {
    return beforeOf(step, leaves);
  } catch (error) {
    fail(step, error.message);
  }
}

function guard(step) {
  guaranteeOf(step);
  let said;
  try {
    said = guardOf(step);
  } catch (error) {
    fail(step, error.message);
  }
  if (said !== null) fail(step, `${step} refuses to run: ${said}`);
}

function landed(step, was) {
  guaranteeOf(step);
  try {
    return landedOf(step, was);
  } catch (error) {
    fail(step, error.message);
  }
}

function spoken(step, ahead) {
  const now = held();
  if (now === null) {
    fail(step, `${step} ran and Cargo.toml, pnpm-workspace.yaml and the landed copy carry no line a step of this chain states, so this pass cannot tell its having run from its not having run`);
  }
  for (const [head, body] of ahead) {
    if (now.get(head) !== body) return head;
  }
  fail(
    step,
    `${step} ran and the lines it states read as the ones that stood before it (${[...ahead.keys()].join('; ')}), so this pass ` +
      'cannot tell its having run from its not having run',
  );
}

function settle(command) {
  const seamRecordDir = join(targetDir(), SEAM_RUN_RECORD_DIR);
  if (existsSync(seamRecordDir) && readdirSync(seamRecordDir).length > 0) {
    fail(command, `${seamRecordDir} still holds a record, so a wrapper pass left its injection standing`);
  }
}

function bundleStands(step) {
  const dir = bundleDir();
  const held = existsSync(dir) ? readdirSync(dir) : [];
  if (held.length === 0) {
    fail(step, `${dir} holds no installer, so this pass left no product on disk`);
  }
}

async function launch(command) {
  const cli = shell();
  if (!existsSync(cli)) {
    fail(command, `${cli} is not on disk, so this chain has no tauri to start`);
  }
  const binary = wrapperBinary();
  if (!existsSync(binary)) {
    fail(command, `${binary} is not on disk, so this pass would compile with the system compiler`);
  }
  const { path, config } = shellConfig();
  if (path !== rootConfigPath()) {
    fail(command, `the shell face was written to ${path} and this pass hands tauri ${rootConfigPath()}`);
  }
  const carrier = wrapperArguments();
  let code;
  let error;
  let log;
  try {
    ({ code, error, log } = await LAUNCHERS[command](path, carrier));
  } catch (caught) {
    removeRootConfig();
    throw caught;
  }
  if (code !== 0) {
    removeRootConfig();
    const named = /could not compile `([^`]+)`/.exec(log)?.[1];
    const detail = error ?? (named === undefined ? null : probeCompiler(named, carrier));
    fail(
      command,
      `tauri ${command} exited ${code} over ${config.productName} at ${path}` +
        (detail === null
          ? ', and neither its output nor a rebuild of the crate it named carried an error[...] line'
          : `:\n${detail}`),
    );
  }
  removeRootConfig();
  settle(command);
  if (command === 'build') bundleStands(command);
}

function dev() {
  return launch('dev');
}

function build() {
  return launch('build');
}

const LAUNCHERS = {
  dev: (path, carrier) => runTauriToExit('dev', path, carrier),
  build: (path, carrier) => runTauriToExit('build', path, carrier),
};

function retireSeamRecords() {
  const dir = join(targetDir(), SEAM_RUN_RECORD_DIR);
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

const RECIPES = [wrapper, absorb, absorb, merge, stripStep, deshellStep, seam, patches, install, frontend, dev, build];

let entranceChecked = false;
function entrance() {
  if (entranceChecked) return;
  entranceChecked = true;
  const steps = shapeSteps();
  const named = ENTRANCES.flatMap((name) => chainOrder(name)).filter(
    (step, index, all) => all.indexOf(step) === index,
  );
  if (RECIPES.length !== steps.length || named.join('\n') !== steps.join('\n')) {
    fail(
      'entrance',
      `the runner carries ${RECIPES.length} recipe(s) over ${named.join(', ')}, and crates/wiring/shape names ${steps.length} ` +
        `step(s) (${steps.join(', ')}): a recipe would answer to the wrong step name`,
    );
  }
}

function recipe(step) {
  const at = shapeSteps().indexOf(step);
  return at === -1 ? undefined : RECIPES[at];
}

const STATED = new Set(['take', 'absorb', 'merge', 'frontend']);

async function runStep(name) {
  entrance();
  guard(name);
  const leaves = guaranteesOf(name);
  const was = before(name, leaves);
  const ahead = STATED.has(name) ? held() : null;
  await recipe(name)();
  for (const project of absorbedProjects()) settleTrees(project.id);
  stepRan(name, `${name}:${Date.now()}`);
  const changed = landed(name, was);
  const spokenHere = ahead === null ? null : spoken(name, ahead);
  process.stdout.write(
    `[run-chain] ${name}: the pass left ${changed.join(', ')}` + (spokenHere === null ? '\n' : `; it states its lines at ${spokenHere}\n`),
  );
}

export async function runChain(entrance) {
  const ordered = chainOrder(entrance);
  for (const name of ordered) {
    await runStep(name);
  }
  return ordered;
}

const STEPS = shapeSteps();

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const requested = process.argv.slice(2);
  if (requested.length === 1 && requested[0] === 'order') {
    process.stdout.write(`${chainOrder('dev').join('\n')}\n`);
  } else if (requested.length === 2 && requested[0] === 'order') {
    process.stdout.write(`${chainOrder(requested[1]).join('\n')}\n`);
  } else if (requested.length === 1) {
    if (!recipe(requested[0])) {
      process.stderr.write(
        `[run-chain] ${requested[0]} is not a step in the chain (${STEPS.join(', ')}); name one, or name order with an entrance\n`,
      );
      process.exit(2);
    }
    await runStep(requested[0]);
  } else if (requested.length === 2 && requested[0] === 'step') {
    if (!recipe(requested[1])) {
      process.stderr.write(
        `[run-chain] ${requested[1]} is not a step in the chain (${STEPS.join(', ')})\n`,
      );
      process.exit(2);
    }
    await runStep(requested[1]);
  } else if (requested.length === 2 && requested[0] === 'land') {
    if (!recipe(requested[1])) {
      process.stderr.write(
        `[run-chain] ${requested[1]} is not a step in the chain (${STEPS.join(', ')})\n`,
      );
      process.exit(2);
    }
    const changed = landed(requested[1], before(requested[1], guaranteesOf(requested[1])));
    process.stdout.write(`${requested[1]} left ${changed.join(', ')}\n`);
  } else {
    process.stderr.write('[run-chain] name one step, or `order dev` / `order build` to list one entrance\n');
    process.exit(2);
  }
}
