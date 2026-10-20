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
	assert_eq!(
		init["result"]["capabilities"]["documentFormattingProvider"],
		true
	);
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
	assert!(
		!diags["params"]["diagnostics"]
			.as_array()
			.unwrap()
			.is_empty()
	);
	assert_eq!(
		diags["params"]["diagnostics"][0]["range"]["start"]["line"],
		1
	);

	c.send(open("{a:1,b:  [1,2]}", 2, "textDocument/didChange"));
	let diags = c.recv();
	assert!(
		diags["params"]["diagnostics"]
			.as_array()
			.unwrap()
			.is_empty()
	);

	c.send(
		json!({"jsonrpc":"2.0","id":2,"method":"textDocument/formatting",
		"params":{"textDocument":{"uri":uri},"options":{"tabSize":2,"insertSpaces":true}}}),
	);
	let edits = c.recv();
	let new_text = edits["result"][0]["newText"].as_str().unwrap();
	assert!(
		new_text.contains("a: 1"),
		"unexpected formatting: {new_text}"
	);

	c.send(json!({"jsonrpc":"2.0","id":3,"method":"shutdown"}));
	c.recv();
	c.send(json!({"jsonrpc":"2.0","method":"exit"}));
	// The server joins its stdin reader thread, which needs EOF.
	drop(c.stdin);
	assert!(c.child.wait().unwrap().success());
}

impl Client {
	fn initialize(&mut self) {
		self.send(
			json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"capabilities":{}}}),
		);
		self.recv();
		self.send(json!({"jsonrpc":"2.0","method":"initialized","params":{}}));
	}

	/// Opens a document and returns its published diagnostics.
	fn open(&mut self, uri: &str, text: &str) -> Value {
		self.send(
			json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{
			"textDocument":{"uri":uri,"languageId":"jsonnet","version":1,"text":text}}}),
		);
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
	let diags = c.open(
		uri,
		"local s = \"é😀\"; local a = 1;\nlocal f(x) = x + a;\nf(a) + nope",
	);
	let diags = diags.as_array().unwrap();
	assert_eq!(diags.len(), 1);
	assert_eq!(diags[0]["message"], "undefined variable `nope`");
	assert_eq!(diags[0]["range"]["start"], json!({"line":2,"character":7}));

	let at = |line, character| json!({"textDocument":{"uri":uri},"position":{"line":line,"character":character}});

	// Definition from the use in `f(é)` goes to the `local a` declaration.
	let def = c.request(1, "textDocument/definition", at(2, 2));
	assert_eq!(
		def["result"]["range"]["start"],
		json!({"line":0,"character":23})
	);

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
	assert!(
		err["error"]["message"]
			.as_str()
			.unwrap()
			.contains("not a valid identifier")
	);

	c.finish();
}

#[test]
fn imports() {
	let ws = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("imports-ws");
	let _ = std::fs::remove_dir_all(&ws);
	for f in [
		"main.jsonnet",
		"sibling.libsonnet",
		"lib/in-lib.libsonnet",
		"vendor/in-vendor.libsonnet",
		"extra/in-jpath.libsonnet",
	] {
		let p = ws.join(f);
		std::fs::create_dir_all(p.parent().unwrap()).unwrap();
		std::fs::write(p, "{}").unwrap();
	}
	let uri_of = |rel: &str| format!("file://{}", ws.join(rel).display());

	let mut c = Client::start();
	c.send(
		json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{
			"capabilities":{},
			"workspaceFolders":[{"uri":uri_of(""),"name":"ws"}],
			"initializationOptions":{"jpath":["extra"]},
		}}),
	);
	c.recv();
	c.send(json!({"jsonrpc":"2.0","method":"initialized","params":{}}));

	let main = uri_of("main.jsonnet");
	let src = r#"[import "sibling.libsonnet", import "in-lib.libsonnet", importstr "in-vendor.libsonnet", import "in-jpath.libsonnet", import "missing.libsonnet"]"#;
	let diags = c.open(&main, src);
	let diags = diags.as_array().unwrap();
	assert_eq!(diags.len(), 1, "{diags:?}");
	assert_eq!(diags[0]["severity"], 2);
	assert!(
		diags[0]["message"]
			.as_str()
			.unwrap()
			.contains("missing.libsonnet")
	);

	let links = c.request(
		1,
		"textDocument/documentLink",
		json!({"textDocument":{"uri":main}}),
	);
	let links = links["result"].as_array().unwrap();
	assert_eq!(links.len(), 4);
	assert_eq!(links[0]["target"], uri_of("sibling.libsonnet"));
	assert_eq!(links[2]["target"], uri_of("vendor/in-vendor.libsonnet"));

	// Column 17 is inside the first path string.
	let def = c.request(
		2,
		"textDocument/definition",
		json!({
		"textDocument":{"uri":main},"position":{"line":0,"character":17}}),
	);
	assert_eq!(def["result"]["uri"], uri_of("sibling.libsonnet"));
	assert_eq!(
		def["result"]["range"]["start"],
		json!({"line":0,"character":0})
	);

	c.finish();
}

#[test]
fn field_navigation_and_completion() {
	let ws = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("fields-ws");
	let _ = std::fs::remove_dir_all(&ws);
	std::fs::create_dir_all(&ws).unwrap();
	// `lib.libsonnet` has a local in front of the object, and a nested object.
	std::fs::write(
		ws.join("lib.libsonnet"),
		"local h = 1;\n{\n  alpha: h,\n  nested: { deep: 1 },\n  method(x):: x,\n  'quoted-key': 2,\n  [\"dyn\" + \"amic\"]: 3,\n}\n",
	)
	.unwrap();
	let uri_of = |rel: &str| format!("file://{}", ws.join(rel).display());

	let mut c = Client::start();
	c.send(
		json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{
		"capabilities":{}, "workspaceFolders":[{"uri":uri_of(""),"name":"ws"}]}}),
	);
	c.recv();
	c.send(json!({"jsonrpc":"2.0","method":"initialized","params":{}}));

	let main = uri_of("main.jsonnet");
	let src = "local lib = import 'lib.libsonnet';\nlocal obj = ({ own: 1 } + lib);\n[lib.alpha, lib.nested.deep, obj.own, obj.alpha, lib.method, lib.]";
	let diags = c.open(&main, src);
	// Only the trailing `lib.` is a syntax error.
	assert!(!diags.as_array().unwrap().is_empty());

	let at = |line, character| json!({"textDocument":{"uri":main},"position":{"line":line,"character":character}});
	let def = |c: &mut Client, id, line, character| {
		let r = c.request(id, "textDocument/definition", at(line, character));
		r["result"].clone()
	};

	// `lib.alpha` in main (line 2) goes to `alpha`, declared on line 2 of lib.
	let d = def(&mut c, 1, 2, 6);
	assert_eq!(d["uri"], uri_of("lib.libsonnet"));
	assert_eq!(d["range"]["start"], json!({"line":2,"character":2}));

	// Chained: `lib.nested.deep` -> the `deep` in the nested object.
	let d = def(&mut c, 2, 2, 25);
	assert_eq!(d["uri"], uri_of("lib.libsonnet"));
	assert_eq!(d["range"]["start"], json!({"line":3,"character":12}));

	// Through `+` and parentheses: own field stays local, the rest comes from lib.
	let d = def(&mut c, 3, 2, 34);
	assert_eq!(d["uri"], main);
	let d = def(&mut c, 4, 2, 43);
	assert_eq!(d["uri"], uri_of("lib.libsonnet"));

	// Methods are found, too.
	let d = def(&mut c, 5, 2, 57);
	assert_eq!(d["range"]["start"], json!({"line":4,"character":2}));

	// Completion after the dangling `lib.`: fixed names only, computed `dyn...` skipped.
	let items = c.request(6, "textDocument/completion", at(2, 65));
	let mut labels: Vec<_> = items["result"]
		.as_array()
		.unwrap()
		.iter()
		.map(|i| i["label"].as_str().unwrap().to_owned())
		.collect();
	labels.sort();
	assert_eq!(labels, ["alpha", "method", "nested", "quoted-key"]);

	c.finish();
}

#[test]
fn symbols_and_scope_completion() {
	let mut c = Client::start();
	c.initialize();

	let uri = "file:///sym.jsonnet";
	c.open(
		uri,
		"local a = 1;\nlocal f(x) = x;\n{\n  top: 1,\n  nested: { inner: 2 },\n  m(y):: y,\n}\n",
	);
	let syms = c.request(
		1,
		"textDocument/documentSymbol",
		json!({"textDocument":{"uri":uri}}),
	);
	let syms = syms["result"].as_array().unwrap();
	let names: Vec<_> = syms
		.iter()
		.map(|s| (s["name"].as_str().unwrap(), s["kind"].as_i64().unwrap()))
		.collect();
	// LSP kinds: 13 variable, 12 function, 8 field, 19 object, 6 method.
	assert_eq!(
		names,
		[("a", 13), ("f", 12), ("top", 8), ("nested", 19), ("m", 6)]
	);
	assert_eq!(syms[3]["children"][0]["name"], "inner");
	assert_eq!(syms[3]["children"][0]["range"]["start"]["line"], 4);

	let labels = |r: &Value| -> Vec<String> {
		let mut v: Vec<_> = r["result"]
			.as_array()
			.unwrap()
			.iter()
			.map(|i| i["label"].as_str().unwrap().to_owned())
			.collect();
		v.sort();
		v
	};
	let at = |uri: &str, line, character| json!({"textDocument":{"uri":uri},"position":{"line":line,"character":character}});

	// `std.` lists the standard library.
	let u2 = "file:///std.jsonnet";
	c.open(u2, "std.");
	let r = c.request(2, "textDocument/completion", at(u2, 0, 4));
	let l = labels(&r);
	assert!(l.contains(&"length".to_owned()) && l.contains(&"manifestJson".to_owned()));
	assert!(!l.iter().any(|n| n.starts_with("__")));

	// A local named `std` is not the standard library.
	let u3 = "file:///shadow.jsonnet";
	c.open(u3, "local std = { mine: 1 }; std.");
	let r = c.request(3, "textDocument/completion", at(u3, 0, 29));
	assert_eq!(labels(&r), ["mine"]);

	// Plain completion offers names in scope at the cursor (a `local` ends at the comma after its
	// body; a comprehension variable is visible in its head, not in its own iterable), no others.
	let u4 = "file:///scope.jsonnet";
	c.open(
		u4,
		"local outer = 1;\n[local inner = 2; inner, [x for loopvar in [1]], \n]",
	);
	let r = c.request(4, "textDocument/completion", at(u4, 1, 22));
	assert_eq!(labels(&r), ["inner", "outer", "std"]);
	let r = c.request(5, "textDocument/completion", at(u4, 1, 27));
	assert_eq!(labels(&r), ["loopvar", "outer", "std"]);
	let r = c.request(6, "textDocument/completion", at(u4, 2, 0));
	assert_eq!(labels(&r), ["outer", "std"]);

	c.finish();
}

#[test]
fn self_super_and_dollar() {
	let mut c = Client::start();
	c.initialize();
	let uri = "file:///keywords.jsonnet";
	c.open(uri, "{\n  a: 1,\n  b: self.a,\n  nested: { c: $.a, z: 1, d: self.z },\n} + {\n  e: super.a,\n  f: self.b,\n}\n");
	let mut id = 0;
	let mut def = |line, character| {
		id += 1;
		let r = c.request(
			id,
			"textDocument/definition",
			json!({
			"textDocument":{"uri":uri},"position":{"line":line,"character":character}}),
		);
		r["result"]["range"]["start"].clone()
	};
	// `self.a` inside the first object.
	assert_eq!(def(2, 10), json!({"line":1,"character":2}));
	// `$` is the outermost object even from a nested one.
	assert_eq!(def(3, 17), json!({"line":1,"character":2}));
	// `self` is the innermost object.
	assert_eq!(def(3, 34), json!({"line":3,"character":20}));
	// `super` in the right operand of `+` is the left operand.
	assert_eq!(def(5, 11), json!({"line":1,"character":2}));
	// `self` there also sees what it extends.
	assert_eq!(def(6, 10), json!({"line":2,"character":2}));
	c.finish();
}

#[test]
fn hover() {
	let mut c = Client::start();
	c.initialize();
	let uri = "file:///hover.jsonnet";
	c.open(uri, "local lib = { foo: 1, bar(x):: x };\nlocal f(a, b) = a + b;\n[lib.foo, lib.bar, f, std.length, std, nope]");
	let mut id = 0;
	let mut hover = |line, character| {
		id += 1;
		let r = c.request(
			id,
			"textDocument/hover",
			json!({
			"textDocument":{"uri":uri},"position":{"line":line,"character":character}}),
		);
		r["result"]["contents"]["value"].as_str().map(str::to_owned)
	};
	assert!(hover(2, 19).unwrap().contains("local f(a, b) = a + b"));
	assert!(hover(1, 16).unwrap().contains("(parameter) a"));
	assert!(hover(2, 6).unwrap().contains("foo: 1"));
	assert!(hover(2, 15).unwrap().contains("bar(x):: x"));
	assert!(hover(2, 28).unwrap().contains("std.length"));
	assert!(hover(2, 35).unwrap().contains("standard library"));
	// Undefined names have nothing to show.
	assert_eq!(hover(2, 41), None);
	c.finish();
}

#[test]
fn import_diagnostics_refresh_on_file_changes() {
	let ws = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("watch-ws");
	let _ = std::fs::remove_dir_all(&ws);
	std::fs::create_dir_all(&ws).unwrap();
	let uri_of = |rel: &str| format!("file://{}", ws.join(rel).display());

	let mut c = Client::start();
	c.send(
		json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{
		"capabilities":{"workspace":{"didChangeWatchedFiles":{"dynamicRegistration":true}}},
		"workspaceFolders":[{"uri":uri_of(""),"name":"ws"}]}}),
	);
	c.recv();
	c.send(json!({"jsonrpc":"2.0","method":"initialized","params":{}}));

	// The server asks to watch Jsonnet files.
	let reg = c.recv();
	assert_eq!(reg["method"], "client/registerCapability");
	let registration = &reg["params"]["registrations"][0];
	assert_eq!(registration["method"], "workspace/didChangeWatchedFiles");
	assert_eq!(
		registration["registerOptions"]["watchers"][0]["globPattern"],
		"**/*.{jsonnet,libsonnet}"
	);
	c.send(json!({"jsonrpc":"2.0","id":reg["id"],"result":null}));

	let main = uri_of("main.jsonnet");
	let diags = c.open(&main, "import 'late.libsonnet'");
	assert_eq!(diags.as_array().unwrap().len(), 1);

	std::fs::write(ws.join("late.libsonnet"), "{}").unwrap();
	c.send(
		json!({"jsonrpc":"2.0","method":"workspace/didChangeWatchedFiles","params":{
		"changes":[{"uri":uri_of("late.libsonnet"),"type":1}]}}),
	);
	let refreshed = c.recv();
	assert_eq!(refreshed["method"], "textDocument/publishDiagnostics");
	assert_eq!(refreshed["params"]["uri"], main);
	assert!(
		refreshed["params"]["diagnostics"]
			.as_array()
			.unwrap()
			.is_empty()
	);

	c.finish();
}

#[test]
fn fields_through_destructuring() {
	let mut c = Client::start();
	c.initialize();
	let uri = "file:///destructure.jsonnet";
	let line0 = "local {foo, nested: {deep}, alias: al} = { foo: { bar: 1 }, nested: { deep: { baz: 2 } }, alias: { qux: 3 } };";
	let line1 = "[foo.bar, deep.baz, al.qux, foo.]";
	c.open(uri, &format!("{line0}\n{line1}"));
	let col = |line: &str, needle: &str| line.find(needle).unwrap() as u64;

	let mut id = 0;
	let mut def = |c: &mut Client, character: u64| {
		id += 1;
		let r = c.request(
			id,
			"textDocument/definition",
			json!({
			"textDocument":{"uri":uri},"position":{"line":1,"character":character}}),
		);
		r["result"]["range"]["start"].clone()
	};
	// Each lookup lands on the field declared inside the object literal on line 0.
	for (use_, decl) in [(".bar", "bar:"), (".baz", "baz:"), (".qux", "qux:")] {
		let got = def(&mut c, col(line1, use_) + 2);
		assert_eq!(
			got,
			json!({"line":0,"character":col(line0, decl)}),
			"for {use_}"
		);
	}

	// Completion sees through the pattern as well.
	let items = c.request(
		99,
		"textDocument/completion",
		json!({
		"textDocument":{"uri":uri},"position":{"line":1,"character":col(line1, "foo.]") + 4}}),
	);
	let labels: Vec<_> = items["result"]
		.as_array()
		.unwrap()
		.iter()
		.map(|i| i["label"].as_str().unwrap())
		.collect();
	assert_eq!(labels, ["bar"]);
	c.finish();
}

#[test]
fn signature_help() {
	let mut c = Client::start();
	c.initialize();
	let uri = "file:///sig.jsonnet";
	let l0 = "local f(a, b = 2) = a;";
	let l1 = "local lib = { m(x, y):: x, g: function(p, q) p };";
	let l2 = "f(1, 2) + lib.m(1, 2) + lib.g(1, 2) + f(b = 5, a = 1) + f(1,";
	c.open(uri, &format!("{l0}\n{l1}\n{l2}"));
	let col = |needle: &str| l2.find(needle).unwrap() as u64;

	let mut id = 0;
	let mut sig = |c: &mut Client, character: u64| {
		id += 1;
		let r = c.request(
			id,
			"textDocument/signatureHelp",
			json!({
			"textDocument":{"uri":uri},"position":{"line":2,"character":character}}),
		);
		r["result"].clone()
	};

	// A local function: right after `(`, then after the comma.
	let r = sig(&mut c, col("f(1, 2)") + 2);
	assert_eq!(r["signatures"][0]["label"], "f(a, b = 2)");
	assert_eq!(r["signatures"][0]["parameters"][0]["label"], json!([2, 3]));
	assert_eq!(r["signatures"][0]["parameters"][1]["label"], json!([5, 10]));
	assert_eq!(r["activeParameter"], 0);
	assert_eq!(sig(&mut c, col("1, 2)") + 3)["activeParameter"], 1);

	// Fields: a method, and a field whose value is a function.
	let r = sig(&mut c, col("m(1") + 2);
	assert_eq!(r["signatures"][0]["label"], "m(x, y)");
	let r = sig(&mut c, col("g(1") + 2);
	assert_eq!(r["signatures"][0]["label"], "g(p, q)");

	// Named arguments pick their parameter, whatever the position.
	assert_eq!(sig(&mut c, col("5, a") + 1)["activeParameter"], 1);
	assert_eq!(sig(&mut c, col("1)") + 1)["activeParameter"], 0);

	// An unterminated call still works, with the cursor after the last comma.
	assert_eq!(sig(&mut c, l2.len() as u64)["activeParameter"], 1);

	// Not inside a call: on the operator, and right after the closing paren.
	assert!(sig(&mut c, col(" + lib") + 1).is_null());
	assert!(sig(&mut c, col("f(1, 2)") + 7).is_null());
	c.finish();
}

#[test]
fn std_signatures() {
	let mut c = Client::start();
	c.initialize();
	let uri = "file:///stdsig.jsonnet";
	let src = "std.avg([1], 2) + std.base64Decode('x', lossy = true) + std.nope(1)";
	c.open(uri, src);
	let col = |needle: &str| src.find(needle).unwrap() as u64;

	let mut id = 0;
	let mut req = |c: &mut Client, uri: &str, method: &str, character: u64| {
		id += 1;
		let params =
			json!({"textDocument":{"uri":uri},"position":{"line":0,"character":character}});
		c.request(id, method, params)["result"].clone()
	};
	const SIG: &str = "textDocument/signatureHelp";

	let r = req(&mut c, uri, SIG, col("avg(") + 4);
	assert_eq!(r["signatures"][0]["label"], "std.avg(arr, onEmpty = …)");
	assert_eq!(r["activeParameter"], 0);
	assert_eq!(req(&mut c, uri, SIG, col("2)"))["activeParameter"], 1);

	// A named argument selects its parameter, even with a default shown after the name.
	let r = req(&mut c, uri, SIG, col("true"));
	assert_eq!(
		r["signatures"][0]["label"],
		"std.base64Decode(str, lossy = false)"
	);
	assert_eq!(r["activeParameter"], 1);

	// Unknown members have no signature.
	assert!(req(&mut c, uri, SIG, col("nope(") + 5).is_null());

	// Hover shows the parameters.
	let h = req(&mut c, uri, "textDocument/hover", col("avg") + 1);
	assert!(
		h["contents"]["value"]
			.as_str()
			.unwrap()
			.contains("std.avg(arr, onEmpty = …)")
	);

	// A local named `std` is not the standard library.
	let shadow = "file:///stdshadow.jsonnet";
	let src = "local std = { avg(q):: q }; std.avg(1)";
	c.open(shadow, src);
	let r = req(&mut c, shadow, SIG, src.find("avg(1").unwrap() as u64 + 4);
	assert_eq!(r["signatures"][0]["label"], "avg(q)");

	// Completion carries the parameters, and marks plain values as constants.
	let comp = "file:///stdcomp.jsonnet";
	c.open(comp, "std.");
	let items = req(&mut c, comp, "textDocument/completion", 4);
	let items = items.as_array().unwrap();
	let avg = items.iter().find(|i| i["label"] == "avg").unwrap();
	assert_eq!(avg["detail"], "(arr, onEmpty = …)");
	let pi = items.iter().find(|i| i["label"] == "pi").unwrap();
	assert_eq!(pi["kind"], 21); // CompletionItemKind::CONSTANT
	assert!(pi["detail"].is_null());
	c.finish();
}

#[test]
fn incremental_sync() {
	let mut c = Client::start();
	c.send(json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"capabilities":{}}}));
	let init = c.recv();
	// 2 is TextDocumentSyncKind::Incremental.
	assert_eq!(init["result"]["capabilities"]["textDocumentSync"], 2);
	c.send(json!({"jsonrpc":"2.0","method":"initialized","params":{}}));

	let uri = "file:///inc.jsonnet";
	assert!(
		c.open(uri, "local a = 1;\na")
			.as_array()
			.unwrap()
			.is_empty()
	);

	let change = |c: &mut Client, edits: Value| {
		c.send(
			json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{
			"textDocument":{"uri":uri,"version":2},"contentChanges":edits}}),
		);
		c.recv()["params"]["diagnostics"].clone()
	};
	let range = |l1, c1, l2, c2| json!({"start":{"line":l1,"character":c1},"end":{"line":l2,"character":c2}});

	// Two edits in one notification; the second one's range refers to the text after the first.
	let diags = change(
		&mut c,
		json!([
			{"range": range(0, 10, 0, 11), "text": "2000"},
			{"range": range(1, 0, 1, 1), "text": "b"},
		]),
	);
	let diags = diags.as_array().unwrap();
	assert_eq!(diags.len(), 1);
	assert_eq!(diags[0]["message"], "undefined variable `b`");
	assert_eq!(diags[0]["range"]["start"], json!({"line":1,"character":0}));

	// Fixing it clears the diagnostic, and the earlier edit is still there.
	let diags = change(&mut c, json!([{"range": range(1, 0, 1, 1), "text": "a"}]));
	assert!(diags.as_array().unwrap().is_empty());
	let hover = c.request(
		1,
		"textDocument/hover",
		json!({
		"textDocument":{"uri":uri},"position":{"line":1,"character":0}}),
	);
	assert!(
		hover["result"]["contents"]["value"]
			.as_str()
			.unwrap()
			.contains("local a = 2000")
	);

	// A change without a range still replaces the whole text.
	let diags = change(&mut c, json!([{"text": "nope"}]));
	assert_eq!(diags[0]["message"], "undefined variable `nope`");
	c.finish();
}

#[test]
fn fields_through_indexing_and_array_destructuring() {
	let mut c = Client::start();
	c.initialize();
	let uri = "file:///index.jsonnet";
	let lines = [
		"local lib = { a: { x: 1 }, 'b-c': { y: 2 }, list: [{ p: 1 }, { q: 2 }] };",
		"local [first, second] = lib.list;",
		"local [h, ...rest] = [{ m: 1 }, { n: 2 }];",
		"[lib[\"a\"].x, lib?.[\"b-c\"].y, lib.list[1].q, first.p, second.q, h.m, rest.zzz]",
	];
	c.open(uri, &lines.join("\n"));

	let col = |line: usize, needle: &str| lines[line].find(needle).unwrap() as u64;
	let mut id = 0;
	let mut def = |c: &mut Client, character: u64| {
		id += 1;
		let r = c.request(
			id,
			"textDocument/definition",
			json!({
			"textDocument":{"uri":uri},"position":{"line":3,"character":character}}),
		);
		r["result"].clone()
	};
	let at = |line: u64, character: u64| json!({"line":line,"character":character});

	let cases = [
		// (cursor in line 3, declaration line, declaration needle)
		(col(3, ".x") + 1, 0, "x:"),
		(col(3, ".y") + 1, 0, "y:"),
		(col(3, "].q") + 2, 0, "q:"),
		(col(3, "first.p") + 6, 0, "p:"),
		(col(3, "second.q") + 7, 0, "q:"),
		(col(3, "h.m") + 2, 2, "m:"),
		// The string in `lib["a"]` itself goes to the field it names.
		(col(3, "\"a\"") + 1, 0, "a:"),
	];
	for (cursor, line, needle) in cases {
		let got = def(&mut c, cursor);
		assert_eq!(
			got["range"]["start"],
			at(line as u64, col(line, needle)),
			"for {needle}"
		);
	}
	// Elements after a `...rest` can't be told apart statically, and `rest` is not an object.
	assert!(def(&mut c, col(3, "rest.zzz") + 6).is_null());
	c.finish();
}

#[test]
fn open_buffers_win_over_disk_for_imports() {
	let ws = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("buffers-ws");
	let _ = std::fs::remove_dir_all(&ws);
	std::fs::create_dir_all(&ws).unwrap();
	std::fs::write(ws.join("lib.libsonnet"), "{ old: 1 }").unwrap();
	let uri_of = |rel: &str| format!("file://{}", ws.join(rel).display());

	let mut c = Client::start();
	c.send(
		json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{
		"capabilities":{}, "workspaceFolders":[{"uri":uri_of(""),"name":"ws"}]}}),
	);
	c.recv();
	c.send(json!({"jsonrpc":"2.0","method":"initialized","params":{}}));

	let main = uri_of("main.jsonnet");
	let src = "[(import 'lib.libsonnet').old, (import 'lib.libsonnet').fresh]";
	c.open(&main, src);
	let col = |needle: &str| src.find(needle).unwrap() as u64;
	let mut id = 0;
	let mut def = |c: &mut Client, character: u64| {
		id += 1;
		c.request(
			id,
			"textDocument/definition",
			json!({
			"textDocument":{"uri":main},"position":{"line":0,"character":character}}),
		)["result"]
			.clone()
	};

	// From disk, only `old` exists.
	assert_eq!(def(&mut c, col(".old") + 1)["uri"], uri_of("lib.libsonnet"));
	assert!(def(&mut c, col(".fresh") + 1).is_null());

	// The unsaved buffer replaces what is on disk.
	c.open(&uri_of("lib.libsonnet"), "{ fresh: 1 }");
	assert!(def(&mut c, col(".old") + 1).is_null());
	let got = def(&mut c, col(".fresh") + 1);
	assert_eq!(got["uri"], uri_of("lib.libsonnet"));
	assert_eq!(got["range"]["start"], json!({"line":0,"character":2}));
	c.finish();
}
