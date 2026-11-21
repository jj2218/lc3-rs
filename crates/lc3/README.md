# lc3

Pure-Rust LC3 (Low Complexity Communication Codec, the Bluetooth LE Audio codec): encoder and
decoder with packet loss concealment, ported from Google's liblc3 1.1.3 (Apache-2.0). Its
output is bit-exact with liblc3's (but where liblc3's is undefined), the same on every
IEEE-float target (tested: x86-64 and aarch64; 32-bit x86 needs SSE2, x87 is refused at
compile time). No input panics. No dependencies; the only `unsafe` is `src/simd.rs` (SSE2
kernels on x86-64).

```rust
use lc3::{Decoder, Encoder, Status};

fn main() -> Result<(), lc3::Error> {
    // 10 ms frames at 48 kHz, 96 kbit/s: 120-byte frames of 480 samples
    let mut enc = Encoder::new(10_000, 48_000, 48_000)?;
    let mut dec = Decoder::new(10_000, 48_000, 48_000)?;
    let pcm = vec![0i16; enc.frame_samples()];
    let mut frame = [0u8; 120];
    enc.encode(&pcm, &mut frame)?;

    let mut out = vec![0i16; dec.frame_samples()];
    assert_eq!(dec.decode(Some(&frame), &mut out)?, Status::Decoded);
    dec.decode(None, &mut out)?; // a lost frame: concealed
    Ok(())
}
```

Frame durations 7.5 and 10 ms (LC3) and liblc3's 2.5 and 5 ms; sample rates 8, 16, 24, 32
and 48 kHz; PCM at the coded rate or above (e.g. 48 kHz PCM coded at 16 kHz); 16-bit or float
samples; frames of 20 to 400 bytes (any bitrate the frame size gives).

## Results
Measured, with the recipe that reproduces each number, in
[`docs/RESULTS.md`](https://github.com/jj2218/lc3-rs/blob/main/docs/RESULTS.md).

- **Bit-exact with liblc3** (built without `-ffast-math`, the order its source states): the
  decoder on every Bluetooth SIG conformance operating point (7.5 and 10 ms; 2.5 and 5 ms at
  the same rates), 48 kHz output and burst losses; the encoder byte-identical on the same grid.
  On x86-64 and aarch64. Two differences, both liblc3 defects: a decoded 16-bit sample beyond
  the int range (a corrupt frame's) saturates, where liblc3's cast is undefined; and after ~5 s
  of continuous loss, liblc3's float output clicks (±1.0, an exponent underflow), where this
  crate's is silent.
- **Faster than liblc3 as distributions build it** (`-O3 -ffast-math`, LTO): 1.08–1.15× at
  16 and 48 kHz (medians of 3 runs), on x86-64 without AVX2; faster than liblc3's exact build at every measured
  operating point (x86-64; aarch64 is tested for correctness only).
- **Fuzzed against liblc3**: the decoder on random and corrupted frames, the encoder on random
  PCM, both formats.

## Scope
Single channel (a stream per channel, as liblc3). No LC3plus high-resolution mode. Not
qualified by the Bluetooth SIG (the SIG's test software is members-only); its conformance
operating points are checked through liblc3, which passes them. Needs Rust 1.88 or later.

## Patents
LC3 is covered by patents of its developers and others, several in force into the late 2030s.
This crate's Apache-2.0 licence, like liblc3's, grants no rights under any third party's
patents. The Bluetooth SIG's Patent/Copyright License Agreement cross-licenses its members'
"Necessary Claims" (those a compliant implementation necessarily infringes, read strictly)
royalty-free, for members' qualified Bluetooth products only; any other use (outside Bluetooth,
or by non-members), and any claim beyond those, may need licences from the patent holders
(Fraunhofer IIS offers licences for its LC3/LC3plus patents; Ericsson has declared LC3plus
patents under FRAND terms at ETSI). This is not legal advice.

## License
Apache-2.0 ([LICENSE](https://github.com/jj2218/lc3-rs/blob/main/crates/lc3/LICENSE)).
Derived from liblc3 1.1.3 (Apache-2.0); see
[NOTICE](https://github.com/jj2218/lc3-rs/blob/main/crates/lc3/NOTICE).
