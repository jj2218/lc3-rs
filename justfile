# Every recurring task is a recipe here, backed by a script when it has logic: `just --list`.
# The recipes below `rules` are the development repository's, unchanged.

default:
    @just --list

# The full gate: export guard, format, lint, tests against the oracle (downloaded into .cache/)
check:
    @bash scripts/check.sh

# Export guard: no third-party source, test items or stray files in the repo
rules:
    @bash scripts/export-guard.sh

# The published codec crates present in this checkout, as `-p` arguments: recipe bodies stay the
# same in every product (a product's justfile copies them; doctor compares them).
export CODECS := `for c in amr-nb amr-wb lc3; do [ -d crates/$c ] && printf -- '-p %s ' $c; done; true`

fmt-check:
    cargo fmt --all --check
    for m in fuzz/*/Cargo.toml; do cargo fmt --manifest-path "$m" --check; done

lint: liblc3 msrv
    cargo clippy --workspace --all-targets --locked -- -D warnings
    RUSTFLAGS="--cfg portable" CARGO_TARGET_DIR=target/portable cargo clippy $CODECS --all-targets --locked -- -D warnings
    for m in fuzz/*/Cargo.toml; do cargo clippy --manifest-path "$m" --locked -- -D warnings; done
    cargo clippy -q $CODECS --lib --locked --target aarch64-unknown-linux-gnu -- -D warnings
    RUSTDOCFLAGS="-D warnings" cargo doc -q $CODECS --no-deps --locked

# The library builds with its minimum supported Rust (`rust-version` in Cargo.toml; installs it)
msrv:
    #!/usr/bin/env bash
    set -euo pipefail
    v=$(sed -n 's/^rust-version = "\(.*\)"$/\1/p' Cargo.toml)
    rustup toolchain install "$v" --profile minimal --no-self-update >/dev/null 2>&1
    CARGO_TARGET_DIR=target/msrv cargo "+$v" check -q $CODECS --lib --locked
    echo "msrv: $(sed "s/-p //g" <<<"$CODECS")build with Rust $v"

# The codec tests against their C oracles (fetched automatically, cached in .cache/), in parallel:
# the default build (AMR: runtime AVX2 dispatch; LC3: baseline SSE2 kernels) and the portable
# path (--cfg portable: src/simd.rs compiled out); with AMR-NB also its vectors on the SSE2
# kernels' array forms (the non-x86 bodies). One line per run, a failing run's whole log
# (scripts/test.sh).
test: liblc3 lc3plus
    @bash scripts/test.sh

# The tests and conformance on aarch64: cross-built, each test binary under qemu-aarch64 (the portable code; correctness only, qemu timing means nothing; C-oracle-only tests skipped; ~15 min)
test-aarch64: liblc3 lc3plus
    @TARGET=aarch64 bash scripts/test.sh

# Codec conformance, one summary line per run: each codec crate present, bit-exact vs its oracle (its conformance tests name the set: AMR the 3GPP vectors and reference C, LC3 liblc3 on the SIG operating points; pinned deviations counted, not failed)
conform: liblc3 lc3plus
    @[ ! -d crates/amr-nb ] || cargo test -q --release -p amr-nb --test conformance -- --nocapture 2>&1 | grep -E "bit-exact|failing|mismatch|panicked|error" | sed "s/^\.*//" || true
    @[ ! -d crates/amr-wb ] || cargo test -q --release -p amr-wb --test conformance -- --nocapture 2>&1 | grep -E "bit-exact|panicked|error" | sed "s/^\.*//" || true
    @[ ! -d crates/amr-wb ] || cargo test -q --release -p amr-wb --test enc_conformance -- --nocapture 2>&1 | grep -E "bit-exact|bit sync|panicked|error" | sed "s/^\.*//" || true
    @[ ! -d crates/amr-wb ] || WB_STRESS_STREAMS=200 cargo test -q --release -p amr-wb --test vs_ref -- --nocapture 2>&1 | grep -E "bit-exact|differs|panicked|error" | sed "s/^\.*//" || true
    @[ ! -d crates/amr-wb ] || cargo test -q --release -p amr-wb --test enc_vs_ref -- --nocapture 2>&1 | grep -E "bit-exact|differs|panicked|error" | sed "s/^\.*//" || true
    @[ ! -d crates/lc3 ] || cargo test -q --release -p lc3 --test conformance -- --nocapture 2>&1 | grep -E "bit-exact|byte-identical|panicked|error" | sed "s/^\.*//" || true

# Decoder + encoder speed, Rust vs the legacy C (AMR: opencore -O3; LC3: liblc3 as distributions build it, -O3 -ffast-math, and strict), pinned to one core; µs/frame per mode (CRATE=<codec>, default the first present; BENCH=dec|enc; BENCH=all: every codec × side, the "all" lines (before/after a sweep); BENCH=stages: encoder and decoder µs/frame per stage, time attribution; BENCH=kern: ns/call per kernel, KERN=<substring> filters; AB=<ref>: ref vs working tree (B=<ref>: vs that ref; 3 reps per run), BENCH_MODE=<mode>: one mode only (each: per-mode table); with kern: min of 3 + ratio)
bench: liblc3 lc3plus
    @bash scripts/bench.sh

# Disassemble one function (count, top instructions, listing in target/asm.s), e.g. 'ltpf.*dot16' (an AMR AVX2 copy: 'cor_h_tm.*avx2_copy')
[positional-arguments]
asm symbol:
    @bash scripts/asm.sh "$1"

# Rank the Rust codec's functions by sampled cycles (perf on the shipped build, inline frames resolved; the default and the portable build; CRATE=<codec>, BENCH=dec|enc|all, BENCH_MODE=<mode>, TOP=<n>)
perf: liblc3 lc3plus
    @bash scripts/perf.sh

# Rank the Rust codec's functions by self cost (callgrind, no-inline copy; CRATE=<codec>, BENCH=dec|enc, BENCH_MODE=<mode>: one mode)
profile: liblc3 lc3plus
    @bash scripts/profile.sh

# Fuzz (nightly + cargo-fuzz, installed on first use; fuzz/<product>/), each codec vs its C oracle: AMR (nb_*, wb_*) the rfc4867 readers, the decoder and the encoder vs the 3GPP reference C; lc3_decode_vs_c: the LC3 decoder vs liblc3
[positional-arguments]
fuzz target="all" seconds="60": liblc3
    @bash scripts/fuzz.sh "$1" "$2"

# Regenerate crates/lc3/src/tables.rs from liblc3's compiled tables (scripts/gen_lc3_tables.c)
lc3-tables: liblc3
    @mkdir -p target && L=.cache/liblc3/liblc3-1.1.3 && awk '/static const (float|int16_t) (ge_(14|18|22|26|30)|h4|lag_window|h_(8|16|24|32|48)k_12k8_q15)\[/,/};/' $L/src/sns.c $L/src/ltpf.c $L/src/tns.c > target/lc3_locals.h && cc -std=c11 -Itarget -I$L/include -I$L/src scripts/gen_lc3_tables.c $L/src/bits.c $L/src/bwdet.c $L/src/ltpf.c -lm -o target/gen_lc3_tables && target/gen_lc3_tables > crates/lc3/src/tables.rs && rustfmt --edition 2024 crates/lc3/src/tables.rs && echo "lc3-tables: crates/lc3/src/tables.rs ($(wc -l < crates/lc3/src/tables.rs) lines)"

# Fetch liblc3 1.1.3 (Apache-2.0; the LC3 port's C oracle and speed baseline) into .cache/liblc3, SHA-verified
liblc3:
    @bash scripts/liblc3.sh

# Fetch ETSI TS 103 634 V1.7.1 (LC3plus), SHA-verified, and extract its thetest*.wav inputs into .cache/lc3plus/wav (LC3 conformance input only)
lc3plus:
    @bash scripts/lc3plus.sh
