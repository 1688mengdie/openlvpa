#!/usr/bin/env node

import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));

export const REPO_ROOT = process.env.OPENLVPA_REPO_ROOT
  ? resolve(process.env.OPENLVPA_REPO_ROOT)
  : resolve(SCRIPT_DIR, '..');

export const DECLARATION_PATH = join(SCRIPT_DIR, 'absorbed-sources.json');

export function absorbedProjects() {
  if (!existsSync(DECLARATION_PATH)) {
    throw new Error(`[absorbed-sources] the declaration is missing: ${DECLARATION_PATH}`);
  }
  const declaration = JSON.parse(readFileSync(DECLARATION_PATH, 'utf8'));
  const projects = declaration?.projects;
  if (!Array.isArray(projects) || projects.length === 0) {
    throw new Error(`[absorbed-sources] ${DECLARATION_PATH} declares no projects, so there is no tree to take`);
  }
  const roots = new Set();
  const ids = new Set();
  for (const project of projects) {
    for (const key of ['id', 'root', 'url', 'branch', 'snapshotFile', 'stripManifest']) {
      if (typeof project?.[key] !== 'string' || project[key].length === 0) {
        throw new Error(`[absorbed-sources] a project entry is missing \`${key}\`: ${JSON.stringify(project)}`);
      }
    }
    if (roots.has(project.root)) {
      throw new Error(`[absorbed-sources] two projects declare the root \`${project.root}\`, so a lookup by root cannot choose`);
    }
    roots.add(project.root);
    if (ids.has(project.id)) {
      throw new Error(`[absorbed-sources] two projects declare the id \`${project.id}\`, so a take cannot be addressed to one of them`);
    }
    ids.add(project.id);
    mechanism(project);
    frontend(project);
  }
  return projects;
}

export function projectById(id) {
  const project = absorbedProjects().find((entry) => entry.id === id);
  if (!project) {
    throw new Error(`[absorbed-sources] no project \`${id}\` is declared. Declared: ${absorbedProjects().map((entry) => entry.id).join(', ')}`);
  }
  return project;
}

export function projectRoot(project) {
  return join(REPO_ROOT, ...project.root.split('/'));
}

export function snapshotPath(project) {
  return join(REPO_ROOT, ...project.snapshotFile.split('/'));
}

export function stripManifestPath(project) {
  return join(REPO_ROOT, ...project.stripManifest.split('/'));
}

export function packageManifestPath(project) {
  const declared = project?.sync?.packageManifest;
  if (typeof declared !== 'string' || declared === '') {
    throw new Error(`[absorbed-sources] the project \`${project?.id}\` declares no \`sync.packageManifest\``);
  }
  const root = project?.root;
  if (typeof root !== 'string' || root === '') {
    throw new Error(`[absorbed-sources] the project \`${project?.id}\` declares no \`root\``);
  }
  const rootSegments = root.replace(/\/+$/, '').split('/');
  const segments = declared.replace(/\/+$/, '').split('/');
  const underRoot = segments.length > rootSegments.length && rootSegments.every((segment, index) => segments[index] === segment);
  if (!underRoot) {
    throw new Error(
      `[absorbed-sources] the project \`${project?.id}\` names \`sync.packageManifest\` as ${JSON.stringify(declared)}, ` +
        `which does not stand under its declared root \`${root}\``,
    );
  }
  return segments.slice(rootSegments.length).join('/');
}

export function packageDirectory(project) {
  const declared = project?.sync?.packageDirectory;
  if (declared === undefined) return '';
  if (typeof declared !== 'string' || declared === '' || declared.startsWith('/') || declared.includes('..')) {
    throw new Error(
      `[absorbed-sources] the project \`${project?.id}\` names \`sync.packageDirectory\` as ${JSON.stringify(declared)}, ` +
        'which is not a directory inside its declared root. A tree whose root is the crate leaves the key out.',
    );
  }
  return declared.replace(/\/+$/, '');
}

export function packageDirectoryPath(project) {
  return join(projectRoot(project), ...packageDirectory(project).split('/').filter(Boolean));
}

export function buildConfigFiles(project) {
  const declared = project?.sync?.buildConfigFiles;
  if (!Array.isArray(declared) || declared.length === 0) {
    throw new Error(
      `[absorbed-sources] the project \`${project?.id}\` declares no \`sync.buildConfigFiles\`, so the take has no list of ` +
        'the build inputs it must find standing in that tree',
    );
  }
  for (const entry of declared) {
    if (typeof entry !== 'string' || entry === '' || entry.startsWith('/') || entry.includes('..')) {
      throw new Error(
        `[absorbed-sources] the project \`${project?.id}\` names \`sync.buildConfigFiles\` entry ${JSON.stringify(entry)}, ` +
          'which is not a path inside its declared root',
      );
    }
  }
  return [...declared];
}

export const MECHANISMS = Object.freeze(['cargo-workspace', 'single-crate']);
export const ROOT_MEMBERSHIPS = Object.freeze(['pnpm-closure', 'mini-app-payload', 'own-resolver']);

export function mechanism(project) {
  const declared = project?.sync?.mechanism;
  if (!MECHANISMS.includes(declared)) {
    throw new Error(
      `[absorbed-sources] the project \`${project?.id}\` declares \`sync.mechanism\` as ${JSON.stringify(declared)}, ` +
        `which is not one of ${MECHANISMS.join(', ')}`,
    );
  }
  return declared;
}

export function packageDirectoryRequirement(project) {
  return mechanism(project) === 'cargo-workspace' ? 'none' : 'required';
}

export function assertMechanismPairing(project) {
  const declared = project?.sync?.packageDirectory;
  const wants = packageDirectoryRequirement(project);
  const present = typeof declared === 'string' && declared !== '';
  if (wants === 'none' && present) {
    throw new Error(
      `[absorbed-sources] the project \`${project?.id}\` is a ${mechanism(project)} tree, whose root is the crate, ` +
        `and it names \`sync.packageDirectory\` as ${JSON.stringify(declared)}`,
    );
  }
  if (wants === 'required' && !present) {
    throw new Error(
      `[absorbed-sources] the project \`${project?.id}\` is a ${mechanism(project)} tree, so the directory holding its ` +
        'crate is not its root, and it names no `sync.packageDirectory`',
    );
  }
  return wants;
}

export function frontend(project) {
  const declared = project?.sync?.frontend;
  if (declared === undefined || declared === null) return null;
  if (typeof declared !== 'object' || Array.isArray(declared)) {
    throw new Error(`[absorbed-sources] the project \`${project?.id}\` declares \`sync.frontend\` as ${JSON.stringify(declared)}, which is not an object`);
  }
  const roots = declared.roots;
  if (!Array.isArray(roots) || roots.length === 0) {
    throw new Error(`[absorbed-sources] the project \`${project?.id}\` declares \`sync.frontend\` with no root, so the key describes nothing`);
  }
  for (const root of roots) {
    if (typeof root !== 'object' || root === null) {
      throw new Error(`[absorbed-sources] the project \`${project?.id}\` names a \`sync.frontend\` root as ${JSON.stringify(root)}, which is not an object`);
    }
    if (typeof root.path !== 'string' || root.path === '' || root.path.startsWith('/') || root.path.includes('..')) {
      throw new Error(
        `[absorbed-sources] the project \`${project?.id}\` names \`sync.frontend\` root path ${JSON.stringify(root.path)}, ` +
          'which is not a path inside its declared root',
      );
    }
    if (!ROOT_MEMBERSHIPS.includes(root.membership)) {
      throw new Error(
        `[absorbed-sources] the project \`${project?.id}\` names \`sync.frontend\` root ${JSON.stringify(root.path)} with ` +
          `membership ${JSON.stringify(root.membership)}, which is not one of ${ROOT_MEMBERSHIPS.join(', ')}`,
      );
    }
  }
  return { roots: roots.map((root) => ({ path: root.path, membership: root.membership })) };
}

export function frontendRoots(project) {
  const declared = frontend(project);
  return declared === null ? [] : declared.roots;
}
