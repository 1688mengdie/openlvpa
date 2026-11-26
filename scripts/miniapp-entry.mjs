#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { REPO_ROOT, absorbedProjects, buildConfigFiles, frontendMembership, frontendRoots, projectRoot, strippedAbsence } from './absorbed-sources.mjs';
import { targetPath } from './target-dir.mjs';

const DECLARATION = join(REPO_ROOT, 'scripts', 'miniapp-payload.json');
const PATCH_DIR = join(REPO_ROOT, 'patches');
export const PATCH_SUFFIX = '.patch.json';
export const LANDINGS = Object.freeze(['frontend', 'member', 'create']);
const TREE_RUNTIME = 'obf/src/apps/miniapp-shell';
const I18N_MODULE = 'src/i18n/index.ts';

const BUILT_APP_DIR = 'app';

const WORKSPACE_ESCAPES = ['--ignore-workspace', '--lockfile=false', '--no-lockfile'];

export function fail(message) {
  process.stderr.write(`[sync-openalgo-frontend] ${message}\n`);
  process.exit(1);
}

function pnpm() {
  return process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm';
}

function pnpmIn(dir, args, why) {
  const escaped = args.find((arg) => WORKSPACE_ESCAPES.includes(arg));
  if (escaped !== undefined) {
    fail(`${why} would pass ${escaped} in ${dir}, and there is one member list, one dependency set and one lock at the repository root`);
  }
  const ran = spawnSync(pnpm(), args, {
    cwd: dir,
    stdio: 'inherit',
    shell: pnpm().endsWith('.cmd'),
    env: { ...process.env, NODE_ENV: 'production' },
  });
  if (ran.status !== 0) {
    fail(`${why} exited ${ran.status} under ${dir}`);
  }
}

export function payloadDeclaration() {
  if (!existsSync(DECLARATION)) {
    fail(`the payload declaration is missing: ${DECLARATION}`);
  }
  const payload = JSON.parse(readFileSync(DECLARATION, 'utf8'));
  for (const key of ['appId', 'source', 'root', 'language']) {
    if (typeof payload[key] !== 'string' || payload[key] === '') {
      fail(`${DECLARATION} names no \`${key}\`, so the landed MiniApp would carry an incomplete identity card`);
    }
  }
  if (payload.language !== 'zh') {
    fail(`${DECLARATION} names \`language\` as ${JSON.stringify(payload.language)}, and a word table replayed over the landed copy states one language`);
  }
  const extra = Object.keys(payload).filter((key) => !['_comment', 'appId', 'source', 'root', 'language'].includes(key));
  if (extra.length > 0) {
    fail(`${DECLARATION} carries ${extra.join(', ')}, and a patch states itself in patches/ rather than a second time here`);
  }
  return payload;
}

export function payloadProject(payload) {
  const project = absorbedProjects().find((entry) => entry.id === payload.source);
  if (project === undefined) {
    fail(`${DECLARATION} names \`source\` as ${JSON.stringify(payload.source)}, and no absorbed tree is declared under that id`);
  }
  if (!frontendRoots(project).includes(payload.root)) {
    fail(
      `${project.id} names no frontend root ${JSON.stringify(payload.root)} among ${frontendRoots(project).join(', ')}, ` +
        'so the payload would be landed from a place that tree does not carry',
    );
  }
  if (frontendMembership(project, payload.root) !== 'mini-app-payload') {
    fail(
      `${project.id} declares ${JSON.stringify(payload.root)} as ${frontendMembership(project, payload.root)}, and ${DECLARATION} ` +
        'lands that root as the MiniApp: one root has one destination',
    );
  }
  return project;
}

export function payloadRootPath(project, payload) {
  const root = projectRoot(project);
  return payload.root === '.' ? root : join(root, ...payload.root.split('/'));
}

export function patchFiles() {
  if (!existsSync(PATCH_DIR)) return [];
  const names = readdirSync(PATCH_DIR).sort();
  const stray = names.filter((name) => name.endsWith('.json') && !name.endsWith(PATCH_SUFFIX));
  if (stray.length > 0) {
    fail(
      `patches/ carries ${stray.join(', ')} as .json and not as ${PATCH_SUFFIX}: a patch is written one way in one ` +
        'directory, so a name this chain does not read is a patch that stands where nothing applies it',
    );
  }
  return names.filter((name) => name.endsWith(PATCH_SUFFIX)).map((name) => join(PATCH_DIR, name));
}

function landingHead(path, patch) {
  if (typeof patch.landed !== 'string' || patch.landed === '') {
    fail(`${path} carries no \`landed\`, so nothing says which landing applies it`);
  }
  const head = patch.landed.split(':')[0].trim();
  if (!LANDINGS.includes(head)) {
    fail(
      `${path} says it lands as ${JSON.stringify(patch.landed)}, and a patch lands as ${LANDINGS.join(' or ')}: ` +
        'one patch has one place it is applied at',
    );
  }
  return head;
}

function readPatch(path) {
  let patch;
  try {
    patch = JSON.parse(readFileSync(path, 'utf8'));
  } catch (error) {
    fail(`${path} does not read as JSON (${error.message}), so nothing says which copy it changes`);
  }
  if (typeof patch.file !== 'string' || patch.file === '') {
    fail(`${path} names no \`file\`, so nothing says which copy it changes`);
  }
  return { path, patch, landing: landingHead(path, patch) };
}

export function declaredPatches(project) {
  const patches = [];
  for (const path of patchFiles()) {
    const held = readPatch(path);
    if (held.patch.file.split('/')[0] !== project.root) continue;
    if (held.landing !== 'frontend') continue;
    patches.push({ name: path.split(/[\\/]/).pop(), path, patch: held.patch });
  }
  return patches;
}

export function readPatchDeclarations() {
  return patchFiles().map((path) => readPatch(path));
}

export function resolveLanding(payload) {
  if (process.env.OPENBITFUN_MINIAPP_ROOT) {
    return join(process.env.OPENBITFUN_MINIAPP_ROOT, payload.appId);
  }
  const userRoot = (() => {
    if (process.env.OPENBITFUN_USER_ROOT) return process.env.OPENBITFUN_USER_ROOT;
    if (process.platform === 'win32') {
      if (!process.env.APPDATA) fail('APPDATA is not set, so the host names no user data root to land the payload in');
      return join(process.env.APPDATA, 'openbitfun');
    }
    const home = process.env.HOME ?? process.env.USERPROFILE ?? '';
    if (home === '') fail('HOME is not set, so the host names no user data root to land the payload in');
    return process.platform === 'darwin'
      ? join(home, 'Library', 'Application Support', 'openbitfun')
      : join(home, '.config', 'openbitfun');
  })();
  return join(userRoot, 'data', 'miniapps', payload.appId);
}

export function landingFacts(payload, project) {
  const root = payloadRootPath(project, payload);
  const files = {};
  for (const name of ['index.html', 'openscript-runner.html']) {
    const path = join(root, name);
    if (!existsSync(path)) {
      fail(`${project.id}: the taken tree carries no ${path}, so the landed copy would carry no ${name} for the host to open`);
    }
    files[name] = path;
  }
  return { root, files };
}

function projectSources(project, root, target) {
  const source = join(root, 'src');
  if (!existsSync(source)) {
    fail(`${project.id}: the taken tree carries no ${source}, so the landed copy would carry no source for the patches to land on`);
  }
  const face = shellFace(project);
  const projected = [];
  const walk = (dir, prefix) => {
    for (const entry of readdirSync(dir).sort()) {
      if (prefix === 'src' && entry === face) continue;
      const full = join(dir, entry);
      const stat = statSync(full);
      const relative = `${prefix}/${entry}`;
      if (stat.isDirectory()) {
        walk(full, relative);
        continue;
      }
      const destination = join(target, ...relative.split('/'));
      mkdirSync(join(destination, '..'), { recursive: true });
      writeFileSync(destination, readFileSync(full));
      projected.push(relative);
    }
  };
  walk(source, 'src');
  return projected;
}

function treeEntry(root, name, target) {
  const path = join(root, ...name.split('/'));
  if (!existsSync(path)) return false;
  const destination = join(target, ...name.split('/'));
  mkdirSync(join(destination, '..'), { recursive: true });
  writeFileSync(destination, readFileSync(path));
  return true;
}

function rootEntry(payload) {
  return [
    '<!doctype html>',
    `<html lang="${payload.language}">`,
    '  <head>',
    '    <meta charset="UTF-8" />',
    '    <meta name="viewport" content="width=device-width, initial-scale=1.0" />',
    `    <title>${payload.appId}</title>`,
    '    <link rel="stylesheet" href="./source/style.css" />',
    '  </head>',
    '  <body>',
    '    <div id="root"></div>',
    '    <script type="module">',
    "      import { mount } from './source/ui.js';",
    "      mount(document.getElementById('root')).catch((error) => {",
    "        document.getElementById('root').textContent = String(error && error.message ? error.message : error);",
    '      });',
    '    </script>',
    '  </body>',
    '</html>',
    '',
  ].join('\n');
}

function identityCard(payload) {
  return JSON.stringify(
    {
      id: payload.appId,
      name: payload.appId,
      description: `${payload.appId}, landed from the absorbed tree as a MiniApp: no Tauri shell, no window.`,
      icon: 'ChartCandlestick',
      category: 'finance',
      tags: [payload.appId],
      version: 1,
      created_at: 0,
      updated_at: 0,
      permissions: {
        fs: { read: ['{appdata}'], write: ['{appdata}'] },
        shell: { allow: [] },
        net: { allow: [] },
        node: { enabled: false, max_memory_mb: 128, timeout_ms: 5000 },
      },
      ai_context: null,
    },
    null,
    2,
  );
}

function bootModule(payload) {
  return [
    `const BUNDLE = ${JSON.stringify(`./${BUILT_APP_DIR}/index.html`)};`,
    '',
    "export async function mount(element) {",
    "  element.setAttribute('data-openalgo-mounted', 'pending');",
    '  const frame = document.createElement(\'iframe\');',
    '  frame.src = new URL(BUNDLE, import.meta.url).href;',
    '  frame.style.border = \'0\';',
    '  frame.style.width = \'100%\';',
    '  frame.style.height = \'100%\';',
    '  frame.setAttribute(\'title\', ' + JSON.stringify(payload.appId) + ');',
    '  element.appendChild(frame);',
    "  frame.addEventListener('load', () => {",
    "    element.setAttribute('data-openalgo-mounted', 'ready');",
    '  });',
    '}',
    '',
  ].join('\n');
}

const FRONTEND_INPUTS = [
  'package.json',
  'vite.config.ts',
  'tsconfig.json',
  'tsconfig.app.json',
  'tsconfig.node.json',
  'index.html',
  'openscript-runner.html',
  'biome.json',
  'components.json',
  'src',
  'public',
];

const INPUT_SKIP = new Set(['node_modules', 'dist', 'build', 'target', '.turbo', 'coverage', '.git', '.vite']);

function shellFace(project) {
  const absence = strippedAbsence(project);
  return absence === null ? null : absence.segment('');
}

function copyFrontendInputs(project, from, into, skipped) {
  const face = shellFace(project);
  for (const name of FRONTEND_INPUTS) {
    const source = join(from, name);
    if (!existsSync(source)) continue;
    const target = join(into, name);
    if (statSync(source).isDirectory()) {
      const walk = (at, to) => {
        mkdirSync(to, { recursive: true });
        for (const entry of readdirSync(at).sort()) {
          if (INPUT_SKIP.has(entry) || (skipped !== null && skipped.includes(entry))) continue;
          if (face !== null && entry === face) continue;
          const full = join(at, entry);
          const next = join(to, entry);
          if (statSync(full).isDirectory()) {
            walk(full, next);
            continue;
          }
          writeFileSync(next, readFileSync(full));
        }
      };
      walk(source, target);
      continue;
    }
    mkdirSync(join(target, '..'), { recursive: true });
    writeFileSync(target, readFileSync(source));
  }
}

export function buildFrontend(project, payload) {
  const root = payloadRootPath(project, payload);
  if (frontendMembership(project, payload.root) !== 'mini-app-payload') {
    fail(
      `${project.id} declares its frontend root ${JSON.stringify(payload.root)} as ${frontendMembership(project, payload.root)}, ` +
        'and a root this chain builds for the MiniApp is the one that is a payload',
    );
  }
  const manifest = join(root, 'package.json');
  if (!existsSync(manifest)) {
    fail(`${project.id}: the taken tree carries no ${manifest}, so nothing declares how its frontend is built`);
  }
  let declared;
  try {
    declared = JSON.parse(readFileSync(manifest, 'utf8'));
  } catch (error) {
    fail(`${project.id}: ${manifest} is not readable as JSON (${error.message}), so nothing says how its frontend is built`);
  }
  const script = declared.scripts?.build;
  if (typeof script !== 'string' || script === '') {
    fail(`${project.id}: its package.json names no \`build\` script, so this chain cannot produce its frontend`);
  }
  if (!/\bvite\s+build\b/.test(script)) {
    fail(`${project.id}: its \`build\` script is ${JSON.stringify(script)} and carries no \`vite build\`, so this chain cannot say which step of it writes the static frontend`);
  }
  if (!existsSync(join(root, 'src'))) {
    fail(`${project.id}: the taken tree carries no ${join(root, 'src')}, so there is no frontend source to build`);
  }
  const workspace = targetPath(FRONTEND_WORKSPACE, project.id);
  const sourceKey = createHash('sha256')
    .update(readFileSync(manifest))
    .update(script)
    .update(sourceDigest(join(root, 'src')))
    .digest('hex');
  const keyPath = join(workspace, '.lvpa-frontend-source.json');
  const recorded = () => {
    if (!existsSync(keyPath)) return null;
    let held;
    try {
      held = JSON.parse(readFileSync(keyPath, 'utf8')).source;
    } catch {
      fail(`${project.id}: ${keyPath} stands but does not read as JSON, so this step cannot say which sources the workspace holds`);
    }
    if (typeof held !== 'string') {
      fail(`${project.id}: ${keyPath} carries no \`source\` string, so this step cannot say which sources the workspace holds`);
    }
    return held;
  };
  const settled = recorded();
  if (settled === sourceKey && existsSync(join(workspace, 'dist', 'index.html'))) {
    process.stdout.write(
      `[sync-openalgo-frontend] ${project.id}: ${workspace} holds ${FRONTEND_INPUTS.join(', ')} built from the sources now on disk, ` +
        'so this step neither copies nor rebuilds\n',
    );
    return join(workspace, 'dist');
  }
  rmSync(workspace, { recursive: true, force: true });
  mkdirSync(workspace, { recursive: true });
  copyFrontendInputs(project, root, workspace, buildConfigFiles(project));
  pnpmIn(workspace, ['exec', 'vite', 'build'], `${project.id}'s build`);
  const out = join(workspace, 'dist');
  if (!existsSync(join(out, 'index.html'))) {
    fail(`${project.id}: \`${script}\` finished and ${join(out, 'index.html')} stands nowhere, so the build wrote outside the place this chain reads`);
  }
  writeFileSync(keyPath, `${JSON.stringify({ source: sourceKey }, null, 2)}\n`);
  return out;
}

function sourceDigest(dir) {
  const hash = createHash('sha256');
  const walk = (at) => {
    for (const entry of readdirSync(at).sort()) {
      const full = join(at, entry);
      const stat = statSync(full);
      if (stat.isDirectory()) {
        walk(full);
        continue;
      }
      hash.update(full.slice(dir.length)).update(readFileSync(full));
    }
  };
  walk(dir);
  return hash.digest('hex');
}

function copyBuiltApp(from, into) {
  const copied = [];
  const walk = (dir, prefix) => {
    for (const entry of readdirSync(dir).sort()) {
      const full = join(dir, entry);
      const stat = statSync(full);
      const relative = `${prefix}/${entry}`;
      if (stat.isDirectory()) {
        walk(full, relative);
        continue;
      }
      const destination = join(into, ...relative.split('/'));
      mkdirSync(join(destination, '..'), { recursive: true });
      writeFileSync(destination, readFileSync(full));
      copied.push(relative);
    }
  };
  walk(from, BUILT_APP_DIR);
  return copied;
}

export function landBundle(payload, project, target, built) {
  const facts = landingFacts(payload, project);
  if (existsSync(target)) rmSync(target, { recursive: true });
  mkdirSync(join(target, 'source'), { recursive: true });

  const bundle = join(target, 'source');
  const projected = [...projectSources(project, facts.root, target), ...copyBuiltApp(built, bundle)];
  for (const name of buildConfigFiles(project)) treeEntry(facts.root, name, target);
  writeFileSync(join(bundle, 'index.html'), readFileSync(facts.files['index.html']));
  writeFileSync(join(bundle, 'style.css'), readFileSync(join(facts.root, 'src', 'index.css')));
  writeFileSync(join(bundle, 'ui.js'), bootModule(payload));
  writeFileSync(join(bundle, 'worker.js'), 'export {};\n');
  writeFileSync(join(bundle, 'esm_dependencies.json'), '[]\n');
  writeFileSync(
    join(bundle, 'package.json'),
    `${JSON.stringify({ name: `miniapp-${payload.appId}`, private: true, dependencies: {} }, null, 2)}\n`,
  );

  writeFileSync(join(target, 'meta.json'), identityCard(payload));
  writeFileSync(join(target, 'index.html'), rootEntry(payload));
  writeFileSync(join(target, 'package.json'), `${JSON.stringify({ name: `miniapp-${payload.appId}`, private: true, dependencies: {} }, null, 2)}\n`);
  writeFileSync(join(target, 'storage.json'), '{}\n');
  writeFileSync(join(target, 'compiled.html'), '<!DOCTYPE html><html><head><meta charset="utf-8"></head><body>Loading...</body></html>');
  return { projected, facts };
}

export function replayPatches(project, target, patches) {
  const written = [];
  for (const { path, patch } of patches) {
    if (typeof patch.file !== 'string' || !patch.file.startsWith(`${project.root}/`)) {
      fail(`${path} targets ${JSON.stringify(patch.file)}, which does not stand in ${project.root}`);
    }
    const inside = patch.file.slice(project.root.length + 1);
    const file = join(target, ...inside.split('/'));
    const replacements = patch.replacements ?? [];
    const carried = patch.creates;
    if (replacements.length === 0 && carried === undefined) {
      fail(`${path} carries neither a replacement nor a file, so it changes nothing on the landed copy`);
    }
    if (replacements.length > 0 && !existsSync(file)) {
      fail(`${path} targets ${inside}, which the landed copy does not carry, so an anchor there names nothing`);
    }
    let text = existsSync(file) ? readFileSync(file, 'utf8') : '';
    for (const replacement of replacements) {
      if (typeof replacement.find !== 'string' || replacement.find === '') {
        fail(`${path} carries an empty find, and a file the host lands carries its own body rather than one a patch spells whole`);
      }
      if (replacement.find === replacement.replace) {
        fail(`${path} has the same find and replace, so it changes no byte`);
      }
      const hits = text.split(replacement.find).length - 1;
      if (hits === 0) {
        fail(
          `${path} finds no occurrence of its find text in ${inside} of the landed copy, so the premise it stands on does ` +
            'not hold in this copy and this chain lands nothing on a premise it cannot read',
        );
      }
      if (hits > 1) fail(`${path} finds ${hits} occurrences of its find text in ${inside}, and one anchor names one place`);
      text = text.split(replacement.find).join(replacement.replace);
    }
    if (existsSync(file)) {
      writeFileSync(file, text);
      written.push(inside);
    }
    for (const created of carried === undefined ? [] : [carried]) {
      if (typeof created.path !== 'string' || created.path === '') {
        fail(`${path} carries a file and names no path for it, so nothing says where its body is written`);
      }
      if (typeof created.body !== 'string' || created.body === '') {
        fail(`${path} carries ${created.path} and states no body for it, so the file it declares would be written empty`);
      }
      const destination = join(target, ...created.path.split('/'));
      if (existsSync(destination)) {
        fail(`${path} carries ${created.path}, and the landed copy already stands there`);
      }
      mkdirSync(join(destination, '..'), { recursive: true });
      writeFileSync(destination, created.body);
      written.push(created.path);
    }
  }
  return written;
}

export function syncOpenAlgoFrontend() {
  const payload = payloadDeclaration();
  const project = payloadProject(payload);
  const target = resolveLanding(payload);
  if (!existsSync(join(REPO_ROOT, ...TREE_RUNTIME.split('/')))) {
    fail(`the host carries no ${TREE_RUNTIME}, so the payload has no window to run in`);
  }
  const patches = declaredPatches(project);
  if (!patches.some(({ patch }) => patch.creates?.path === I18N_MODULE)) {
    fail(`patches/ declares none of them over ${I18N_MODULE} in ${project.root}, so the landed copy would read no Chinese word table`);
  }
  const landed = landBundle(payload, project, target, buildFrontend(project, payload));
  const patched = replayPatches(project, target, patches);
  return [{ id: project.id, target, projected: landed.projected.length, patched, root: landed.facts.root }];
}
