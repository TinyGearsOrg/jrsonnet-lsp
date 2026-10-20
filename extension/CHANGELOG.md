# Changelog

## 0.0.3

- Fixed a crash of the server on incomplete code: an object field without a `:` (for example a
  stray `+` before the closing `}`), a bare `function` and a bare `import` made the parser panic.
  They are now reported as syntax errors.

## 0.0.2

- Packages for Linux and Alpine (x64 and arm64), macOS (arm64 and x64) and Windows (x64 and arm64),
  built by CI, with each binary that can run on its build machine started and checked.
- The Linux server is a static musl binary, so it runs on any distribution whatever its glibc
  version.
- An extension icon, and the MIT license in the package.
- Published to Open VSX.

## 0.0.1

First version: language server bundled for linux-x64, syntax highlighting, and the settings and
commands described in the README.
