import assert from 'node:assert/strict';
import * as path from 'node:path';
import { test } from 'node:test';
import { binaryName, candidates, resolveServer, target, type Options } from '../src/server.ts';

// Paths are built with `path.join`, so the expectations hold on Windows as well.
const p = (...parts: string[]) => path.join(path.sep, ...parts);

const base: Options = {
  configured: '',
  extensionPath: p('ext', 'jrsonnet-lsp'),
  development: false,
  platform: 'linux',
  arch: 'x64',
  home: p('home', 'me'),
};

const files = (known: Record<string, number>) => (file: string) => known[file];

test('target and binary names', () => {
  assert.equal(target('linux', 'x64'), 'linux-x64');
  assert.equal(binaryName('linux'), 'jsonnet-lsp');
  assert.equal(binaryName('win32'), 'jsonnet-lsp.exe');
});

test('the bundled binary is used by default, whatever the platform', () => {
  for (const [platform, arch, bin] of [
    ['linux', 'x64', 'jsonnet-lsp'],
    ['linux', 'arm', 'jsonnet-lsp'], // armhf
    ['darwin', 'arm64', 'jsonnet-lsp'],
    ['win32', 'x64', 'jsonnet-lsp.exe'],
  ] as const) {
    const bundled = path.join(base.extensionPath, 'server', bin);
    const r = resolveServer({ ...base, platform, arch }, files({ [bundled]: 1 }));
    assert.deepEqual(r, { ok: true, path: bundled, source: 'bundled' }, `${platform}-${arch}`);
  }
});

test('a configured path wins, and is expanded', () => {
  const home = path.join(base.home, 'bin', 'jsonnet-lsp');
  const bundled = path.join(base.extensionPath, 'server', 'jsonnet-lsp');
  const r = resolveServer({ ...base, configured: ' ~/bin/jsonnet-lsp ' }, files({ [home]: 1, [bundled]: 1 }));
  assert.deepEqual(r, { ok: true, path: home, source: 'setting' });
});

test('a configured path that does not exist is an error, not a fallback', () => {
  const bundled = path.join(base.extensionPath, 'server', 'jsonnet-lsp');
  const r = resolveServer({ ...base, configured: p('nope', 'jsonnet-lsp') }, files({ [bundled]: 1 }));
  assert.equal(r.ok, false);
  assert.ok((r.ok ? '' : r.message).includes(`server.path is set to "${p('nope', 'jsonnet-lsp')}"`));
});

test('development builds come from the repo, and the newest wins', () => {
  // The extension lives in <repo>/extension, next to <repo>/target.
  const dev = { ...base, extensionPath: p('repo', 'extension'), development: true };
  const bundled = p('repo', 'extension', 'server', 'jsonnet-lsp');
  const debug = p('repo', 'target', 'debug', 'jsonnet-lsp');
  const release = p('repo', 'target', 'release', 'jsonnet-lsp');
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
  assert.ok(message.includes(path.join('server', 'jsonnet-lsp')));
  assert.match(message, /jrsonnet-lsp\.server\.path/);
});
