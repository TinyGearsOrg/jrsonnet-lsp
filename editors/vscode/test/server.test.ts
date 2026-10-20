import assert from 'node:assert/strict';
import { test } from 'node:test';
import { binaryName, candidates, resolveServer, target, type Options } from '../src/server.ts';

const base: Options = {
  configured: '',
  extensionPath: '/ext/jrsonnet-lsp',
  development: false,
  platform: 'linux',
  arch: 'x64',
  home: '/home/me',
};

const files = (known: Record<string, number>) => (file: string) => known[file];

test('target and binary names', () => {
  assert.equal(target('linux', 'x64'), 'linux-x64');
  assert.equal(binaryName('linux'), 'jsonnet-lsp');
  assert.equal(binaryName('win32'), 'jsonnet-lsp.exe');
});

test('the bundled binary is used by default', () => {
  const bundled = '/ext/jrsonnet-lsp/server/linux-x64/jsonnet-lsp';
  const r = resolveServer(base, files({ [bundled]: 1 }));
  assert.deepEqual(r, { ok: true, path: bundled, source: 'bundled' });
});

test('a configured path wins, and is expanded', () => {
  const r = resolveServer(
    { ...base, configured: ' ~/bin/jsonnet-lsp ' },
    files({ '/home/me/bin/jsonnet-lsp': 1, '/ext/jrsonnet-lsp/server/linux-x64/jsonnet-lsp': 1 }),
  );
  assert.deepEqual(r, { ok: true, path: '/home/me/bin/jsonnet-lsp', source: 'setting' });
});

test('a configured path that does not exist is an error, not a fallback', () => {
  const r = resolveServer(
    { ...base, configured: '/nope/jsonnet-lsp' },
    files({ '/ext/jrsonnet-lsp/server/linux-x64/jsonnet-lsp': 1 }),
  );
  assert.equal(r.ok, false);
  assert.match(r.ok ? '' : r.message, /server\.path is set to "\/nope\/jsonnet-lsp"/);
});

test('development builds come from the repo, and the newest wins', () => {
  // The extension lives in <repo>/editors/vscode, next to <repo>/target.
  const dev = { ...base, extensionPath: '/repo/editors/vscode', development: true };
  const bundled = '/repo/editors/vscode/server/linux-x64/jsonnet-lsp';
  const debug = '/repo/target/debug/jsonnet-lsp';
  const release = '/repo/target/release/jsonnet-lsp';
  assert.deepEqual(candidates(dev).map((c) => c.path), [bundled, debug, release]);

  assert.deepEqual(resolveServer(dev, files({ [debug]: 5, [release]: 9 })), {
    ok: true,
    path: release,
    source: 'development',
  });
  assert.deepEqual(resolveServer(dev, files({ [debug]: 9, [release]: 5 })), {
    ok: true,
    path: debug,
    source: 'development',
  });
  // A stale bundled binary does not hide a fresher build, but a fresher bundled one wins.
  assert.deepEqual(resolveServer(dev, files({ [bundled]: 1, [debug]: 2 })), {
    ok: true,
    path: debug,
    source: 'development',
  });
  assert.deepEqual(resolveServer(dev, files({ [bundled]: 3, [debug]: 2 })), {
    ok: true,
    path: bundled,
    source: 'bundled',
  });
});

test('development paths are not tried outside development', () => {
  assert.deepEqual(candidates(base).map((c) => c.source), ['bundled']);
});

test('a missing binary explains where it looked and what to do', () => {
  const r = resolveServer({ ...base, platform: 'darwin', arch: 'arm64' }, files({}));
  assert.equal(r.ok, false);
  const message = r.ok ? '' : r.message;
  assert.match(message, /darwin-arm64/);
  assert.match(message, /server\/darwin-arm64\/jsonnet-lsp/);
  assert.match(message, /jrsonnet-lsp\.server\.path/);
});
