//! FFI to liblc3 1.1.3, used only as a test and benchmark oracle (strict IEEE float build:
//! `build.rs`). Internal functions the Rust port is differentially tested against; signatures
//! mirror the C headers, C enums passed as `i32`.
#![allow(unsafe_code, missing_docs)]

use std::ffi::c_void;

/// `enum lc3_pcm_format`.
pub const LC3_PCM_FORMAT_S16: i32 = 0;
pub const LC3_PCM_FORMAT_FLOAT: i32 = 3;

// `lc3.h`: the public codec API (encoder/decoder in caller-provided memory).
unsafe extern "C" {
    pub fn lc3_frame_samples(dt_us: i32, sr_hz: i32) -> i32;
    pub fn lc3_frame_bytes(dt_us: i32, bitrate: i32) -> i32;
    pub fn lc3_encoder_size(dt_us: i32, sr_hz: i32) -> u32;
    pub fn lc3_setup_encoder(
        dt_us: i32,
        sr_hz: i32,
        sr_pcm_hz: i32,
        mem: *mut c_void,
    ) -> *mut c_void;
    pub fn lc3_encode(
        encoder: *mut c_void,
        fmt: i32,
        pcm: *const c_void,
        stride: i32,
        nbytes: i32,
        out: *mut c_void,
    ) -> i32;
    pub fn lc3_decoder_size(dt_us: i32, sr_hz: i32) -> u32;
    pub fn lc3_setup_decoder(
        dt_us: i32,
        sr_hz: i32,
        sr_pcm_hz: i32,
        mem: *mut c_void,
    ) -> *mut c_void;
    pub fn lc3_decode(
        decoder: *mut c_void,
        input: *const c_void,
        nbytes: i32,
        fmt: i32,
        pcm: *mut c_void,
        stride: i32,
    ) -> i32;
}

/// An owned liblc3 encoder or decoder (its memory, 8-byte aligned).
pub struct Mem(Vec<u64>);

impl Mem {
    pub fn new(bytes: u32) -> Self {
        Mem(vec![0; (bytes as usize).div_ceil(8)])
    }
    pub fn ptr(&mut self) -> *mut c_void {
        self.0.as_mut_ptr().cast()
    }
}

unsafe extern "C" {
    /// `mdct.h`: `x` ns samples, `d` nd delayed samples (updated), `y` ns coefficients.
    pub fn lc3_mdct_forward(dt: i32, sr: i32, sr_dst: i32, x: *const f32, d: *mut f32, y: *mut f32);
    /// `mdct.h`: `x` ns coefficients, `d` nd delayed samples (updated), `y` ns samples.
    pub fn lc3_mdct_inverse(dt: i32, sr: i32, sr_src: i32, x: *const f32, d: *mut f32, y: *mut f32);
}

/// liblc3's public codec API as function pointers: the oracle build or the speed baseline.
#[derive(Clone, Copy)]
pub struct Api {
    pub encoder_size: unsafe extern "C" fn(i32, i32) -> u32,
    pub setup_encoder: unsafe extern "C" fn(i32, i32, i32, *mut c_void) -> *mut c_void,
    pub encode: unsafe extern "C" fn(*mut c_void, i32, *const c_void, i32, i32, *mut c_void) -> i32,
    pub decoder_size: unsafe extern "C" fn(i32, i32) -> u32,
    pub setup_decoder: unsafe extern "C" fn(i32, i32, i32, *mut c_void) -> *mut c_void,
    pub decode: unsafe extern "C" fn(*mut c_void, *const c_void, i32, i32, *mut c_void, i32) -> i32,
}

/// The oracle build's API.
pub const STRICT: Api = Api {
    encoder_size: lc3_encoder_size,
    setup_encoder: lc3_setup_encoder,
    encode: lc3_encode,
    decoder_size: lc3_decoder_size,
    setup_decoder: lc3_setup_decoder,
    decode: lc3_decode,
};

/// The speed baseline: liblc3 as distributions build it (`-O3 -ffast-math`, LTO; `build.rs`),
/// its output not the oracle's. `None` where it isn't built (not x86-64, or no GCC/binutils).
#[cfg(liblc3_fast)]
pub const FAST: Option<Api> = Some(Api {
    encoder_size: fast::lc3fast_encoder_size,
    setup_encoder: fast::lc3fast_setup_encoder,
    encode: fast::lc3fast_encode,
    decoder_size: fast::lc3fast_decoder_size,
    setup_decoder: fast::lc3fast_setup_decoder,
    decode: fast::lc3fast_decode,
});
#[cfg(not(liblc3_fast))]
pub const FAST: Option<Api> = None;

#[cfg(liblc3_fast)]
mod fast {
    use std::ffi::c_void;
    unsafe extern "C" {
        pub fn lc3fast_encoder_size(dt_us: i32, sr_hz: i32) -> u32;
        pub fn lc3fast_setup_encoder(
            dt_us: i32,
            sr_hz: i32,
            sr_pcm_hz: i32,
            mem: *mut c_void,
        ) -> *mut c_void;
        pub fn lc3fast_encode(
            encoder: *mut c_void,
            fmt: i32,
            pcm: *const c_void,
            stride: i32,
            nbytes: i32,
            out: *mut c_void,
        ) -> i32;
        pub fn lc3fast_decoder_size(dt_us: i32, sr_hz: i32) -> u32;
        pub fn lc3fast_setup_decoder(
            dt_us: i32,
            sr_hz: i32,
            sr_pcm_hz: i32,
            mem: *mut c_void,
        ) -> *mut c_void;
        pub fn lc3fast_decode(
            decoder: *mut c_void,
            input: *const c_void,
            nbytes: i32,
            fmt: i32,
            pcm: *mut c_void,
            stride: i32,
        ) -> i32;
    }
}
