#!/usr/bin/env node
import { fileURLToPath } from 'node:url';
import { fail, payloadDeclaration, resolveLanding, syncOpenAlgoFrontend as land } from './miniapp-entry.mjs';

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const report = land();
  for (const line of report) {
    process.stdout.write(
      `[sync-openalgo-frontend] landed ${line.id} at ${line.target}: ${line.projected} projected file(s), ` +
        `${line.patched.length} patched file(s)\n`,
    );
  }
}

export { fail, payloadDeclaration, resolveLanding, land as syncOpenAlgoFrontend };