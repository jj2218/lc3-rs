//! LC3 decoder differential: `lc3::Decoder` against liblc3's (strict-IEEE build,
//! `liblc3-sys`), frame by frame, 16-bit and float output compared bit for bit, with the
//! status (decoded, concealed) and the frame-size error.
//! Input: one byte for the configuration (frame duration × coded rate × PCM rate, the coded
//! rate or 48 kHz), then per frame a 2-byte header (bit 0: lost; bit 1: encoded; the rest: the
//! frame size, 0..=420 bytes, sizes outside 20..=400 rejected by both) and the frame's bytes,
//! or, encoded, one frame of 16-bit PCM (little-endian, at the coded rate) that liblc3's
//! encoder turns into the frame (in range: 20 + size % 381 bytes), then 1 byte: the bit to flip
//! (byte × 13; none past the frame). Encoded frames reach the decoding paths that random bytes rarely do.
#![no_main]
#![allow(unsafe_code)]

use lc3::{Decoder, Error, Status};
use libfuzzer_sys::fuzz_target;
use liblc3_sys::*;
use std::ffi::c_void;

struct C {
    _mem: Mem,
    dec: *mut c_void,
}

impl C {
    fn new(dt: i32, sr: i32, sr_pcm: i32) -> Self {
        let mut mem = Mem::new(unsafe { lc3_decoder_size(dt, sr_pcm) });
        let dec = unsafe { lc3_setup_decoder(dt, sr, sr_pcm, mem.ptr()) };
        assert!(!dec.is_null());
        C { _mem: mem, dec }
    }
    fn decode<T>(&mut self, frame: Option<&[u8]>, nbytes: usize, fmt: i32, pcm: &mut [T]) -> i32 {
        let input: *const c_void = frame.map_or(std::ptr::null(), |f| f.as_ptr().cast());
        unsafe {
            lc3_decode(
                self.dec,
                input,
                nbytes as i32,
                fmt,
                pcm.as_mut_ptr().cast(),
                1,
            )
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&cfg, mut rest)) = data.split_first() else {
        return;
    };
    let dt = [2500u32, 5000, 7500, 10000][usize::from(cfg % 4)];
    let sr = [8000u32, 16000, 24000, 32000, 48000][usize::from(cfg / 4 % 5)];
    let sr_pcm = if cfg / 20 % 2 == 1 { 48000 } else { sr };
    let (mut r16, mut rf) = (
        Decoder::new(dt, sr, sr_pcm).unwrap(),
        Decoder::new(dt, sr, sr_pcm).unwrap(),
    );
    let (mut c16, mut cf) = (
        C::new(dt as i32, sr as i32, sr_pcm as i32),
        C::new(dt as i32, sr as i32, sr_pcm as i32),
    );
    let ns = r16.frame_samples();
    let ns_enc = unsafe { lc3_frame_samples(dt as i32, sr as i32) } as usize;
    let mut enc_mem = Mem::new(unsafe { lc3_encoder_size(dt as i32, sr as i32) });
    let enc = unsafe { lc3_setup_encoder(dt as i32, sr as i32, 0, enc_mem.ptr()) };
    let mut encoded = Vec::new();
    let mut f = 0;
    while let [h0, h1, tail @ ..] = rest {
        let h = u16::from_le_bytes([*h0, *h1]);
        let lost = h & 1 == 1;
        let mut nbytes = usize::from(h >> 2) % 421;
        let buf = if lost {
            rest = tail;
            &[][..]
        } else if h & 2 == 2 {
            let Some((pcm, [flip, next @ ..])) = tail.split_at_checked(2 * ns_enc) else {
                break;
            };
            rest = next;
            let pcm: Vec<i16> = pcm
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&b| i16::from_le_bytes(b))
                .collect();
            nbytes = 20 + nbytes % 381;
            encoded.resize(nbytes, 0);
            let rc = unsafe {
                lc3_encode(
                    enc,
                    LC3_PCM_FORMAT_S16,
                    pcm.as_ptr().cast(),
                    1,
                    nbytes as i32,
                    encoded.as_mut_ptr().cast(),
                )
            };
            assert_eq!(rc, 0);
            let flip = usize::from(*flip) * 13;
            if flip < 8 * nbytes {
                encoded[flip / 8] ^= 1 << (flip % 8);
            }
            &encoded[..]
        } else {
            let Some((buf, next)) = tail.split_at_checked(nbytes) else {
                break;
            };
            rest = next;
            buf
        };
        let frame = (!lost).then_some(buf);
        let at = format!("dt {dt} sr {sr}->{sr_pcm} frame {f} ({nbytes} bytes, lost {lost})");

        let (mut o16, mut p16) = (vec![0i16; ns], vec![0i16; ns]);
        let s16 = r16.decode(frame, &mut o16);
        let rc16 = c16.decode(frame, nbytes, LC3_PCM_FORMAT_S16, &mut p16);
        let (mut of, mut pf) = (vec![0f32; ns], vec![0f32; ns]);
        let sf = rf.decode_f32(frame, &mut of);
        let rcf = cf.decode(frame, nbytes, LC3_PCM_FORMAT_FLOAT, &mut pf);
        for (s, rc) in [(s16, rc16), (sf, rcf)] {
            let want = match s {
                Ok(Status::Decoded) => 0,
                Ok(Status::Concealed) => 1,
                Err(Error::FrameSize) => -1,
                Err(e) => panic!("{e:?} {at}"),
            };
            assert_eq!(want, rc, "status {at}");
        }
        if rc16 >= 0 {
            assert_eq!(o16, p16, "s16 {at}");
            let bits = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
            assert_eq!(bits(&of), bits(&pf), "float {at}");
        }
        f += 1;
    }
});
