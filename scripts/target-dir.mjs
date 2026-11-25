import { join } from 'node:path';

export const TARGET_DIR = 'G:/openlvpa-target';

export function targetDir() {
  return process.env.CARGO_TARGET_DIR || TARGET_DIR;
}

export function targetPath(...segments) {
  return join(targetDir(), ...segments);
}
