// Copies a built jsonnet-lsp binary to server/, where the extension looks for it. A
// platform-specific VSIX carries exactly one binary, so there is no per-platform directory.
//
//   node scripts/bundle-server.mjs                     # target/release build, for this platform
//   node scripts/bundle-server.mjs --platform win32-x64 --binary path/to/jsonnet-lsp.exe
//
// --platform is the VS Code target name; it only decides whether the file is called `.exe`.

import { chmodSync, copyFileSync, existsSync, mkdirSync, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const { values } = parseArgs({
  options: {
    platform: { type: 'string', default: `${process.platform}-${process.arch}` },
    binary: { type: 'string' },
  },
});

const exe = values.platform.startsWith('win32') ? 'jsonnet-lsp.exe' : 'jsonnet-lsp';
const binary = values.binary ?? join(root, '..', 'target', 'release', exe);

if (!existsSync(binary)) {
  console.error(`No server binary at ${binary}. Build it first (make build-server) or pass --binary.`);
  process.exit(1);
}

// Start clean, so a binary left over from another platform can't end up in the package.
const dir = join(root, 'server');
rmSync(dir, { recursive: true, force: true });
mkdirSync(dir, { recursive: true });
const dest = join(dir, exe);
copyFileSync(binary, dest);
chmodSync(dest, 0o755);
console.log(`bundled ${binary} -> ${dest}`);
