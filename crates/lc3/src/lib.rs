//! Pure-Rust LC3 (Low Complexity Communication Codec, the Bluetooth LE Audio codec): encoder
//! and decoder with packet loss concealment, ported from liblc3 1.1.3 (Apache-2.0; see NOTICE).
//! Bit-exact with liblc3 built without `-ffast-math` (the operation order its source states),
//! so the output is the same on every IEEE-float target (tested: x86-64 and aarch64; x87 floats,
//! 32-bit x86 without SSE2, are refused at compile time). Where liblc3's behaviour is undefined
//! (a float-to-int cast out of range) or audibly broken, this crate differs: see the
//! [`Decoder`] and [`Encoder::encode_f32`] docs. No input panics.
//!
//! One [`Encoder`] or [`Decoder`] per channel, set up for a frame duration (7.5 or 10 ms; also
//! liblc3's 2.5 and 5 ms), a coded sample rate (8–48 kHz) and a PCM rate at or above it. A
//! frame is [`MIN_FRAME_BYTES`] to [`MAX_FRAME_BYTES`] bytes: its size sets the bitrate. A lost
//! frame is decoded as `None` and concealed.
//!
//! ```
//! use lc3::{Decoder, Encoder, Status};
//!
//! // 10 ms frames at 48 kHz, 96 kbit/s: 120-byte frames of 480 samples
//! let mut enc = Encoder::new(10_000, 48_000, 48_000)?;
//! let mut dec = Decoder::new(10_000, 48_000, 48_000)?;
//! let pcm = vec![0i16; enc.frame_samples()];
//! let mut frame = [0u8; 120];
//! enc.encode(&pcm, &mut frame)?;
//!
//! let mut out = vec![0i16; dec.frame_samples()];
//! assert_eq!(dec.decode(Some(&frame), &mut out)?, Status::Decoded);
//! dec.decode(None, &mut out)?; // a lost frame: concealed
//! # Ok::<(), lc3::Error>(())
//! ```

// x87 floats (32-bit x86 without SSE2) round every operation to 80 bits first: the output is
// then not liblc3's (measured on i586: every stream differs), and fastmath's rounding trick
// (`x + 1.5 * 2^20`) needs single-precision rounding.
#[cfg(all(target_arch = "x86", not(target_feature = "sse2")))]
compile_error!(
    "lc3 needs IEEE single-precision float arithmetic: on 32-bit x86 enable SSE2 \
     (e.g. target i686, or -C target-feature=+sse2); x87 floats change the output"
);

use std::fmt;

mod attdet;
mod bits;
mod bwdet;
mod common;
mod decoder;
mod encoder;
mod energy;
mod fastmath;
mod ltpf;
mod mdct;
mod plc;
mod simd;
mod sns;
mod spec;
mod tables;
#[cfg(test)]
mod testutil;
mod tns;

pub use decoder::{Decoder, Status};
pub use encoder::Encoder;

/// The smallest frame, in bytes (`LC3_MIN_FRAME_BYTES`).
pub const MIN_FRAME_BYTES: usize = 20;
/// The largest frame, in bytes (`LC3_MAX_FRAME_BYTES`).
pub const MAX_FRAME_BYTES: usize = 400;

/// Why an encoder or decoder can't be built, or a frame can't be encoded or decoded. A corrupt
/// frame is not an error: the decoder conceals it ([`Status::Concealed`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Error {
    /// The frame duration (µs) is not 2500, 5000, 7500 or 10000.
    InvalidFrameDuration(u32),
    /// A sample rate or PCM rate (Hz) is not 8000, 16000, 24000, 32000 or 48000.
    InvalidSampleRate(u32),
    /// The PCM rate is below the coded sample rate.
    PcmRateBelowSampleRate {
        /// the coded rate (Hz)
        sample_rate: u32,
        /// the PCM rate (Hz)
        pcm_rate: u32,
    },
    /// A frame of this many bytes: below [`MIN_FRAME_BYTES`] or above [`MAX_FRAME_BYTES`].
    FrameSize(usize),
    /// A PCM buffer shorter than a frame (nothing is encoded or decoded).
    PcmLength {
        /// the samples a frame needs (`frame_samples()`)
        expected: usize,
        /// the buffer's
        actual: usize,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Error::InvalidFrameDuration(us) => {
                write!(f, "frame duration {us} µs is not 2500, 5000, 7500 or 10000")
            }
            Error::InvalidSampleRate(hz) => {
                write!(
                    f,
                    "sample rate {hz} Hz is not 8000, 16000, 24000, 32000 or 48000"
                )
            }
            Error::PcmRateBelowSampleRate {
                sample_rate,
                pcm_rate,
            } => write!(
                f,
                "PCM rate {pcm_rate} Hz is below the coded rate {sample_rate} Hz"
            ),
            Error::FrameSize(n) => write!(
                f,
                "frame of {n} bytes, not {MIN_FRAME_BYTES} to {MAX_FRAME_BYTES}"
            ),
            Error::PcmLength { expected, actual } => {
                write!(
                    f,
                    "PCM buffer of {actual} samples, a frame needs {expected}"
                )
            }
        }
    }
}

impl std::error::Error for Error {}

/// `resolve_dt` and `resolve_srate` of `lc3_setup_encoder`/`lc3_setup_decoder`: the C enums'
/// indices (dt, sr, sr_pcm).
fn setup(frame_us: u32, sample_rate: u32, pcm_rate: u32) -> Result<(usize, usize, usize), Error> {
    let dt = common::FRAME_US
        .iter()
        .position(|&v| v == frame_us)
        .ok_or(Error::InvalidFrameDuration(frame_us))?;
    let srate = |hz: u32| {
        common::SAMPLE_RATES
            .iter()
            .position(|&v| v == hz)
            .ok_or(Error::InvalidSampleRate(hz))
    };
    let (sr, sr_pcm) = (srate(sample_rate)?, srate(pcm_rate)?);
    if sr > sr_pcm {
        return Err(Error::PcmRateBelowSampleRate {
            sample_rate,
            pcm_rate,
        });
    }
    Ok((dt, sr, sr_pcm))
}

/// The frame-size check of `lc3_encode`/`lc3_decode`.
fn check_frame(len: usize) -> Result<(), Error> {
    if (MIN_FRAME_BYTES..=MAX_FRAME_BYTES).contains(&len) {
        Ok(())
    } else {
        Err(Error::FrameSize(len))
    }
}

/// A PCM buffer holds a frame (checked before any state changes).
fn check_pcm(len: usize, expected: usize) -> Result<(), Error> {
    if len < expected {
        Err(Error::PcmLength {
            expected,
            actual: len,
        })
    } else {
        Ok(())
    }
}

/// The codec's configuration in the caller's units, for `Debug`.
fn debug_config(
    f: &mut fmt::Formatter<'_>,
    name: &str,
    (dt, sr, sr_pcm): (usize, usize, usize),
) -> fmt::Result {
    f.debug_struct(name)
        .field("frame_us", &common::FRAME_US[dt])
        .field("sample_rate", &common::SAMPLE_RATES[sr])
        .field("pcm_rate", &common::SAMPLE_RATES[sr_pcm])
        .finish_non_exhaustive()
}

/// The README's example compiles and runs.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctest;
