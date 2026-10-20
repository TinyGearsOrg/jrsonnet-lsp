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
  and a warning when the file can't be found, re-checked when files change on disk (needs a
  client that supports watched-file registration)
- Go to definition of `x.field` and completion after `.`, following object literals, `+`,
  parentheses, `local` names, `import`s, `self`/`super`/`$` and chained fields across files
  (static, best effort: no calls, conditionals or `std`), including through object destructuring
  such as `local {foo, nested: {deep}} = lib;`
- Hover: a local's binding, a field's declaration (also across files), `std` members by name,
  and where an import path resolves to
- `std.` completion (names taken from jrsonnet's own `std` at the pinned rev), completion of
  variables in scope, and a document outline of top-level locals and object fields

## Import search paths

An import is looked up relative to the importing file first, then in each workspace
folder and its `vendor/` (jsonnet-bundler) and `lib/` directories, then in `jpath`. Set
`jpath` through the client's `initializationOptions`, e.g. `{"jpath": ["extra", "/abs/dir"]}`;
relative entries are relative to the first workspace folder. A missing import is only a
warning, since the real run may use different `-J` paths.

## Planned

- Signature help, and std documentation in hover
- Field lookup through array destructuring and `?.[...]` indexing
- Windows paths
- Incremental text sync, and hint-level diagnostics once `SyntaxError` is exported upstream

## Development

`cargo test` runs unit tests plus end-to-end tests that drive the binary over stdio.
`cargo run --example dump -- '<jsonnet>'` prints the CST for a snippet, which is handy
since the parser's tree doesn't always match `jsonnet.ungram`.

jrsonnet is pinned by git rev in `Cargo.toml` because its API is still moving.

## jrsonnet extensions

The CST parser accepts jrsonnet's extensions unconditionally, and the server understands them:
destructuring in `local` and parameters (`{a, b: [c, ...rest]}`, `?` skips, defaults),
`for [k, v] in obj` comprehensions, and `??` / `?.`. Names, references and rename work in all of
them, and fields can be followed through object patterns. `std` completion lists whatever
jrsonnet's own `std` provides, including its additions to the standard library.
