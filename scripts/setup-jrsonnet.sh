#!/bin/sh
# Prepares `.jrsonnet/`: jrsonnet at the rev pinned in Cargo.toml, plus the local patches in
# `patches/`. `.cargo/config.toml` overrides the git dependency with it. Run this once before
# the first `cargo build`, and again after bumping the rev or changing a patch.
set -eu

cd "$(dirname "$0")/.."

rev=$(sed -n 's/.*jrsonnet-rowan-parser = {.*rev = "\([0-9a-f]*\)".*/\1/p' Cargo.toml)
[ -n "$rev" ] || { echo "no jrsonnet rev found in Cargo.toml" >&2; exit 1; }

# A stamp of the rev and patch contents tells us whether `.jrsonnet` is already up to date.
stamp="$rev $(cat patches/*.patch | cksum)"
if [ -f .jrsonnet/.stamp ] && [ "$(cat .jrsonnet/.stamp)" = "$stamp" ]; then
	echo ".jrsonnet is up to date ($rev)"
	exit 0
fi

rm -rf .jrsonnet
mkdir .jrsonnet
git -C .jrsonnet init -q
git -C .jrsonnet fetch -q --depth 1 "${JRSONNET_URL:-https://github.com/deltarocks/jrsonnet}" "$rev"
git -C .jrsonnet checkout -q FETCH_HEAD
for p in patches/*.patch; do
	git -C .jrsonnet apply "../$p"
done
echo "$stamp" > .jrsonnet/.stamp
set -- patches/*.patch
echo "prepared .jrsonnet at $rev with $# patch(es)"
