// Copies a built jsonnet-lsp binary into server/<target>/, where the extension looks for it.
//
//   node scripts/bundle-server.mjs                     # this platform, target/release build
//   node scripts/bundle-server.mjs --platform linux-arm64 --binary path/to/jsonnet-lsp
//
// <target> is the VS Code target name (linux-x64, darwin-arm64, win32-x64, ...), which is also
// what `vsce package --target` takes.

import { chmodSync, copyFileSync, existsSync, mkdirSync } from 'node:fs';
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

const platform = values.platform;
const exe = platform.startsWith('win32') ? 'jsonnet-lsp.exe' : 'jsonnet-lsp';
const binary = values.binary ?? join(root, '..', 'target', 'release', exe);

if (!existsSync(binary)) {
  console.error(`No server binary at ${binary}. Build it first (make build-server) or pass --binary.`);
  process.exit(1);
}

const dir = join(root, 'server', platform);
mkdirSync(dir, { recursive: true });
const dest = join(dir, exe);
copyFileSync(binary, dest);
chmodSync(dest, 0o755);
console.log(`bundled ${binary} -> ${dest}`);
