//! End-to-end smoke test: drive the built binary over stdio.

use std::{
	io::{BufRead, BufReader, Read, Write},
	process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

use serde_json::{Value, json};

struct Client {
	child: Child,
	stdin: ChildStdin,
	stdout: BufReader<ChildStdout>,
}

impl Client {
	fn start() -> Self {
		let mut child = Command::new(env!("CARGO_BIN_EXE_jsonnet-lsp"))
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.spawn()
			.unwrap();
		let stdin = child.stdin.take().unwrap();
		let stdout = BufReader::new(child.stdout.take().unwrap());
		Self {
			child,
			stdin,
			stdout,
		}
	}

	fn send(&mut self, msg: Value) {
		let body = msg.to_string();
		write!(self.stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
		self.stdin.flush().unwrap();
	}

	fn recv(&mut self) -> Value {
		let mut len = 0;
		loop {
			let mut line = String::new();
			self.stdout.read_line(&mut line).unwrap();
			let line = line.trim();
			if line.is_empty() {
				break;
			}
			if let Some(v) = line.strip_prefix("Content-Length:") {
				len = v.trim().parse().unwrap();
			}
		}
		let mut buf = vec![0; len];
		self.stdout.read_exact(&mut buf).unwrap();
		serde_json::from_slice(&buf).unwrap()
	}
}

#[test]
fn diagnostics_and_formatting() {
	let mut c = Client::start();
	let uri = "file:///t.jsonnet";

	c.send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}));
	let init = c.recv();
	assert_eq!(init["result"]["capabilities"]["documentFormattingProvider"], true);
	c.send(json!({"jsonrpc":"2.0","method":"initialized","params":{}}));

	let open = |text: &str, ver: i64, method: &str| {
		let doc = if method == "textDocument/didOpen" {
			json!({"uri":uri,"languageId":"jsonnet","version":ver,"text":text})
		} else {
			json!({"uri":uri,"version":ver})
		};
		let mut params = json!({"textDocument": doc});
		if method == "textDocument/didChange" {
			params["contentChanges"] = json!([{"text": text}]);
		}
		json!({"jsonrpc":"2.0","method":method,"params":params})
	};

	c.send(open("{a:1,\n b:[1,2}", 1, "textDocument/didOpen"));
	let diags = c.recv();
	assert_eq!(diags["method"], "textDocument/publishDiagnostics");
	assert!(!diags["params"]["diagnostics"].as_array().unwrap().is_empty());
	assert_eq!(diags["params"]["diagnostics"][0]["range"]["start"]["line"], 1);

	c.send(open("{a:1,b:  [1,2]}", 2, "textDocument/didChange"));
	let diags = c.recv();
	assert!(diags["params"]["diagnostics"].as_array().unwrap().is_empty());

	c.send(json!({"jsonrpc":"2.0","id":2,"method":"textDocument/formatting",
		"params":{"textDocument":{"uri":uri},"options":{"tabSize":2,"insertSpaces":true}}}));
	let edits = c.recv();
	let new_text = edits["result"][0]["newText"].as_str().unwrap();
	assert!(new_text.contains("a: 1"), "unexpected formatting: {new_text}");

	c.send(json!({"jsonrpc":"2.0","id":3,"method":"shutdown"}));
	c.recv();
	c.send(json!({"jsonrpc":"2.0","method":"exit"}));
	// The server joins its stdin reader thread, which needs EOF.
	drop(c.stdin);
	assert!(c.child.wait().unwrap().success());
}
