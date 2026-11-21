# Results

Measured results of `lc3`. Every number is reproducible with the recipe or command in its
heading; the oracle (liblc3 1.1.3) and the test items (the ETSI LC3plus `thetest*.wav`, as
input audio only) are downloaded and SHA-verified on first use into `.cache/`, never committed.

## Conformance (`just conform`)
The Bluetooth SIG LC3 conformance operating points (the sample rate × bitrate pairs of liblc3's
conformance reports, made with the SIG's LC3 Test Software: speech at 8–32 kHz, music at 48 kHz,
80–124 kbit/s; 8 at 7.5 ms, 7 at 10 ms) and the non-high-resolution points of liblc3's LC3plus
reports (ETSI's test: 2.5 ms at 48 kHz, 128–320 kbit/s; 5 ms at 48 kHz, 64–256 kbit/s; more
10 ms bitrates), plus the SIG's rates at 2.5 and 5 ms (its 10 ms bitrates, frames clamped to
20–400 bytes), over the ETSI items `thetest{8,16,24,32,48}.wav`.

| side | 2.5 ms | 5 ms | 7.5 ms | 10 ms | result |
|---|---:|---:|---:|---:|---|
| operating points (SIG + LC3plus) | 7 + 3 | 7 + 6 | 8 + 0 | 7 + 6 | |
| decoder: clean, 48 kHz output, burst losses (~10%) | 96,819 | 60,510 | 28,227 | 32,256 | **bit-exact** with liblc3, 16-bit and float output |
| encoder: PCM at the coded rate and at 48 kHz | 56,477 | 34,289 | 17,473 | 19,152 | **byte-identical** frames |

Bit-exact output meets the SIG decoder test's thresholds against liblc3 (max abs diff ≤ 0.00148,
RMS ≤ −89.06 dB) trivially. The SIG's own test software is members-only and has not been run;
liblc3 passes it (its reports), so conformance is inferred through liblc3.

## Against liblc3 (`cargo test --release -p lc3 --test vs_c`)
liblc3 built without `-ffast-math` and without FMA contraction (`crates/liblc3-sys`), so its
operation order is the source's and its output the same on every CPU: the port keeps that order.
- Decoder: 48,000 frames over every duration × coded rate × PCM rate (all 60 pairs), 20–400
  bytes, bursts of 1–12 lost frames (every concealment step) and bit-flipped frames; and 54,000
  random-byte frames over the same 60. Bit-exact, 16-bit and float output.
- Encoder: 24,000 frames over the same 60 configurations (voiced, noise, silence, clicks,
  band-limited, near-Nyquist, lone full-scale impulses, ±32768 squares, to clipping), 16-bit and
  float input. Byte-identical.
- 7 s of continuous loss at three configurations: liblc3's float output but for its clicks
  (defect 4 below; 480,527 samples identical).
- Both on x86-64 and on aarch64 (`just test-aarch64`, under qemu).

## Fuzzing (`just fuzz <target> [seconds]`)
| target | checks | findings |
|---|---|---|
| `lc3_decode_vs_c` | the decoder vs liblc3 on random frames (sizes 0–420), losses, and liblc3-encoded frames bit-flipped | none |
| `lc3_encode_vs_c` | the encoder vs liblc3 on random PCM, both formats, gains to 4× | none |
| `lc3_api` | the public API alone: any configuration (valid ones build, invalid ones their error), any 16-bit or float bit pattern, any bytes, short and long PCM buffers, loss runs | none (2026-11-21: 704k runs in 5 min, 667k in 15) |

## Speed (`CRATE=lc3 just bench`)
µs per frame, best of 15, single core, x86-64 (no AVX2: every build is baseline x86-64).
"distro": liblc3 as distributions build it (`-O3 -ffast-math`, LTO); "exact": the oracle build
above. 16-bit PCM over the ETSI items; medians of 3 runs.

| side | operating point | Rust | distro | exact | vs distro | vs exact |
|---|---|---:|---:|---:|---:|---:|
| decoder | 16 kHz, 7.5 ms, 32 kbit/s | 4.41 | 5.07 | 6.12 | **1.15×** | 1.39× |
| decoder | 48 kHz, 10 ms, 96 kbit/s | 14.65 | 16.22 | 18.76 | **1.11×** | 1.28× |
| encoder | 16 kHz, 7.5 ms, 32 kbit/s | 11.55 | 12.26 | 13.54 | **1.08×** | 1.17× |
| encoder | 48 kHz, 10 ms, 96 kbit/s | 28.34 | 32.41 | 35.75 | **1.14×** | 1.26× |

Measured 2026-11-21 for 0.2.0 (quiet machine; the encoder's 16 kHz ratio ranged 1.00–1.08× over
the runs; 0.1.0 on 2026-11-19: 1.01×, 1.05×, 0.97×, 1.09×). The decoder's gain since 0.1.0 is
mostly the arithmetic decoder's symbol search (cycles −5…6%, `AB= COUNT=1`; slower at 8 kHz
2.5 ms, +3%); the rest is the machine's day.

## Defects found in the C (liblc3 1.1.3; each absent in the Rust port)
Each: **the defect**: its effect. Found: how. Rust: how the port avoids it.
1. **Out-of-range float-to-int cast (UB) in the 16-bit output**: `lc3_decode`'s store converts
   each synthesised sample with `(int)`; beyond the `int` range the result is undefined (x86:
   `INT_MIN`, so a huge positive sample comes out −32768). Reached by frames that pass the
   bitstream checks with a huge gain (the review: global gain 255, 3 of 36,000 frames). Found:
   reading the C while porting. Rust: saturates to [−32768, 32767].
2. **The same cast on float input**: `LC3_SAT16((int32_t)xs[i])` in the encoder's float path is
   undefined from magnitude 65536 (2^31 after the 2^15 scaling) and for NaN, so frames from such
   input depend on the CPU and compiler. Found: reading the C while porting. Rust: the cast
   saturates (NaN gives 0); `Encoder::encode_f32` documents where the frames may differ.
3. **Shift by 32 in the bit reader** once a frame is exhausted (undefined: x86 keeps the
   value, ARM clears it). Such a frame fails the decoder's check and is concealed either way.
   Found: reading the C while porting. Rust: `checked_shr`, giving 0.
4. **Full-scale clicks after long losses, float output**: `store_float` scales each sample by
   `lc3_ldexpf(s, -15)`, which subtracts 15 from the exponent field without an underflow check;
   once the concealment's fade (×0.85 a frame) brings samples below 2^-112, they wrap into huge
   values of the opposite sign and the clamp outputs ±1.0. Measured: from ~5 s of continuous loss
   (48 kHz, 10 ms: first at frame 506), 35,833 such samples over 7 s at three configurations; the
   16-bit output stays silent. Found: the pre-0.2.0 review (a PR hindsight review that ran long
   losses). Rust: those samples scale exactly (~0); every other sample is liblc3's
   (`long_loss_differs_from_liblc3_only_by_its_clicks`, a pinned deviation).

## Improvements over the C (liblc3 1.1.3)
1. **One output on every IEEE-float CPU (x87 refused at compile time), at the speed of the fast build**: liblc3 as distributions build it
   (`-O3 -ffast-math`) differs from its strict build from the first frame, and per CPU, so its
   output is not defined by its source. Found: the oracle spike (strict vs `-ffast-math`
   builds). Rust: the C's float operation order (no reassociation, no FMA contraction): the
   strict build's output on x86-64 and aarch64, faster than the distribution build at the
   measured points (1.08–1.15×, Speed above).
