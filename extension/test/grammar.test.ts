// Tokenizes Jsonnet with the same TextMate engine VS Code uses and checks the scopes.

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { before, test } from 'node:test';
import textmate, { type IGrammar, type StateStack } from 'vscode-textmate';
import oniguruma from 'vscode-oniguruma';

const { INITIAL, Registry, parseRawGrammar } = textmate;
const { createOnigScanner, createOnigString, loadWASM } = oniguruma;

const require = createRequire(import.meta.url);

let grammar: IGrammar;

before(async () => {
  const wasm = readFileSync(require.resolve('vscode-oniguruma/release/onig.wasm'));
  await loadWASM(wasm.buffer.slice(wasm.byteOffset, wasm.byteOffset + wasm.byteLength) as ArrayBuffer);
  const registry = new Registry({
    onigLib: Promise.resolve({ createOnigScanner, createOnigString }),
    loadGrammar: async (scope) => {
      if (scope !== 'source.jsonnet') {
        return null;
      }
      // A URL object (not its `pathname`) works with fs on every platform.
      const url = new URL('../syntaxes/jsonnet.tmLanguage.json', import.meta.url);
      return parseRawGrammar(readFileSync(url, 'utf8'), 'jsonnet.tmLanguage.json');
    },
  });
  const loaded = await registry.loadGrammar('source.jsonnet');
  assert.ok(loaded, 'grammar loads');
  grammar = loaded;
});

/** `[text, innermost scope]` for each token of the lines. */
function tokens(...lines: string[]): [string, string][][] {
  let state: StateStack = INITIAL;
  return lines.map((line) => {
    const result = grammar.tokenizeLine(line, state);
    state = result.ruleStack;
    return result.tokens.map((t): [string, string] => [
      line.slice(t.startIndex, t.endIndex),
      t.scopes[t.scopes.length - 1] ?? '',
    ]);
  });
}

/** The scope of the first token containing `text`. */
function scopeOf(line: string, text: string): string | undefined {
  const [row] = tokens(line);
  return row?.find(([t]) => t.includes(text))?.[1];
}

test('comments', () => {
  assert.equal(scopeOf('// a comment', 'comment'), 'comment.line.double-slash.jsonnet');
  assert.equal(scopeOf('# a comment', 'comment'), 'comment.line.number-sign.jsonnet');
  // Only the marker is punctuation.
  assert.equal(scopeOf('// a comment', '//'), 'punctuation.definition.comment.jsonnet');
  const rows = tokens('/* start', 'middle */ 1');
  assert.equal(rows[0]?.at(-1)?.[1], 'comment.block.jsonnet');
  assert.equal(rows[1]?.[0]?.[1], 'comment.block.jsonnet');
  assert.equal(rows[1]?.at(-1)?.[1], 'constant.numeric.jsonnet');
});

test('strings', () => {
  assert.equal(scopeOf(`"plain"`, 'plain'), 'string.quoted.double.jsonnet');
  assert.equal(scopeOf(`'single'`, 'single'), 'string.quoted.single.jsonnet');
  assert.equal(scopeOf(`"a\\nb"`, '\\n'), 'constant.character.escape.jsonnet');
  assert.equal(scopeOf(`"bad \\q"`, '\\q'), 'invalid.illegal.unknown-escape.jsonnet');
  // A comment marker inside a string is not a comment.
  assert.equal(scopeOf(`"http://x"`, 'http'), 'string.quoted.double.jsonnet');
  assert.equal(scopeOf(`@"C:\\dir ""q"""`, 'dir'), 'string.quoted.double.verbatim.jsonnet');
});

test('text blocks span lines', () => {
  const rows = tokens('local t = |||', '  text // not a comment', '|||;', '1');
  assert.equal(rows[1]?.[0]?.[1], 'string.unquoted.block.jsonnet');
  assert.equal(rows[2]?.[0]?.[1], 'string.unquoted.block.jsonnet');
  // The block has ended, so the next line is code again.
  assert.equal(rows[3]?.[0]?.[1], 'constant.numeric.jsonnet');
});

test('numbers, constants and the special variables', () => {
  assert.equal(scopeOf('12.5e-3', '12.5e-3'), 'constant.numeric.jsonnet');
  assert.equal(scopeOf('true', 'true'), 'constant.language.jsonnet');
  assert.equal(scopeOf('null', 'null'), 'constant.language.jsonnet');
  assert.equal(scopeOf('self.x', 'self'), 'variable.language.jsonnet');
  assert.equal(scopeOf('$.x', '$'), 'variable.language.jsonnet');
  assert.equal(scopeOf('super.x', 'super'), 'variable.language.jsonnet');
});

test('keywords', () => {
  for (const kw of ['if', 'then', 'else', 'for', 'in', 'assert', 'error']) {
    assert.equal(scopeOf(`${kw} x`, kw), 'keyword.control.jsonnet', kw);
  }
  assert.equal(scopeOf(`import 'a'`, 'import'), 'keyword.control.import.jsonnet');
  assert.equal(scopeOf(`importstr 'a'`, 'importstr'), 'keyword.control.import.jsonnet');
  assert.equal(scopeOf('local x = 1;', 'local'), 'storage.type.local.jsonnet');
  assert.equal(scopeOf('function(a) a', 'function'), 'storage.type.function.jsonnet');
  // A word that merely contains a keyword is an ordinary identifier.
  assert.equal(scopeOf('format', 'format'), 'source.jsonnet');
});

test('std members and function definitions', () => {
  const [row] = tokens('std.length(x)');
  assert.deepEqual(row?.slice(0, 3), [
    ['std', 'support.constant.std.jsonnet'],
    ['.', 'punctuation.accessor.jsonnet'],
    ['length', 'support.function.builtin.jsonnet'],
  ]);
  assert.equal(scopeOf('local add(a, b) = a + b;', 'add'), 'entity.name.function.jsonnet');
});

test('object fields, but not slices', () => {
  assert.equal(scopeOf('{ name: 1 }', 'name'), 'variable.other.property.jsonnet');
  assert.equal(scopeOf('{ name:: 1 }', 'name'), 'variable.other.property.jsonnet');
  assert.equal(scopeOf('{ name+: 1 }', 'name'), 'variable.other.property.jsonnet');
  assert.equal(scopeOf('{ method(x):: x }', 'method'), 'variable.other.property.jsonnet');
  assert.equal(scopeOf('xs[a:b]', 'a'), 'source.jsonnet');
});

test('operators', () => {
  assert.equal(scopeOf('a ?? b', '??'), 'keyword.operator.jsonnet');
  assert.equal(scopeOf('a == b', '=='), 'keyword.operator.jsonnet');
  assert.equal(scopeOf('a?.b', '?.'), 'keyword.operator.jsonnet');
});
