# Changelog

One section per release, newest first. The release commit in the repository carries the
section of each crate it releases.

## 0.1.0 (2026-11-19)
First release: LC3 encoder and decoder, ported from liblc3 1.1.3.
- `Encoder` and `Decoder` for 2.5, 5, 7.5 and 10 ms frames at 8 to 48 kHz, PCM at the coded
  rate or above, 16-bit or float samples, frames of 20 to 400 bytes; packet loss concealment
  for lost or corrupt frames.
- Bit-exact with liblc3 built without `-ffast-math`, on x86-64 and aarch64: the Bluetooth SIG
  conformance operating points over realistic audio, burst losses, and fuzzing.
- Faster than liblc3 as distributions build it at 48 kHz and on par at 16 kHz (x86-64, no
  AVX2 needed).
