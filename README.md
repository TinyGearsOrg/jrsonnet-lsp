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

## Planned

- `import` resolution and cross-file navigation
- Document symbols, `std.*` completion and hover
- Field completion and navigation

## Development

`cargo test` runs unit tests plus an end-to-end test that drives the binary over stdio.
`cargo run --example dump -- '<jsonnet>'` prints the CST for a snippet, which is handy
since the parser's tree doesn't always match `jsonnet.ungram`.

jrsonnet is pinned by git rev in `Cargo.toml` because its API is still moving.
