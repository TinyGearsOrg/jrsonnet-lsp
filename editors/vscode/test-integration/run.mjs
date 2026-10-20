// Launches a real VS Code with the extension loaded from source and runs out/integration.js in
// its extension host. Needs a display (use xvfb-run on a headless machine).
//
//   VSCODE_VERSION=1.140.0 xvfb-run -a node test-integration/run.mjs
//
// Set JRSONNET_LSP_SERVER_PATH to run the tests against a particular server binary (or a
// wrapper script that logs the traffic) instead of the one the extension would pick.

import { runTests } from '@vscode/test-electron';
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');

// A throwaway profile, so the tests never touch the user's settings and can set machine-scoped ones.
const userData = mkdtempSync(join(tmpdir(), 'jsonnet-vscode-test-'));
mkdirSync(join(userData, 'User'), { recursive: true });
const settings = {};
if (process.env.JRSONNET_LSP_SERVER_PATH) {
  settings['jrsonnet-lsp.server.path'] = process.env.JRSONNET_LSP_SERVER_PATH;
}
writeFileSync(join(userData, 'User', 'settings.json'), JSON.stringify(settings));

try {
  await runTests({
    version: process.env.VSCODE_VERSION ?? 'stable',
    // Point this at an unpacked VSIX to test the package as users get it.
    extensionDevelopmentPath: process.env.EXTENSION_DEVELOPMENT_PATH ?? root,
    extensionTestsPath: join(root, 'out', 'integration.js'),
    launchArgs: [
      join(root, 'demo'),
      `--user-data-dir=${userData}`,
      '--disable-extensions',
      '--disable-gpu',
      '--disable-workspace-trust',
      '--no-sandbox',
    ],
  });
} catch (error) {
  console.error('Integration tests failed:', error);
  process.exit(1);
}
