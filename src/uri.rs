//! `file://` URI <-> path conversion, including Windows drive-letter paths.
//!
//! Windows clients send `file:///c%3A/Users/me/x.jsonnet` (VS Code percent-encodes the drive
//! colon) or `file:///C:/Users/me/x.jsonnet`; both mean `C:\Users\me\x.jsonnet`. UNC paths
//! (`file://server/share/...`) are not supported.

use std::{
	path::{Path, PathBuf},
	str::FromStr,
};

use lsp_types::Uri;

pub fn to_path(uri: &Uri) -> Option<PathBuf> {
	path_from_uri(uri.as_str()).map(PathBuf::from)
}

pub fn from_path(path: &Path) -> Option<Uri> {
	Uri::from_str(&uri_from_path(path.to_str()?, cfg!(windows))).ok()
}

/// A string that is equal for paths that name the same file, to match a path against the URIs
/// of open documents. Windows paths are case-insensitive and take either separator.
pub fn path_key(path: &Path) -> String {
	key(&path.to_string_lossy(), cfg!(windows))
}

fn key(path: &str, windows: bool) -> String {
	if windows {
		path.replace('\\', "/").to_lowercase()
	} else {
		path.to_owned()
	}
}

/// Whether `s` starts with a drive letter and colon, such as `C:` or `c:/`.
fn has_drive(s: &str) -> bool {
	let b = s.as_bytes();
	b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b.len() == 2 || b[2] == b'/')
}

fn path_from_uri(uri: &str) -> Option<String> {
	let rest = uri.strip_prefix("file://")?;
	let rest = rest.strip_prefix("localhost").unwrap_or(rest);
	if !rest.starts_with('/') {
		return None;
	}
	let bytes = rest.as_bytes();
	let mut out = Vec::with_capacity(bytes.len());
	let mut i = 0;
	while i < bytes.len() {
		if bytes[i] == b'%' {
			let hex = rest.get(i + 1..i + 3)?;
			out.push(u8::from_str_radix(hex, 16).ok()?);
			i += 3;
		} else {
			out.push(bytes[i]);
			i += 1;
		}
	}
	let path = String::from_utf8(out).ok()?;
	// `/c:/Users/me` is `c:/Users/me`: the slash before a drive letter is part of the URI syntax.
	Some(match path.strip_prefix('/') {
		Some(no_slash) if has_drive(no_slash) => no_slash.to_owned(),
		_ => path,
	})
}

fn uri_from_path(path: &str, windows: bool) -> String {
	use std::fmt::Write as _;
	let path = if windows {
		path.replace('\\', "/")
	} else {
		path.to_owned()
	};
	let mut out = String::from("file://");
	if has_drive(&path) {
		out.push('/');
	}
	for (i, b) in path.bytes().enumerate() {
		// The colon after a drive letter stays readable.
		let drive_colon = i == 1 && has_drive(&path);
		if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) || drive_colon {
			out.push(b as char);
		} else {
			write!(out, "%{b:02X}").expect("writing to a String");
		}
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn round_trip() {
		let path = Path::new("/home/a b/ü/x.jsonnet");
		let uri = from_path(path).unwrap();
		assert_eq!(uri.as_str(), "file:///home/a%20b/%C3%BC/x.jsonnet");
		assert_eq!(to_path(&uri).unwrap(), path);
	}

	#[test]
	fn rejects_other_schemes_and_hosts() {
		assert_eq!(
			to_path(&Uri::from_str("untitled:Untitled-1").unwrap()),
			None
		);
		assert_eq!(path_from_uri("file://server/share/x.jsonnet"), None);
		assert_eq!(
			path_from_uri("file://localhost/home/x"),
			Some("/home/x".to_owned())
		);
	}

	#[test]
	fn windows_drive_paths_from_uris() {
		// VS Code encodes the colon and lowercases the drive; others don't.
		assert_eq!(
			path_from_uri("file:///c%3A/Users/me/a%20b.jsonnet").as_deref(),
			Some("c:/Users/me/a b.jsonnet")
		);
		assert_eq!(path_from_uri("file:///C:/x").as_deref(), Some("C:/x"));
		assert_eq!(path_from_uri("file:///C:").as_deref(), Some("C:"));
		// Not a drive: a Unix directory that happens to start with a letter and colon-less name.
		assert_eq!(path_from_uri("file:///c/x").as_deref(), Some("/c/x"));
		assert_eq!(path_from_uri("file:///cd:/x").as_deref(), Some("/cd:/x"));
	}

	#[test]
	fn windows_paths_to_uris() {
		assert_eq!(
			uri_from_path(r"C:\Users\me\a b.jsonnet", true),
			"file:///C:/Users/me/a%20b.jsonnet"
		);
		assert_eq!(uri_from_path("C:/x/ü", true), "file:///C:/x/%C3%BC");
		// Where backslash is an ordinary character, it is escaped and nothing is a drive.
		assert_eq!(uri_from_path(r"/a\b", false), "file:///a%5Cb");
		// A round trip on the URI side.
		let uri = uri_from_path(r"D:\w\k.libsonnet", true);
		assert_eq!(path_from_uri(&uri).as_deref(), Some("D:/w/k.libsonnet"));
	}

	#[test]
	fn path_keys_ignore_windows_spelling() {
		assert_eq!(
			key(r"C:\Users\Me\x.jsonnet", true),
			key("c:/users/me/x.jsonnet", true)
		);
		assert_ne!(key("/A/b", false), key("/a/b", false));
	}
}
