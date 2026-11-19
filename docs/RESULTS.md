# Results

Measured results of `lc3`. Every number is reproducible with the recipe or command in its
heading; the oracle (liblc3 1.1.3) and the test items (the ETSI LC3plus `thetest*.wav`, as
input audio only) are downloaded and SHA-verified on first use into `.cache/`, never committed.

## Conformance (`just conform`)
The Bluetooth SIG LC3 conformance operating points (the 8 sample rate × bitrate pairs of
liblc3's conformance reports, made with the SIG's LC3 Test Software: speech at 8–32 kHz, music
at 48 kHz, 80–124 kbit/s), every frame duration (2.5 and 5 ms at the 10 ms bitrates, frames
clamped to 20–400 bytes), over the ETSI items `thetest{8,16,24,32,48}.wav`.

| side | 2.5 ms | 5 ms | 7.5 ms | 10 ms | result |
|---|---:|---:|---:|---:|---|
| decoder: clean, 48 kHz output, burst losses (~10%) | 96,819 | 48,408 | 32,259 | 24,192 | **bit-exact** with liblc3, 16-bit and float output |
| encoder: PCM at the coded rate and at 48 kHz | 52,443 | 26,221 | 17,473 | 13,104 | **byte-identical** frames |

So the SIG decoder test's metrics against liblc3 are a max abs diff of 0 (limit 0.00148) and
an RMS of −∞ dB (limit −89.06 dB). The SIG's own test software is members-only and has not been
run; liblc3 passes it (its reports), so conformance is inferred through liblc3.

## Against liblc3 (`cargo test --release -p lc3 --test vs_c`)
liblc3 built without `-ffast-math` and without FMA contraction (`crates/liblc3-sys`), so its
operation order is the source's and its output the same on every CPU: the port keeps that order.
- Decoder: 48,000 frames over every duration × rate × PCM rate, 20–400 bytes, bursts of 1–12
  lost frames (every concealment step) and bit-flipped frames; and 50,000 random-byte frames.
  Bit-exact, 16-bit and float output.
- Encoder: 24,000 frames (voiced, noise, silence, clicks, band-limited, near-Nyquist, to
  clipping), 16-bit and float input. Byte-identical.
- Both on x86-64 and on aarch64 (`just test-aarch64`, under qemu).

## Fuzzing (`just fuzz <target> [seconds]`)
| target | checks | findings |
|---|---|---|
| `lc3_decode_vs_c` | the decoder vs liblc3 on random frames (sizes 0–420), losses, and liblc3-encoded frames bit-flipped | none |
| `lc3_encode_vs_c` | the encoder vs liblc3 on random PCM, both formats, gains to 4× | none |

## Speed (`CRATE=lc3 just bench`)
µs per frame, best of 15, single core, x86-64 (no AVX2: every build is baseline x86-64).
"distro": liblc3 as distributions build it (`-O3 -ffast-math`, LTO); "exact": the oracle build
above. 16-bit PCM over the ETSI items; medians of 3 runs.

| side | operating point | Rust | distro | exact | vs distro | vs exact |
|---|---|---:|---:|---:|---:|---:|
| decoder | 16 kHz, 7.5 ms, 32 kbit/s | 4.59 | 4.65 | 5.42 | **1.01×** | 1.18× |
| decoder | 48 kHz, 10 ms, 96 kbit/s | 14.81 | 15.53 | 17.42 | **1.05×** | 1.18× |
| encoder | 16 kHz, 7.5 ms, 32 kbit/s | 11.75 | 11.42 | 12.73 | **0.97×** | 1.08× |
| encoder | 48 kHz, 10 ms, 96 kbit/s | 27.19 | 29.66 | 32.97 | **1.09×** | 1.21× |

Measured 2026-11-19 for 0.1.0.
