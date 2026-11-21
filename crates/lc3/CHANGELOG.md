# Changelog

One section per release, newest first. The release commit in the repository carries the
section of each crate it releases.

## 0.2.0 (2026-11-21)
Review sweep before the first update (16 reviews; the codec still bit-exact with liblc3 but
for one liblc3 defect fixed, below). Breaking: `Error` and `Status` changes.
- `Error` says what was wrong: `InvalidFrameDuration(us)`, `InvalidSampleRate(hz)`,
  `PcmRateBelowSampleRate { .. }`, `FrameSize(len)`, `PcmLength { expected, actual }`; it
  implements `Display` and `std::error::Error`, and is `#[non_exhaustive]` (so is `Status`).
- No input panics: a PCM buffer shorter than a frame is `Error::PcmLength` (it panicked; the
  decoder's state had already advanced), and every rejected call leaves the codec unchanged.
- `Debug` of `Encoder`/`Decoder` prints the configuration (it dumped ~10 KB of buffers).
- Float output after ~5 s of continuous loss: liblc3 (and 0.1.0) emit full-scale clicks there
  (its `ldexpf` wraps the fading samples' exponent); they are now ~0. Every other sample is still
  liblc3's.
- The decoder ~5% faster on x86-64 (the arithmetic decoder's symbol search by one division and
  lane compares; 8 kHz at 2.5 ms ~3% slower); now 1.08–1.15× liblc3 as distributions build it.
- x87 floats (32-bit x86 without SSE2) are refused at compile time: their output differed.
- Docs: `# Errors` on every fallible function, the README example compiled as a doctest, the
  one 16-bit output difference from liblc3 (where its cast is undefined) stated; the patent
  wording names the SIG licence's scope (members' necessary claims).

## 0.1.0 (2026-11-19)
First release: LC3 encoder and decoder, ported from liblc3 1.1.3.
- `Encoder` and `Decoder` for 2.5, 5, 7.5 and 10 ms frames at 8 to 48 kHz, PCM at the coded
  rate or above, 16-bit or float samples, frames of 20 to 400 bytes; packet loss concealment
  for lost or corrupt frames.
- Bit-exact with liblc3 built without `-ffast-math`, on x86-64 and aarch64: the Bluetooth SIG
  conformance operating points over realistic audio, burst losses, and fuzzing.
- Faster than liblc3 as distributions build it at 48 kHz and on par at 16 kHz (x86-64, no
  AVX2 needed).
