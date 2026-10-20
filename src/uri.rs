//! `file://` URI <-> path conversion (Unix paths only).

use std::{
	path::{Path, PathBuf},
	str::FromStr,
};

use lsp_types::Uri;

pub fn to_path(uri: &Uri) -> Option<PathBuf> {
	let rest = uri.as_str().strip_prefix("file://")?;
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
	Some(PathBuf::from(String::from_utf8(out).ok()?))
}

pub fn from_path(path: &Path) -> Option<Uri> {
	use std::fmt::Write as _;
	let mut out = String::from("file://");
	for b in path.to_str()?.bytes() {
		if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
			out.push(b as char);
		} else {
			write!(out, "%{b:02X}").ok()?;
		}
	}
	Uri::from_str(&out).ok()
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
	fn rejects_other_schemes() {
		assert_eq!(to_path(&Uri::from_str("untitled:Untitled-1").unwrap()), None);
	}
}
