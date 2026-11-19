//! LC3 encoder differential: `lc3::Encoder` against liblc3's (strict-IEEE build,
//! `liblc3-sys`), frame by frame, the frames compared byte for byte.
//! Input: one byte for the configuration (frame duration × coded rate × PCM rate, the coded
//! rate or 48 kHz), then per frame a 2-byte header (bit 0: float input, else 16-bit; the
//! rest: the frame size, 20 + v % 381 bytes) and one frame of 16-bit PCM (little-endian; as
//! float, scaled by 1/32768 and a gain from the header up to 4, clipping included).
#![no_main]
#![allow(unsafe_code)]

use lc3::Encoder;
use libfuzzer_sys::fuzz_target;
use liblc3_sys::*;

fuzz_target!(|data: &[u8]| {
    let Some((&cfg, mut rest)) = data.split_first() else {
        return;
    };
    let dt = [2500u32, 5000, 7500, 10000][usize::from(cfg % 4)];
    let sr = [8000u32, 16000, 24000, 32000, 48000][usize::from(cfg / 4 % 5)];
    let sr_pcm = if cfg / 20 % 2 == 1 { 48000 } else { sr };
    let mut rust = Encoder::new(dt, sr, sr_pcm).unwrap();
    let ns = rust.frame_samples();
    let mut mem = Mem::new(unsafe { lc3_encoder_size(dt as i32, sr_pcm as i32) });
    let c = unsafe { lc3_setup_encoder(dt as i32, sr as i32, sr_pcm as i32, mem.ptr()) };
    let mut f = 0;
    while let [h0, h1, tail @ ..] = rest {
        let Some((pcm, next)) = tail.split_at_checked(2 * ns) else {
            break;
        };
        rest = next;
        let h = u16::from_le_bytes([*h0, *h1]);
        let nbytes = 20 + usize::from(h >> 1) % 381;
        let pcm: Vec<i16> = pcm
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&b| i16::from_le_bytes(b))
            .collect();
        let (mut want, mut got) = (vec![0u8; nbytes], vec![0u8; nbytes]);
        let rc = if h & 1 == 0 {
            rust.encode(&pcm, &mut got).unwrap();
            unsafe {
                lc3_encode(
                    c,
                    LC3_PCM_FORMAT_S16,
                    pcm.as_ptr().cast(),
                    1,
                    nbytes as i32,
                    want.as_mut_ptr().cast(),
                )
            }
        } else {
            let gain = 1.0 + f32::from(h >> 14);
            let pcm: Vec<f32> = pcm.iter().map(|&v| f32::from(v) / 32768.0 * gain).collect();
            rust.encode_f32(&pcm, &mut got).unwrap();
            unsafe {
                lc3_encode(
                    c,
                    LC3_PCM_FORMAT_FLOAT,
                    pcm.as_ptr().cast(),
                    1,
                    nbytes as i32,
                    want.as_mut_ptr().cast(),
                )
            }
        };
        assert_eq!(rc, 0);
        assert_eq!(
            got, want,
            "dt {dt} sr {sr_pcm}->{sr} frame {f} ({nbytes} bytes)"
        );
        f += 1;
    }
});
