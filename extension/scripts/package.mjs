// Builds one platform-specific VSIX per target from the bundled server (see bundle-server.mjs).
//
//   node scripts/package.mjs                          # this platform
//   node scripts/package.mjs --target linux-x64,alpine-x64
//
// Targets are VS Code's names: win32-x64, win32-arm64, linux-x64, linux-arm64, linux-armhf,
// alpine-x64, alpine-arm64, darwin-x64, darwin-arm64. This replaces a shell one-liner so it also
// works where npm scripts run under cmd.exe.

import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');

/** This machine's VS Code target. */
function hostTarget() {
  const arch = process.platform === 'linux' && process.arch === 'arm' ? 'armhf' : process.arch;
  return `${process.platform}-${arch}`;
}

const { values } = parseArgs({ options: { target: { type: 'string', default: hostTarget() } } });
const targets = values.target.split(',').map((t) => t.trim()).filter(Boolean);

if (!existsSync(join(root, 'server'))) {
  console.error('No server/ directory: run bundle-server.mjs first (make bundle).');
  process.exit(1);
}

const vsce = join(root, 'node_modules', '@vscode', 'vsce', 'vsce');
for (const target of targets) {
  const run = spawnSync(process.execPath, [vsce, 'package', '--no-dependencies', '--target', target], {
    cwd: root,
    stdio: 'inherit',
  });
  if (run.status !== 0) {
    process.exit(run.status ?? 1);
  }
}
