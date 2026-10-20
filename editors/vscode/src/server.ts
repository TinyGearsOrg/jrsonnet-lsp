// Where to find the language server binary. Pure (no `vscode` import) so it runs under plain Node
// in the unit tests.

import * as path from 'node:path';

export type Source = 'setting' | 'bundled' | 'development';

export interface Candidate {
  path: string;
  source: Source;
}

export interface Options {
  /** The `jrsonnet-lsp.server.path` setting; empty when unset. */
  configured: string;
  extensionPath: string;
  /** Running from source (F5), where a `cargo build` output next to the extension will do. */
  development: boolean;
  platform: NodeJS.Platform;
  arch: string;
  home: string;
}

export type Resolved = { ok: true; path: string; source: Source } | { ok: false; message: string };

/** The VS Code target name for a platform, e.g. `linux-x64`; the bundled binary's directory. */
export function target(platform: string, arch: string): string {
  return `${platform}-${arch}`;
}

export function binaryName(platform: string): string {
  return platform === 'win32' ? 'jsonnet-lsp.exe' : 'jsonnet-lsp';
}

function expandHome(p: string, home: string): string {
  return p === '~' || p.startsWith('~/') ? path.join(home, p.slice(1)) : p;
}

/** Places to look, in order of preference. */
export function candidates(o: Options): Candidate[] {
  const configured = o.configured.trim();
  if (configured) {
    // An explicit choice is never silently replaced by something else.
    return [{ path: expandHome(configured, o.home), source: 'setting' }];
  }
  const bin = binaryName(o.platform);
  const list: Candidate[] = [
    { path: path.join(o.extensionPath, 'server', target(o.platform, o.arch), bin), source: 'bundled' },
  ];
  if (o.development) {
    // editors/vscode/../../target
    const cargoTarget = path.join(o.extensionPath, '..', '..', 'target');
    for (const profile of ['debug', 'release']) {
      list.push({ path: path.join(cargoTarget, profile, bin), source: 'development' });
    }
  }
  return list;
}

/**
 * Picks the server to run. `mtime` gives a file's modification time, or `undefined` when it
 * doesn't exist; among development builds the most recently built one wins.
 */
export function resolveServer(o: Options, mtime: (file: string) => number | undefined): Resolved {
  const list = candidates(o);
  const found = list
    .map((c) => ({ c, time: mtime(c.path) }))
    .filter((f): f is { c: Candidate; time: number } => f.time !== undefined);
  const best = found.reduce<(typeof found)[number] | undefined>(
    (a, b) => (a && a.time >= b.time ? a : b),
    undefined,
  );
  if (best) {
    return { ok: true, path: best.c.path, source: best.c.source };
  }
  if (o.configured.trim()) {
    return {
      ok: false,
      message: `jrsonnet-lsp.server.path is set to "${list[0]?.path}", but there is no file there.`,
    };
  }
  const tried = list.map((c) => `  ${c.path}`).join('\n');
  return {
    ok: false,
    message:
      `No language server binary for ${target(o.platform, o.arch)} was found. Looked in:\n${tried}\n` +
      'Set jrsonnet-lsp.server.path to a jsonnet-lsp binary, or install a build of the extension ' +
      'that bundles one for your platform.',
  };
}
