// Runs inside VS Code's extension host (bundled to out/integration.js) against the demo workspace.
// Everything goes through VS Code's public API, so it exercises the extension, the language
// client and the server together, the way a user's editor does.

import assert from 'node:assert/strict';
import * as vscode from 'vscode';

const results: { name: string; error?: unknown }[] = [];

async function test(name: string, body: () => Promise<void>): Promise<void> {
  try {
    await body();
    results.push({ name });
    console.log(`  ok   ${name}`);
  } catch (error) {
    results.push({ name, error });
    console.log(`  FAIL ${name}\n       ${error instanceof Error ? error.message : String(error)}`);
  }
}

async function waitFor<T>(what: string, probe: () => T | undefined, timeoutMs = 60_000): Promise<T> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const value = probe();
    if (value !== undefined) {
      return value;
    }
    if (Date.now() > deadline) {
      throw new Error(`timed out waiting for ${what}`);
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
}

const text = (contents: vscode.Hover['contents']): string =>
  (Array.isArray(contents) ? contents : [contents])
    .map((c) => (typeof c === 'string' ? c : c.value))
    .join('\n');

export async function run(): Promise<void> {
  const folder = vscode.workspace.workspaceFolders?.[0];
  assert.ok(folder, 'the demo workspace is open');
  const mainUri = vscode.Uri.joinPath(folder.uri, 'main.jsonnet');
  const utilUri = vscode.Uri.joinPath(folder.uri, 'lib', 'util.libsonnet');

  const extension = vscode.extensions.getExtension('tinygearsorg.jrsonnet-lsp');
  assert.ok(extension, 'the extension is installed');
  await extension.activate();

  const doc = await vscode.workspace.openTextDocument(mainUri);
  await vscode.window.showTextDocument(doc);
  const at = (needle: string, delta = 0): vscode.Position => {
    const i = doc.getText().indexOf(needle);
    assert.ok(i >= 0, `${needle} is in main.jsonnet`);
    return doc.positionAt(i + delta);
  };
  const diagnostics = () => vscode.languages.getDiagnostics(mainUri);
  const exec = <T>(command: string, ...args: unknown[]) => vscode.commands.executeCommand<T>(command, ...args);

  await test('the file is a Jsonnet document', async () => {
    assert.equal(doc.languageId, 'jsonnet');
  });

  await test('diagnostics: undefined variable and missing import', async () => {
    const found = await waitFor('diagnostics', () => {
      const d = diagnostics();
      return d.length >= 2 ? d : undefined;
    });
    const by = (severity: vscode.DiagnosticSeverity) => found.filter((d) => d.severity === severity);
    assert.equal(by(vscode.DiagnosticSeverity.Error).length, 1);
    assert.match(by(vscode.DiagnosticSeverity.Error)[0]?.message ?? '', /undefined variable `undefinedVariable`/);
    assert.equal(by(vscode.DiagnosticSeverity.Warning).length, 1);
    assert.match(by(vscode.DiagnosticSeverity.Warning)[0]?.message ?? '', /nope\.libsonnet/);
  });

  await test('hover shows the binding', async () => {
    const hovers = await exec<vscode.Hover[]>('vscode.executeHoverProvider', mainUri, at('greet(name)', 2));
    assert.match(hovers.map((h) => text(h.contents)).join('\n'), /greet/);
  });

  await test('go to definition: into an imported file through an index, and through self', async () => {
    const into = (await exec<vscode.Location[]>('vscode.executeDefinitionProvider', mainUri, at("'config']", 2)))[0];
    assert.equal(into?.uri.toString(), utilUri.toString());
    assert.equal(into?.range.start.line, 3); // `config:` in lib/util.libsonnet
    const self = (await exec<vscode.Location[]>('vscode.executeDefinitionProvider', mainUri, at('self.again', 6)))[0];
    assert.equal(self?.uri.toString(), mainUri.toString());
    assert.equal(self?.range.start.line, doc.positionAt(doc.getText().indexOf('again:')).line);
  });

  await test('signature help: local, destructured and std functions', async () => {
    const label = async (needle: string, delta: number) => {
      const help = await exec<vscode.SignatureHelp | undefined>('vscode.executeSignatureHelpProvider', mainUri, at(needle, delta), '(');
      return help?.signatures[0]?.label ?? null;
    };
    assert.deepEqual(
      { add: await label('add(1', 4), greet: await label('greet(name)', 6), length: await label('length(', 7) },
      { add: 'add(a, b = 1)', greet: 'greet(name)', length: 'std.length(x)' },
    );
  });

  await test('completion: std members and variables in scope', async () => {
    const scratch = await vscode.workspace.openTextDocument({ language: 'jsonnet', content: 'std.' });
    await vscode.window.showTextDocument(scratch);
    const list = await exec<vscode.CompletionList>('vscode.executeCompletionItemProvider', scratch.uri, new vscode.Position(0, 4), '.');
    const labels = list.items.map((i) => (typeof i.label === 'string' ? i.label : i.label.label));
    assert.ok(labels.includes('length') && labels.includes('manifestJson'), `got ${labels.length} items`);
    await vscode.window.showTextDocument(doc);
  });

  await test('document symbols', async () => {
    const symbols = await exec<vscode.DocumentSymbol[]>('vscode.executeDocumentSymbolProvider', mainUri);
    const names = symbols.map((s) => s.name);
    assert.ok(names.includes('add') && names.includes('hello'), names.join(','));
  });

  await test('formatting', async () => {
    const scratch = await vscode.workspace.openTextDocument({ language: 'jsonnet', content: '{a:1,b:  [1,2]}' });
    const edits = await exec<vscode.TextEdit[]>('vscode.executeFormatDocumentProvider', scratch.uri, { tabSize: 2, insertSpaces: true });
    // VS Code may turn a whole-document replacement into several minimal edits, so apply them all.
    const original = scratch.getText();
    let formatted = original;
    for (const e of [...edits].sort((a, b) => scratch.offsetAt(b.range.start) - scratch.offsetAt(a.range.start))) {
      formatted = formatted.slice(0, scratch.offsetAt(e.range.start)) + e.newText + formatted.slice(scratch.offsetAt(e.range.end));
    }
    assert.equal(formatted.trimEnd(), '{ a: 1, b: [ 1, 2 ] }');
  });

  await test('rename and references', async () => {
    const edit = await exec<vscode.WorkspaceEdit>('vscode.executeDocumentRenameProvider', mainUri, at('add(1', 1), 'plus');
    const edits = edit.get(mainUri);
    assert.equal(edits.length, 2); // the declaration and the one use
    assert.ok(edits.every((e) => e.newText === 'plus'));
    const refs = await exec<vscode.Location[]>('vscode.executeReferenceProvider', mainUri, at('add(1', 1));
    assert.equal(refs.length, 2);
  });

  await test('import paths are links', async () => {
    const links = await exec<vscode.DocumentLink[]>('vscode.executeLinkProvider', mainUri);
    assert.ok(links.some((l) => l.target?.toString() === utilUri.toString()), 'a link to lib/util.libsonnet');
  });

  await test('edits are synchronised incrementally', async () => {
    const editor = await vscode.window.showTextDocument(doc);
    const start = doc.positionAt(doc.getText().indexOf('undefinedVariable'));
    const range = new vscode.Range(start, start.translate(0, 'undefinedVariable'.length));
    assert.ok(await editor.edit((b) => b.replace(range, '1')));
    await waitFor('the error to go away', () => (diagnostics().length === 1 ? true : undefined));
    assert.ok(await editor.edit((b) => b.replace(new vscode.Range(start, start.translate(0, 1)), 'nope')));
    await waitFor('the error to come back', () =>
      diagnostics().some((d) => /undefined variable `nope`/.test(d.message)) ? true : undefined,
    );
  });

  await test('restarting the server keeps working', async () => {
    await exec('jrsonnet-lsp.restartServer');
    const edit = new vscode.WorkspaceEdit();
    edit.insert(mainUri, new vscode.Position(0, 0), '// touched\n');
    await vscode.workspace.applyEdit(edit);
    await waitFor('diagnostics after the restart', () => (diagnostics().some((d) => /nope/.test(d.message)) ? true : undefined));
    const hovers = await exec<vscode.Hover[]>('vscode.executeHoverProvider', mainUri, at('greet(name)', 2));
    assert.match(hovers.map((h) => text(h.contents)).join('\n'), /greet/);
  });

  const failed = results.filter((r) => r.error !== undefined);
  console.log(`${results.length - failed.length}/${results.length} integration tests passed`);
  if (failed.length > 0) {
    throw new Error(`${failed.length} integration test(s) failed: ${failed.map((f) => f.name).join('; ')}`);
  }
}
