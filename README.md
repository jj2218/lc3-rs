# lc3-rs

Pure-Rust LC3, the Bluetooth LE Audio codec: encoder and decoder, bit-exact with liblc3.

```sh
cargo add lc3
```

| crate | |
|---|---|
| [`lc3`](crates/lc3) | LC3 (Bluetooth SIG LC3 1.0; liblc3's 2.5/5 ms too): encoder, decoder, packet loss concealment, 8–48 kHz, 16-bit or float PCM. Bit-exact with liblc3 on the SIG conformance operating points; faster than liblc3 as distributions build it at 48 kHz. [README: example, speed, scope, patents](crates/lc3/README.md) |
| `liblc3-sys` | test-only (not published): liblc3 1.1.3 as the oracle (and the speed baseline), compiled from a pinned download |

No dependencies. Measured results, each with the command that reproduces it:
[`docs/RESULTS.md`](docs/RESULTS.md). **Patents:** see the crate README; LC3 is patented, and
this crate's licence grants no third-party patent rights.

## Development
Needs a stable Rust toolchain (pinned in `rust-toolchain.toml`, installed by rustup), [`just`](https://github.com/casey/just),
a C compiler (GCC for the speed baseline), `nm`/`objcopy` (binutils), `curl`, `unzip` and
`python3`.

```sh
just check     # the full gate: guard, fmt, clippy, docs, tests against liblc3
just conform   # bit-exactness vs liblc3 on the SIG operating points, one summary line each
just bench     # µs per frame, Rust vs liblc3 (distribution and exact builds)
just --list    # everything else (fuzz, perf, ...)
```

**Oracles.** The tests compare against liblc3 1.1.3 (Apache-2.0), built without `-ffast-math`
so its output is the same on every CPU, and use the ETSI LC3plus test items
(`thetest*.wav`, TS 103 634) as input audio. `just check` downloads both on first use into the
gitignored `.cache/` and verifies each against a pinned SHA-256. The ETSI items are used as test
input only, never committed. The codec is ported from liblc3.

**CI** runs what builds without the downloads; run `just check` locally before a pull request.

**Releases** are published from a private development repository, one commit per release.
Pull requests are welcome: an accepted change is applied there with the same paths, credited
with `Co-authored-by`, and ships with the next release; the pull request is then closed with a
link to it.

## License
Apache-2.0 ([LICENSE](LICENSE)); attribution in [`crates/lc3/NOTICE`](crates/lc3/NOTICE).
