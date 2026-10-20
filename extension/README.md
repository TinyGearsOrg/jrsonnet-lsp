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

## Install

- **VSCodium and other Open VSX editors:** search for *Jsonnet (jrsonnet-lsp)* in the extensions view,
  or open it on [Open VSX](https://open-vsx.org/extension/TinyGears/jrsonnet-lsp).
- **Visual Studio Code:** it uses a different registry, so download the `.vsix` for your platform
  from the [GitHub releases](https://github.com/TinyGearsOrg/jrsonnet-lsp/releases) and run
  `code --install-extension jrsonnet-lsp-<platform>-<version>.vsix`, or use *Extensions: Install from
  VSIX...*.

## Platforms

Packages are built for Linux and Alpine (x64 and arm64), macOS (arm64 and x64) and Windows (x64 and
arm64). The Linux server is a static musl binary, so it runs on any distribution whatever its glibc
version. On another platform, build `jsonnet-lsp` yourself (see below) and point
`jrsonnet-lsp.server.path` at it.

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
`code --install-extension extension/jrsonnet-lsp-linux-x64-<version>.vsix`.

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

### Releasing

The extension is published to [Open VSX](https://open-vsx.org), the registry used by VSCodium and
other editors built on Open VSX, with [`ovsx`](https://www.npmjs.com/package/ovsx). It is not
published to the Visual Studio Marketplace.

1. Bump `version` in `package.json`, add it to `CHANGELOG.md` and commit.
2. Push a tag `vX.Y.Z` that matches the version. The Release workflow builds the packages for every
   platform, starts the binaries that can run on their runner, and creates a GitHub release with all
   the VSIXes attached.
3. **First release, by hand.** You need an Open VSX account with a
   [personal access token](https://open-vsx.org/user-settings/tokens), and the namespace has to
   exist: it is the `publisher` of `package.json`, `TinyGears`. Create it once, then upload the
   VSIXes of the GitHub release. `ovsx` is pinned in `publish/`, apart from the extension's own
   dependencies:

   ```sh
   cd publish && npm ci --ignore-scripts
   export OVSX_PAT=...                       # the personal access token
   npx ovsx create-namespace TinyGears
   npx ovsx publish --packagePath path/to/*.vsix
   ```

4. **Switch to trusted publishing.** It publishes from GitHub Actions with a short-lived credential
   that Open VSX issues in exchange for the workflow's OpenID Connect token, so no token is stored
   anywhere. Open VSX only does that for a workflow that a namespace owner has registered under
   [trusted publishers](https://open-vsx.org/user-settings/trusted-publishers): register this
   repository, `TinyGearsOrg/jrsonnet-lsp`, and its workflow `release.yml`. Then set the repository
   variable `OPEN_VSX_PUBLISH` to `true` (Settings, Secrets and variables, Actions, Variables).
5. From then on, pushing a tag also publishes: the `open-vsx` job runs
   `ovsx publish --trusted-publishing --skip-duplicate`. It is the only job that can request an
   OIDC token, it installs without running install scripts, and it cannot write to the repository.
   `--skip-duplicate` makes a version that is already there, such as the one uploaded by hand, a
   no-op instead of an error, and a re-run after a partial failure safe.

Running the Release workflow by hand (Actions, *Run workflow*) builds and packages everything but
releases and publishes nothing, which is the way to test a change to it.

### The icon

`images/icon.svg` is the editable source and `images/icon.png` is what ships (VS Code does not accept
SVG icons). Both are generated by `npm run icon` (`scripts/render-icon.mjs`), which
draws a gear between curly braces from plain shapes, so it needs no fonts. Edit the script, rerun it
and commit the results. It also writes `.github/social-preview.png`, the 1280x640 image for the
repository's social preview; GitHub has no API for that, so upload it by hand under *Settings*,
*General*, *Social preview*.

### Packaging for other platforms

A VSIX is built per VS Code target and carries exactly one server binary, in `server/`.
`bundle-server.mjs` puts a built binary there, and `package.mjs` builds one VSIX per target from
it, so a single binary can serve several targets (the static Linux one serves `linux-x64` and
`alpine-x64`):

```sh
node scripts/bundle-server.mjs --platform linux-x64 --binary ../target/x86_64-unknown-linux-musl/release/jsonnet-lsp
node scripts/package.mjs --target linux-x64,alpine-x64
```

`--platform` only decides whether the binary is called `.exe`. Without arguments both scripts
use this machine's platform, which is what `make package` does. CI builds the real packages per
platform; see `.github/workflows/release.yml`, where a platform is one matrix entry. Running that
workflow by hand builds and packages everything without releasing anything.

## License

MIT, see [LICENSE](LICENSE).
