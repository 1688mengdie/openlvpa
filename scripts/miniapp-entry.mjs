#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { REPO_ROOT, absorbedProjects, frontendRoots, projectRoot } from './absorbed-sources.mjs';
import { targetPath } from './target-dir.mjs';

const DECLARATION = join(REPO_ROOT, 'scripts', 'miniapp-payload.json');
const PATCH_DIR = join(REPO_ROOT, 'patches');
const TREE_RUNTIME = 'obf/src/apps/miniapp-shell';

const SOURCE_NAMES = ['index.html', 'style.css', 'ui.js', 'worker.js', 'esm_dependencies.json'];

const BUILT_APP_DIR = 'app';

export function fail(message) {
  process.stderr.write(`[sync-openalgo-frontend] ${message}\n`);
  process.exit(1);
}

function pnpm() {
  return process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm';
}

export function payloadDeclaration() {
  if (!existsSync(DECLARATION)) {
    fail(`the payload declaration is missing: ${DECLARATION}`);
  }
  const payload = JSON.parse(readFileSync(DECLARATION, 'utf8'));
  for (const key of ['appId', 'routes', 'styles', 'index']) {
    if (typeof payload[key] !== 'string' || payload[key] === '') {
      fail(`${DECLARATION} names no \`${key}\`, so the landed MiniApp would carry an incomplete identity card`);
    }
  }
  const extra = Object.keys(payload).filter((key) => !['_comment', 'appId', 'routes', 'styles', 'index'].includes(key));
  if (extra.length > 0) {
    fail(`${DECLARATION} carries ${extra.join(', ')}, and a patch states itself in patches/ rather than a second time here`);
  }
  return payload;
}

export function payloadProjects(payload) {
  const declared = absorbedProjects();
  const entries = [];
  const covered = new Set();
  for (const project of declared) {
    if (frontendRoots(project).every((root) => root.membership !== 'mini-app-payload')) continue;
    if (covered.has(project.id)) continue;
    covered.add(project.id);
    entries.push({ project, entry: { routes: payload.routes, styles: payload.styles, index: payload.index } });
  }
  for (const project of declared) {
    if (covered.has(project.id)) continue;
    if (frontendRoots(project).some((root) => root.membership === 'mini-app-payload')) {
      fail(`${project.id} declares a \`mini-app-payload\` frontend root and no entry names it`);
    }
  }
  if (entries.length === 0) {
    fail(`${DECLARATION} names a payload and no absorbed tree declares a \`mini-app-payload\` frontend root to fill it`);
  }
  return entries;
}

export function declaredPatches(project) {
  const patches = [];
  for (const name of readdirSync(PATCH_DIR).sort()) {
    if (!name.endsWith('.patch.json')) continue;
    const path = join(PATCH_DIR, name);
    const patch = JSON.parse(readFileSync(path, 'utf8'));
    if (typeof patch.file !== 'string' || patch.file === '') {
      fail(`${path} names no \`file\`, so nothing says which copy it is replayed over`);
    }
    if (patch.landed === undefined) continue;
    if (typeof patch.landed !== 'string' || patch.landed === '') {
      fail(`${path} carries \`landed\` that says nothing, and a patch replayed over the landed copy says so in words`);
    }
    if (patch.file.split('/')[0] !== project.root) continue;
    patches.push({ name, path, patch });
  }
  return patches;
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

export function landingFacts(entry) {
  const root = projectRoot(entry.project);
  const files = {};
  for (const name of ['routes', 'styles', 'index']) {
    const declared = entry.entry[name];
    const path = join(root, ...declared.split('/'));
    if (!existsSync(path)) {
      fail(`${entry.project.id}: the taken tree carries no ${declared}, so the landed payload would have no ${name}`);
    }
    files[name] = path;
  }
  for (const name of ['index.html', 'openscript-runner.html']) {
    const path = join(root, name);
    if (existsSync(path)) files[name] = path;
  }
  return { root, files };
}

function projectSources(project, target) {
  const root = projectRoot(project);
  const source = join(root, 'src');
  if (!existsSync(source)) {
    fail(`${project.id}: the taken tree carries no ${source}, so the landed copy would carry no source for the patches to land on`);
  }
  const projected = [];
  const walk = (dir, prefix) => {
    for (const entry of readdirSync(dir).sort()) {
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

function rootEntry(payload) {
  return [
    '<!doctype html>',
    '<html lang="en">',
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

const FRONTEND_WORKSPACE = 'lvpa-frontend';

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

function copyFrontendInputs(from, into) {
  for (const name of FRONTEND_INPUTS) {
    const source = join(from, name);
    if (!existsSync(source)) continue;
    const target = join(into, name);
    if (statSync(source).isDirectory()) {
      const walk = (at, to) => {
        mkdirSync(to, { recursive: true });
        for (const entry of readdirSync(at).sort()) {
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

export function buildFrontend(project) {
  const root = projectRoot(project);
  const manifest = join(root, 'package.json');
  if (!existsSync(manifest)) {
    fail(`${project.id}: the taken tree carries no ${manifest}, so nothing declares how its frontend is built`);
  }
  const declared = JSON.parse(readFileSync(manifest, 'utf8'));
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
        'so this step neither copies, installs nor rebuilds\n',
    );
    return join(workspace, 'dist');
  }
  rmSync(workspace, { recursive: true, force: true });
  mkdirSync(workspace, { recursive: true });
  copyFrontendInputs(root, workspace);
  const vite = join(workspace, 'node_modules', '.bin', process.platform === 'win32' ? 'vite.cmd' : 'vite');
  const installed = spawnSync(
    pnpm(),
    [
      'install',
      '--ignore-workspace',
      '--lockfile=false',
      '--config.strict-dep-builds=false',
      '--config.public-hoist-pattern=*',
    ],
    {
      cwd: workspace,
      stdio: 'inherit',
      shell: pnpm().endsWith('.cmd'),
    },
  );
  if (installed.status !== 0) {
    fail(`${project.id}: pnpm install --ignore-workspace exited ${installed.status} under ${workspace}, and its \`build\` script needs that package set`);
  }
  if (!existsSync(vite)) {
    fail(`${project.id}: ${vite} is not on disk after the install under ${workspace}, so this chain has no bundler to call`);
  }
  const built = spawnSync(vite, ['build'], {
    cwd: workspace,
    stdio: 'inherit',
    shell: vite.endsWith('.cmd'),
    env: { ...process.env, NODE_ENV: 'production' },
  });
  if (built.status !== 0) {
    fail(`${project.id}: its \`${script}\` script exited ${built.status} under ${workspace}, so the landed payload would carry no static frontend`);
  }
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

export function landBundle(payload, entry, target, built) {
  const facts = landingFacts(entry);
  if (existsSync(target)) rmSync(target, { recursive: true });
  mkdirSync(join(target, 'source'), { recursive: true });

  const bundle = join(target, 'source');
  const projected = [...projectSources(entry.project, target), ...copyBuiltApp(built, bundle)];
  writeFileSync(join(bundle, 'index.html'), readFileSync(facts.files.index));
  writeFileSync(join(bundle, 'style.css'), readFileSync(facts.files.styles));
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
  for (const { name, path, patch } of patches) {
    if (typeof patch.file !== 'string' || !patch.file.startsWith(`${project.root}/`)) {
      fail(`${path} targets ${JSON.stringify(patch.file)}, which does not stand in ${project.root}`);
    }
    const inside = patch.file.slice(project.root.length + 1);
    const file = join(target, ...inside.split('/'));
    const replacements = patch.replacements ?? [];
    if (replacements.length === 0) {
      fail(`${path} carries no replacement, so it changes no byte`);
    }
    const creates = patch.creates === true;
    const wholeFile = replacements.every((replacement) => replacement.find === '');
    if (creates !== wholeFile) {
      fail(`${path} declares creates=${creates} and its replacements are ${wholeFile ? 'whole-file' : 'anchored'}, and a file a patch creates is the only form that spells a file whole`);
    }
    if (creates) {
      if (existsSync(file)) {
        fail(`${path} declares itself the creator of ${inside}, and the landed copy already carries it`);
      }
      if (replacements.length !== 1) {
        fail(`${path} creates ${inside} and states ${replacements.length} replacements, and a created file is written once`);
      }
      mkdirSync(join(file, '..'), { recursive: true });
      writeFileSync(file, replacements[0].replace);
      written.push(inside);
      continue;
    }
    if (!existsSync(file)) {
      fail(`${path} targets ${inside}, which the landed copy does not carry, and the patch does not declare creates`);
    }
    let text = readFileSync(file, 'utf8');
    for (const replacement of replacements) {
      if (replacement.find === '') {
        fail(`${path} carries an empty find, which is the whole-file form, and only a patch declaring creates may state it`);
      }
      if (replacement.find === replacement.replace) {
        fail(`${path} has the same find and replace, so it changes no byte`);
      }
      const hits = text.split(replacement.find).length - 1;
      if (hits === 0) fail(`${path} finds no occurrence of its find text in ${inside} of the landed copy`);
      if (hits > 1) fail(`${path} finds ${hits} occurrences of its find text in ${inside}, and one anchor names one place`);
      text = text.split(replacement.find).join(replacement.replace);
    }
    writeFileSync(file, text);
    written.push(inside);
  }
  return written;
}

export function syncOpenAlgoFrontend() {
  const payload = payloadDeclaration();
  const entries = payloadProjects(payload);
  const target = resolveLanding(payload);
  const report = [];
  for (const entry of entries) {
    if (!existsSync(join(REPO_ROOT, ...TREE_RUNTIME.split('/')))) {
      fail(`the host carries no ${TREE_RUNTIME}, so the payload has no window to run in`);
    }
    const patches = declaredPatches(entry.project);
    if (patches.length === 0) {
      fail(`patches/ declares no patch over ${entry.project.root}, so the landed copy would carry the shell's own calls`);
    }
    const landed = landBundle(payload, entry, target, buildFrontend(entry.project));
    const patched = replayPatches(entry.project, target, patches);
    report.push({ id: entry.project.id, target, projected: landed.projected.length, patched, root: landed.facts.root });
  }
  return report;
}
