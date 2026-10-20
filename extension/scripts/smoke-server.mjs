// Starts a built jsonnet-lsp and checks that it speaks LSP: initialize, a diagnostic for a
// document with a syntax error, and a clean shutdown. Used on the release builds, to prove that
// the binary for a platform runs there.
//
//   node scripts/smoke-server.mjs path/to/jsonnet-lsp

import { spawn } from 'node:child_process';

const binary = process.argv[2];
if (!binary) {
  console.error('usage: smoke-server.mjs <binary>');
  process.exit(2);
}

const child = spawn(binary, [], { stdio: ['pipe', 'pipe', 'inherit'] });
const timer = setTimeout(() => fail('timed out after 30 s'), 30_000);
let exited;
const exit = new Promise((resolve) => child.on('exit', (code, signal) => resolve((exited = { code, signal }))));
child.on('error', (error) => fail(`could not start ${binary}: ${error.message}`));

function fail(message) {
  console.error(`smoke test failed: ${message}`);
  child.kill();
  process.exit(1);
}

function send(message) {
  const body = JSON.stringify({ jsonrpc: '2.0', ...message });
  child.stdin.write(`Content-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`);
}

// Messages from the server, in order, for `next` to take.
const queue = [];
let waiting;
let pending = Buffer.alloc(0);
child.stdout.on('data', (chunk) => {
  pending = Buffer.concat([pending, chunk]);
  for (;;) {
    const header = pending.indexOf('\r\n\r\n');
    if (header < 0) return;
    const length = Number(/Content-Length: (\d+)/i.exec(pending.subarray(0, header).toString())?.[1]);
    if (pending.length < header + 4 + length) return;
    const message = JSON.parse(pending.subarray(header + 4, header + 4 + length).toString());
    pending = pending.subarray(header + 4 + length);
    if (waiting) {
      const resolve = waiting;
      waiting = undefined;
      resolve(message);
    } else {
      queue.push(message);
    }
  }
});
const next = () => (queue.length > 0 ? Promise.resolve(queue.shift()) : new Promise((resolve) => (waiting = resolve)));

send({ id: 1, method: 'initialize', params: { capabilities: {} } });
const init = await next();
if (init.id !== 1 || init.result?.capabilities?.hoverProvider !== true) {
  fail(`unexpected initialize response: ${JSON.stringify(init).slice(0, 200)}`);
}
send({ method: 'initialized', params: {} });

send({
  method: 'textDocument/didOpen',
  params: { textDocument: { uri: 'file:///smoke.jsonnet', languageId: 'jsonnet', version: 1, text: '{ a: 1 +' } },
});
const diagnostics = await next();
if (diagnostics.method !== 'textDocument/publishDiagnostics' || diagnostics.params.diagnostics.length === 0) {
  fail(`expected diagnostics for a syntax error, got ${JSON.stringify(diagnostics).slice(0, 200)}`);
}

send({ id: 2, method: 'shutdown' });
const shutdown = await next();
if (shutdown.id !== 2) {
  fail(`unexpected shutdown response: ${JSON.stringify(shutdown).slice(0, 200)}`);
}
send({ method: 'exit' });
child.stdin.end();
await exit;
clearTimeout(timer);
if (exited.code !== 0) {
  fail(`exited with ${exited.code ?? exited.signal}`);
}
console.log(`smoke test passed: ${binary}`);
