#!/usr/bin/env bash
# Fetch liblc3 1.1.3 (Google, Apache-2.0) into the gitignored .cache/liblc3/: the C oracle for
# the LC3 port's differential tests and its speed baseline. SHA-256 pinned (when the pin was
# made, the tarball's embedded commit id, `git get-tar-commit-id`, was checked by hand against
# the v1.1.3 tag's commit 96a3af0b…; Debian repacks it as +dfsg, Arch builds from the tag).
# Idempotent.
set -euo pipefail
cd "$(dirname "$0")/.."

URL=https://github.com/google/liblc3/archive/refs/tags/v1.1.3.tar.gz
SHA=276752ff54ce6a77d54ec133397b9d7e71f90caf3d9afa32d8b0e891b8ecb8af
DIR=.cache/liblc3
TGZ=$DIR/liblc3-1.1.3.tar.gz

source scripts/fetch.sh
fetch "$URL" "$SHA" "$TGZ"
if [ ! -f "$DIR/.unpacked" ]; then
  rm -rf "$DIR/liblc3-1.1.3"
  tar xzf "$TGZ" -C "$DIR"
  touch "$DIR/.unpacked"
fi
echo "liblc3: ok ($DIR/liblc3-1.1.3; sha256 verified)"
