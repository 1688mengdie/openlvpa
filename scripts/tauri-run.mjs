import { spawn, spawnSync } from 'node:child_process';
import { REPO_ROOT, SHELL_CRATE, removeRootConfig, tauriCli, writeRootConfig } from './desktop-config.mjs';
import { targetDir } from './target-dir.mjs';

export function tauriEnvironment() {
  const environment = { ...process.env, CARGO_TARGET_DIR: targetDir(), TAURI_APP_PATH: SHELL_CRATE };
  delete environment.CARGO_BUILD_RUSTC_WRAPPER;
  delete environment.CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER;
  return environment;
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

export function probeCompiler(packageName, { cwd = REPO_ROOT } = {}) {
  const result = spawnSync('cargo', ['build', '-p', packageName], {
    cwd,
    encoding: 'utf8',
    env: tauriEnvironment(),
    maxBuffer: 1 << 26,
  });
  const lines = `${result.stdout ?? ''}\n${result.stderr ?? ''}`.split(/\r?\n/);
  return firstCompilerError(lines);
}

export function runTauri(command) {
  const path = writeRootConfig(command).path;
  const args = command === 'dev'
    ? [tauriCli(), 'dev', '--config', path, '--', '--jobs', '4']
    : [tauriCli(), 'build', '--config', path];
  const collected = [];
  const child = spawn(process.execPath, args, {
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
    removeRootConfig();
    process.exit(130);
  };
  process.on('SIGINT', stop);
  process.on('SIGTERM', stop);
  process.on('exit', removeRootConfig);
  return { child, collected, path };
}

export function runTauriToExit(command) {
  const { child, collected } = runTauri(command);
  return new Promise((resolve) => {
    child.on('exit', (code) => {
      removeRootConfig();
      resolve({ code: code ?? 0, error: firstCompilerError(collected), log: collected.join('\n') });
    });
  });
}
