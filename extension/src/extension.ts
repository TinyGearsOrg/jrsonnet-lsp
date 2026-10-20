import * as fs from 'node:fs';
import * as os from 'node:os';
import * as vscode from 'vscode';
import {
  LanguageClient,
  State,
  TransportKind,
  type LanguageClientOptions,
  type ServerOptions,
} from 'vscode-languageclient/node';
import { resolveServer } from './server.ts';

const SECTION = 'jrsonnet-lsp';

let client: LanguageClient | undefined;
let status: vscode.LanguageStatusItem;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  status = vscode.languages.createLanguageStatusItem(SECTION, { language: 'jsonnet' });
  status.name = 'Jsonnet language server';
  status.text = 'jrsonnet-lsp';

  context.subscriptions.push(
    status,
    vscode.commands.registerCommand(`${SECTION}.restartServer`, () => restart(context)),
    vscode.commands.registerCommand(`${SECTION}.showServerLog`, () => client?.outputChannel.show()),
    // Both settings are only read when the server starts.
    vscode.workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration(`${SECTION}.server.path`) || e.affectsConfiguration(`${SECTION}.jpath`)) {
        void restart(context);
      }
    }),
  );
  await start(context);
}

export async function deactivate(): Promise<void> {
  await stop();
}

async function start(context: vscode.ExtensionContext): Promise<void> {
  const config = vscode.workspace.getConfiguration(SECTION);
  const resolved = resolveServer(
    {
      configured: config.get<string>('server.path', ''),
      extensionPath: context.extensionPath,
      // Running from source (F5 or the integration tests): prefer a fresh `cargo build`.
      development: context.extensionMode !== vscode.ExtensionMode.Production,
      platform: process.platform,
      arch: process.arch,
      home: os.homedir(),
    },
    (file) => {
      try {
        return fs.statSync(file).mtimeMs;
      } catch {
        return undefined;
      }
    },
  );
  if (!resolved.ok) {
    fail(resolved.message);
    return;
  }
  makeExecutable(resolved.path);

  const serverOptions: ServerOptions = {
    command: resolved.path,
    args: [],
    transport: TransportKind.stdio,
  };
  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: 'file', language: 'jsonnet' },
      { scheme: 'untitled', language: 'jsonnet' },
    ],
    initializationOptions: { jpath: config.get<string[]>('jpath', []) },
    outputChannelName: 'Jsonnet Language Server',
  };

  const next = new LanguageClient(SECTION, 'Jsonnet Language Server', serverOptions, clientOptions);
  next.onDidChangeState(({ newState }) => {
    if (client !== next) {
      return;
    }
    status.severity = vscode.LanguageStatusSeverity.Information;
    status.text = newState === State.Running ? 'jrsonnet-lsp' : 'jrsonnet-lsp (stopped)';
    status.detail = `${resolved.source}: ${resolved.path}`;
  });
  client = next;
  try {
    await next.start();
  } catch (error) {
    client = undefined;
    fail(`The language server at ${resolved.path} failed to start: ${String(error)}`);
  }
}

async function stop(): Promise<void> {
  const current = client;
  client = undefined;
  await current?.stop();
}

async function restart(context: vscode.ExtensionContext): Promise<void> {
  await stop();
  await start(context);
}

function fail(message: string): void {
  status.severity = vscode.LanguageStatusSeverity.Error;
  status.text = 'jrsonnet-lsp: not running';
  status.detail = message.split('\n')[0];
  void vscode.window
    .showErrorMessage(`Jsonnet: ${message}`, 'Open Settings')
    .then((choice) => {
      if (choice) {
        return vscode.commands.executeCommand('workbench.action.openSettings', `${SECTION}.server.path`);
      }
      return undefined;
    });
}

/** Packaging can drop the executable bit; the binary has to be runnable. */
function makeExecutable(file: string): void {
  if (process.platform === 'win32') {
    return;
  }
  try {
    fs.chmodSync(file, 0o755);
  } catch {
    // Not ours to change (e.g. a system-wide binary): leave it, it may already work.
  }
}
