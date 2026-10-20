# jrsonnet-lsp

A language server for [Jsonnet](https://jsonnet.org), built on the CST parser and
formatter from [jrsonnet](https://github.com/deltarocks/jrsonnet), so it can follow
jrsonnet's language extensions.

Early work in progress. It talks LSP over stdio: `cargo run`.

## Supported

- Syntax error diagnostics, and `undefined variable` for names that don't resolve
- Formatting (via `jrsonnet-formatter`)
- Go to definition, find references, and rename for local variables, function
  parameters, destructured names and comprehension variables

- `import`, `importstr` and `importbin` paths: go to the imported file, clickable links,
  and a warning when the file can't be found
- Go to definition of `x.field` and completion after `.`, following object literals, `+`,
  parentheses, `local` names, `import`s and chained fields across files (static, best effort:
  no calls, conditionals, `self`/`super` or `std`)
- `std.` completion (names taken from jrsonnet's own `std` at the pinned rev), completion of
  variables in scope, and a document outline of top-level locals and object fields

## Import search paths

An import is looked up relative to the importing file first, then in each workspace
folder and its `vendor/` (jsonnet-bundler) and `lib/` directories, then in `jpath`. Set
`jpath` through the client's `initializationOptions`, e.g. `{"jpath": ["extra", "/abs/dir"]}`;
relative entries are relative to the first workspace folder. A missing import is only a
warning, since the real run may use different `-J` paths.

## Planned

XX

## Development

`cargo test` runs unit tests plus end-to-end tests that drive the binary over stdio.
`cargo run --example dump -- '<jsonnet>'` prints the CST for a snippet, which is handy
since the parser's tree doesn't always match `jsonnet.ungram`.

jrsonnet is pinned by git rev in `Cargo.toml` because its API is still moving.
