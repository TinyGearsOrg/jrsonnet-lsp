<p align="center"><img src="extension/images/icon.png" width="128" alt="jrsonnet-lsp icon: a gear between curly braces"></p>

<h1 align="center">jrsonnet-lsp</h1>

<p align="center"><b>A language server for Jsonnet</b><br>
built on jrsonnet's parser and formatter, with a VS Code extension that bundles it</p>

<p align="center">
  <a href="https://github.com/TinyGearsOrg/jrsonnet-lsp/actions/workflows/ci.yml"><img src="https://github.com/TinyGearsOrg/jrsonnet-lsp/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue" alt="License: MIT"></a>
  <img src="https://img.shields.io/badge/Rust-stable-orange" alt="Rust: stable">
  <img src="https://img.shields.io/badge/VS%20Code-1.91%2B-blue" alt="VS Code 1.91+">
  <a href="https://github.com/deltarocks/jrsonnet"><img src="https://img.shields.io/badge/built%20on-jrsonnet-8a5cf5" alt="Built on jrsonnet"></a>
</p>

A language server for [Jsonnet](https://jsonnet.org), built on the CST parser and
formatter from [jrsonnet](https://github.com/deltarocks/jrsonnet), so it can follow
jrsonnet's language extensions. It talks LSP over stdio.

Early work in progress. A VS Code extension that bundles the server lives in
[`extension`](extension); its README has setup and a manual test checklist.

## Supported

- Syntax error diagnostics, and `undefined variable` for names that don't resolve
- Formatting (via `jrsonnet-formatter`)
- Incremental text sync (the document is still fully re-parsed on each change)
- Go to definition, find references, and rename for local variables, function
  parameters, destructured names and comprehension variables
- `import`, `importstr` and `importbin` paths: go to the imported file, clickable links,
  and a warning when the file can't be found, re-checked when files change on disk (needs a
  client that supports watched-file registration)
- Go to definition of `x.field` and completion after `.`, following object literals, `+`,
  parentheses, `local` names, `import`s, `self`/`super`/`$` and chained fields across files
  (static, best effort: no calls, conditionals or `std`), including through destructuring
  such as `local {foo, nested: {deep}} = lib;` or `local [first, second] = lib.list;`, and through
  `x["name"]`, `x?.["name"]` and `x[0]` with literal indexes
- Hover: a local's binding, a field's declaration (also across files), `std` members with their parameters,
  and where an import path resolves to
- Signature help for calls to `local` functions, to methods or function-valued fields found
  through the same lookup, and to `std` functions (with jrsonnet's real parameter names),
  highlighting the active parameter (by position or `name =`)
- `std.` completion with each function's parameters (read from jrsonnet's own `std` at the pinned
  rev), completion of variables in scope, and a document outline of top-level locals and object
  fields

## Import search paths

An import is looked up relative to the importing file first, then in each workspace
folder and its `vendor/` (jsonnet-bundler) and `lib/` directories, then in `jpath`. Set
`jpath` through the client's `initializationOptions`, e.g. `{"jpath": ["extra", "/abs/dir"]}`;
relative entries are relative to the first workspace folder. A missing import is only a
warning, since the real run may use different `-J` paths.

## Planned

- Prose documentation for `std` functions (only names and parameters are known today)

## Building

`rust-toolchain.toml` selects the latest stable Rust (rustup installs it on first use). The
server needs `SyntaxError` exported from `jrsonnet-rowan-parser`, which the pinned jrsonnet rev
doesn't do yet. `patches/export-syntax-error.patch` adds the export, and `.cargo/config.toml`
points the jrsonnet crates at a patched checkout in `.jrsonnet/`. Cargo can't create that before
resolving dependencies, so set up a fresh checkout first:

```sh
make init     # fetches jrsonnet at the pinned rev, applies patches/, installs the extension's npm packages
make build    # server (release) and extension
make test     # server tests and extension unit tests
```

`make` lists every target (`lint`, `fmt`, `package`, `test-integration`, ...). Without the
extension, `./scripts/setup-jrsonnet.sh && cargo build` is enough.

Run the setup script again after bumping the rev in `Cargo.toml` or changing a patch; it does
nothing when `.jrsonnet/` is already current. Set `JRSONNET_URL` to fetch from a mirror or a fork.
Once the export is released upstream, bump the rev and delete the script, `patches/` and
`.cargo/config.toml`.

## Development

`cargo test` (or `make test-server`) runs unit tests plus end-to-end tests that drive the binary
over stdio. `make test-integration` starts a real VS Code and tests the extension through its API.

`cargo run --example dump -- '<jsonnet>'` prints the CST for a snippet, which is handy since the
parser's tree doesn't always match `jsonnet.ungram`.

`make gen-stdlib` regenerates the `std` table in `src/stdlib/table.rs` from jrsonnet's own
evaluator. Rerun it after bumping the jrsonnet rev.

## jrsonnet extensions

The CST parser accepts jrsonnet's extensions unconditionally, and the server understands them:
destructuring in `local` and parameters (`{a, b: [c, ...rest]}`, `?` skips, defaults),
`for [k, v] in obj` comprehensions, and `??` / `?.`. Names, references and rename work in all of
them, and fields can be followed through object patterns. `std` completion lists whatever
jrsonnet's own `std` provides, including its additions to the standard library.

## Platforms

Releases are built for Linux and Alpine (x64 and arm64, static musl binaries that run on any
distribution), macOS (arm64 and x64) and Windows (x64 and arm64).

CI runs the tests on Linux, macOS and Windows, and the extension in a real VS Code on each of them.
The release build starts every binary that can run on its build machine (Linux x64 and arm64, macOS
arm64, Windows x64) and checks that it answers LSP. The macOS x64 and Windows arm64 binaries are
cross-compiled and have only been built, not run. Windows drive-letter URIs (`file:///c%3A/...` as
VS Code sends them, or `file:///C:/...`) are handled; UNC paths (`file://server/share/...`) are not
supported.

## License

MIT, see [LICENSE](LICENSE), the same license as jrsonnet, which this builds on.
