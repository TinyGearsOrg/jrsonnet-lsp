# Jsonnet (jrsonnet-lsp)

Jsonnet language support for VS Code, powered by a language server built on the parser and
formatter of [jrsonnet](https://github.com/deltarocks/jrsonnet). The extension bundles the server,
so there is nothing else to install.

## Features

- Syntax highlighting, comment toggling and bracket handling for `.jsonnet`, `.libsonnet` and
  `.libjsonnet` files
- Diagnostics for syntax errors, undefined variables and imports that can't be found
- Go to definition, find references and rename for variables, parameters and destructured names
- Go to definition and completion for object fields (`lib.field`, `lib["field"]`, `self`, `super`,
  `$`), following imports, `+`, `local` aliases and destructuring
- Import paths are links; hover shows where they resolve to
- Hover, signature help and completion for `std` with jrsonnet's real parameter names
- Signature help for your own functions and methods, document outline, formatting
- jrsonnet's syntax extensions: destructuring, `??`, `?.`, `for [k, v] in obj`

Lookups are static and best effort: calls, conditionals and comprehensions are not evaluated.

## Platforms

Only **linux-x64** is packaged for now. On other platforms, build `jsonnet-lsp` yourself (see
below) and point `jrsonnet-lsp.server.path` at it.

## Settings

| Setting | Default | |
|---|---|---|
| `jrsonnet-lsp.server.path` | `""` | A `jsonnet-lsp` binary to use instead of the bundled one. User settings only. |
| `jrsonnet-lsp.jpath` | `[]` | Extra import search directories, like `jrsonnet -J`. Relative entries are relative to the first workspace folder. |
| `jrsonnet-lsp.trace.server` | `off` | `messages` or `verbose` logs the protocol traffic to the output channel. |

Imports are looked up relative to the importing file, then in each workspace folder and its
`vendor/` and `lib/` directories, then in `jpath`. Changing `server.path` or `jpath` restarts the
server.

## Commands

- **Jsonnet: Restart Language Server**
- **Jsonnet: Show Language Server Output**

The language status item (the `{}` icon in the status bar, or *Show Language Status*) shows which
server binary is running.

## Developing

Prerequisites: a Rust toolchain (`rust-toolchain.toml` selects the latest stable), Node 20+ and
`make`. From the repository root:

```sh
make init     # patched jrsonnet sources, npm packages
make build    # server (release) and extension
make test     # server tests, extension unit tests
make lint     # rustfmt, clippy, type-check
make package  # VSIX for this platform, with the server bundled
```

`make` lists all targets.

### Trying it by hand

1. `make init build`
2. Open `extension` in VS Code and press **F5** (*Run Extension (demo workspace)*). A second
   window opens on `extension/demo` with the extension loaded from source. When it runs from
   source, the extension uses the newest of `target/debug/jsonnet-lsp`, `target/release/jsonnet-lsp`
   and a bundled binary, so `cargo build` followed by *Jsonnet: Restart Language Server* picks up
   server changes.
3. In that window, open `main.jsonnet` and go through the checklist below. `highlighting.jsonnet`
   is for judging syntax highlighting.

To try the packaged extension instead: `make package`, then
`code --install-extension extension/jrsonnet-lsp-linux-x64-0.0.1.vsix`.

### Checklist for `demo/main.jsonnet`

| Try | Expect |
|---|---|
| Open the Problems panel | A warning on `'nope.libsonnet'` and an error on `undefinedVariable` |
| Ctrl+click `'lib/util.libsonnet'` | Opens the file. Hovering the string shows the resolved path |
| F12 on `config` in `util['config']` | Jumps to `config:` in `lib/util.libsonnet`; F12 on the following `name` goes to its `name:` |
| F12 on `again` in `self.again`, and on `total` in `$.total` | Jumps to those fields in `main.jsonnet` |
| Hover `greet` in `greet(name)` | Shows `local { greet, config: { name } } = util` |
| Hover `length` in `std.length` | Shows `std.length(x)` |
| Put the cursor after `add(` (or press Ctrl+Shift+Space) | Parameter hints `add(a, b = 1)`; after typing a comma the second parameter is highlighted |
| The same after `greet(` and `std.length(` | `greet(name)` and `std.length(x)` |
| Type `std.` on a new line | Completion list with each function's parameters |
| Ctrl+Shift+O | Outline with `add`, `hello`, `total`, ... |
| F2 on `add` | Renames the declaration and its use; Shift+F12 lists both |
| Replace `undefinedVariable` with `1` | The error disappears as you type |
| Create `demo/nope.libsonnet` (anything, e.g. `{}`) | The warning on the import disappears |
| Add `{a:1,b:  [1,2]}` somewhere valid and format the document | The text is reformatted |
| Run *Jsonnet: Restart Language Server* | Everything keeps working |
| Set `jrsonnet-lsp.trace.server` to `verbose`, then *Show Language Server Output* | The protocol traffic is logged |

### Tests

- `npm test` (in this directory): unit tests for the server lookup and the TextMate grammar
- `npm run test:integration`: starts a real VS Code, loads the extension from source and drives
  it through VS Code's API against the demo workspace. On a machine without a display, run it as
  `xvfb-run -a npm run test:integration`. `VSCODE_VERSION` selects the VS Code version.
  `JRSONNET_LSP_SERVER_PATH` runs it against a particular server binary.
- The server side of the demo is checked by `demo_workspace` in the repository's `tests/stdio.rs`

### Packaging for other platforms

`vsce package --target <target>` builds a platform-specific VSIX; the server binary for that
platform goes in `server/<target>/` first:

```sh
node scripts/bundle-server.mjs --platform darwin-arm64 --binary path/to/jsonnet-lsp
npx vsce package --no-dependencies --target darwin-arm64
```

CI does this per platform; see `.github/workflows/release.yml`.

## License

MIT, see [LICENSE](LICENSE).
