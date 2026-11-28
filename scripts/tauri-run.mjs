import { spawn, spawnSync } from 'node:child_process';
import { existsSync, rmSync } from 'node:fs';
import {
  APP_ROOT,
  REPO_ROOT,
  TAURI_CONF,
  appRoot,
  knownTreeArtifacts,
  rootConfigPath,
  tauriCli,
  treeArtifacts,
} from './desktop-config.mjs';
import { wrapperEnvironment } from './install-rustc-wrapper.mjs';
import { targetDir } from './target-dir.mjs';

export const ARTIFACTS_BEFORE = knownTreeArtifacts();

export function treeDidNotGrow() {
  return treeArtifacts(ARTIFACTS_BEFORE);
}

export function tauriEnvironment() {
  return {
    ...wrapperEnvironment(),
    CARGO_TARGET_DIR: targetDir(),
    TAURI_APP_PATH: appRoot(),
    TAURI_FRONTEND_PATH: REPO_ROOT,
  };
}

export function firstCompilerError(lines) {
  const isCode = (line) => /^error\[E\d+\]:/.test(line.trim());
  const isBare = (line) => /^error:/.test(line.trim());
  const codeLine = lines.findIndex(isCode);
  const from = codeLine !== -1 ? codeLine : lines.findIndex(isBare);
  if (from === -1) return null;
  const block = [];
  for (let i = from; i < lines.length && block.length < 8; i += 1) {
    const line = lines[i].trimEnd();
    if (i > from && (isCode(line) || isBare(line))) break;
    block.push(line);
  }
  const cause = lines.find((line) => /^error: could not compile/.test(line.trim()));
  const head = block.join('\n').trim();
  return cause === undefined || head.includes(cause.trim()) ? head : `${head}\n${cause.trim()}`;
}

export function probeCompiler(packageName, carrier, { cwd = REPO_ROOT } = {}) {
  const result = spawnSync('cargo', ['build', '-p', packageName, ...carrier], {
    cwd,
    encoding: 'utf8',
    env: tauriEnvironment(),
    maxBuffer: 1 << 26,
  });
  const lines = `${result.stdout ?? ''}\n${result.stderr ?? ''}`.split(/\r?\n/);
  return firstCompilerError(lines);
}

export function removeRootConfig() {
  rmSync(APP_ROOT, { recursive: true, force: true });
}

export function tauriArgv(command, configPath, carrier) {
  return command === 'dev'
    ? [tauriCli(), 'dev', '--config', configPath, ...carrier, '--', '--jobs', '4']
    : [tauriCli(), 'build', '--config', configPath, ...carrier];
}

export function runTauri(command, configPath, carrier) {
  const collected = [];
  const child = spawn(process.execPath, tauriArgv(command, configPath, carrier), {
    cwd: REPO_ROOT,
    stdio: ['inherit', 'pipe', 'pipe'],
    env: tauriEnvironment(),
  });
  const keep = (chunk) => {
    const text = chunk.toString('utf8');
    process.stderr.write(text);
    for (const line of text.split(/\r?\n/)) collected.push(line);
  };
  child.stdout.on('data', keep);
  child.stderr.on('data', keep);
  let stopping = false;
  const stop = () => {
    if (stopping) return;
    stopping = true;
    if (child.pid) {
      spawnSync('taskkill', ['/pid', String(child.pid), '/T', '/F'], { stdio: 'ignore', shell: true });
    }
    process.exit(130);
  };
  process.on('SIGINT', stop);
  process.on('SIGTERM', stop);
  return { child, collected, path: configPath };
}

export function runTauriToExit(command, configPath, carrier) {
  const { child, collected } = runTauri(command, configPath, carrier);
  return new Promise((resolve) => {
    child.on('exit', (code) => {
      resolve({ code: code ?? 0, error: firstCompilerError(collected), log: collected.join('\n') });
    });
  });
}
