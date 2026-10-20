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

impl Client {
	fn initialize(&mut self) {
		self.send(json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"capabilities":{}}}));
		self.recv();
		self.send(json!({"jsonrpc":"2.0","method":"initialized","params":{}}));
	}

	/// Opens a document and returns its published diagnostics.
	fn open(&mut self, uri: &str, text: &str) -> Value {
		self.send(json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{
			"textDocument":{"uri":uri,"languageId":"jsonnet","version":1,"text":text}}}));
		self.recv()["params"]["diagnostics"].clone()
	}

	fn request(&mut self, id: i64, method: &str, params: Value) -> Value {
		self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
		let resp = self.recv();
		assert_eq!(resp["id"], id);
		resp
	}

	fn finish(mut self) {
		self.send(json!({"jsonrpc":"2.0","id":99,"method":"shutdown"}));
		self.recv();
		self.send(json!({"jsonrpc":"2.0","method":"exit"}));
		drop(self.stdin);
		assert!(self.child.wait().unwrap().success());
	}
}

#[test]
fn navigation_and_rename() {
	let mut c = Client::start();
	c.initialize();
	let uri = "file:///t.jsonnet";
	// The string has a 2-byte and a 4-byte char (1 and 2 UTF-16 units); columns must line up.
	let diags = c.open(uri, "local s = \"é😀\"; local a = 1;\nlocal f(x) = x + a;\nf(a) + nope");
	let diags = diags.as_array().unwrap();
	assert_eq!(diags.len(), 1);
	assert_eq!(diags[0]["message"], "undefined variable `nope`");
	assert_eq!(diags[0]["range"]["start"], json!({"line":2,"character":7}));

	let at = |line, character| json!({"textDocument":{"uri":uri},"position":{"line":line,"character":character}});

	// Definition from the use in `f(é)` goes to the `local a` declaration.
	let def = c.request(1, "textDocument/definition", at(2, 2));
	assert_eq!(def["result"]["range"]["start"], json!({"line":0,"character":23}));

	let mut refs_params = at(0, 23);
	refs_params["context"] = json!({"includeDeclaration": true});
	let refs = c.request(2, "textDocument/references", refs_params);
	assert_eq!(refs["result"].as_array().unwrap().len(), 3);

	let prep = c.request(3, "textDocument/prepareRename", at(2, 2));
	assert_eq!(prep["result"]["start"], json!({"line":2,"character":2}));
	// `std` and unresolved names can't be renamed.
	assert!(c.request(4, "textDocument/prepareRename", at(2, 7))["result"].is_null());

	let mut rename = at(0, 23);
	rename["newName"] = json!("val");
	let edits = c.request(5, "textDocument/rename", rename.clone());
	assert_eq!(edits["result"]["changes"][uri].as_array().unwrap().len(), 3);

	rename["newName"] = json!("local");
	let err = c.request(6, "textDocument/rename", rename);
	assert!(err["error"]["message"].as_str().unwrap().contains("not a valid identifier"));

	c.finish();
}

#[test]
fn imports() {
	let ws = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("imports-ws");
	let _ = std::fs::remove_dir_all(&ws);
	for f in ["main.jsonnet", "sibling.libsonnet", "lib/in-lib.libsonnet", "vendor/in-vendor.libsonnet", "extra/in-jpath.libsonnet"] {
		let p = ws.join(f);
		std::fs::create_dir_all(p.parent().unwrap()).unwrap();
		std::fs::write(p, "{}").unwrap();
	}
	let uri_of = |rel: &str| format!("file://{}", ws.join(rel).display());

	let mut c = Client::start();
	c.send(json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{
		"capabilities":{},
		"workspaceFolders":[{"uri":uri_of(""),"name":"ws"}],
		"initializationOptions":{"jpath":["extra"]},
	}}));
	c.recv();
	c.send(json!({"jsonrpc":"2.0","method":"initialized","params":{}}));

	let main = uri_of("main.jsonnet");
	let src = r#"[import "sibling.libsonnet", import "in-lib.libsonnet", importstr "in-vendor.libsonnet", import "in-jpath.libsonnet", import "missing.libsonnet"]"#;
	let diags = c.open(&main, src);
	let diags = diags.as_array().unwrap();
	assert_eq!(diags.len(), 1, "{diags:?}");
	assert_eq!(diags[0]["severity"], 2);
	assert!(diags[0]["message"].as_str().unwrap().contains("missing.libsonnet"));

	let links = c.request(1, "textDocument/documentLink", json!({"textDocument":{"uri":main}}));
	let links = links["result"].as_array().unwrap();
	assert_eq!(links.len(), 4);
	assert_eq!(links[0]["target"], uri_of("sibling.libsonnet"));
	assert_eq!(links[2]["target"], uri_of("vendor/in-vendor.libsonnet"));

	// Column 17 is inside the first path string.
	let def = c.request(2, "textDocument/definition", json!({
		"textDocument":{"uri":main},"position":{"line":0,"character":17}}));
	assert_eq!(def["result"]["uri"], uri_of("sibling.libsonnet"));
	assert_eq!(def["result"]["range"]["start"], json!({"line":0,"character":0}));

	c.finish();
}
