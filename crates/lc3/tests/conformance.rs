//! Conformance: the decoder on the operating points of the Bluetooth SIG LC3 conformance tests
//! (speech and music, the sample rate × bitrate pairs of liblc3's `conformance/*.html` reports,
//! made by the SIG LC3 Test Software), over realistic input: the ETSI LC3plus test items
//! `thetest{8,16,24,32,48}.wav` (`just lc3plus`, oracle input only), encoded by liblc3. Every
//! frame duration (2.5 and 5 ms: liblc3's LC3plus durations, at the same bitrates, frame sizes
//! clamped to 20..=400 bytes), PCM at the coded rate and at 48 kHz, and a run with lost
//! frames. Rust vs liblc3 (strict-IEEE build), 16-bit and float output; plus the SIG decoder
//! test's metrics against liblc3's float output (max abs diff ≤ 0.00148, RMS ≤ −89.06 dB).
//! The encoder on the same grid, and from the 48 kHz item to every lower coded rate:
//! byte-identical frames (so the round trips through either decoder agree too).
//! One summary line per frame duration and side.
#![allow(unsafe_code)]

use lc3::{Decoder, Encoder, Status};
use liblc3_sys::*;
use std::ffi::c_void;
use std::path::PathBuf;

/// (sample rate, bitrate at 10 ms, bitrate at 7.5 ms): SIG speech then music operating points.
const OPS: [(u32, i32, i32); 8] = [
    (8000, 24000, 27734),
    (16000, 32000, 32000),
    (24000, 48000, 48000),
    (32000, 64000, 61867),
    (32000, 64000, 64000),
    (48000, 80000, 80000),
    (48000, 96000, 96000),
    (48000, 124000, 124800),
];

/// The SIG decoder test's thresholds (LC3 Test Software, liblc3's reports).
const MAX_ABS: f64 = 0.00148;
const RMS_DB: f64 = -89.06;

fn wav(sr: u32) -> Vec<i16> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.cache/lc3plus/wav");
    let path = dir.join(format!("thetest{}.wav", sr / 1000));
    let d = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("{}: {e} (run `just lc3plus`)", path.display()));
    let u32_at = |i: usize| u32::from_le_bytes(d[i..i + 4].try_into().unwrap());
    let u16_at = |i: usize| u16::from_le_bytes(d[i..i + 2].try_into().unwrap());
    assert_eq!(&d[..4], b"RIFF");
    let mut i = 12;
    let mut fmt_ok = false;
    while i + 8 <= d.len() {
        let size = u32_at(i + 4) as usize;
        match &d[i..i + 4] {
            b"fmt " => {
                // PCM, mono, 16-bit, at sr
                fmt_ok = (
                    u16_at(i + 8),
                    u16_at(i + 10),
                    u32_at(i + 12),
                    u16_at(i + 22),
                ) == (1, 1, sr, 16);
            }
            b"data" => {
                assert!(fmt_ok, "{}: not 16-bit mono PCM at {sr} Hz", path.display());
                return d[i + 8..i + 8 + size]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|&b| i16::from_le_bytes(b))
                    .collect();
            }
            _ => {}
        }
        i += 8 + size + (size & 1);
    }
    panic!("{}: no data chunk", path.display());
}

/// liblc3's encoder over `pcm` at `sr_pcm`, coded at `sr`: frames of `nbytes`; the Rust
/// encoder's frames asserted byte-identical.
fn encode(dt: i32, sr: u32, sr_pcm: u32, nbytes: usize, pcm: &[i16], at: &str) -> Vec<Vec<u8>> {
    let ns = unsafe { lc3_frame_samples(dt, sr_pcm as i32) } as usize;
    let mut mem = Mem::new(unsafe { lc3_encoder_size(dt, sr_pcm as i32) });
    let enc = unsafe { lc3_setup_encoder(dt, sr as i32, sr_pcm as i32, mem.ptr()) };
    let mut rust = Encoder::new(dt as u32, sr, sr_pcm).unwrap();
    let mut o = vec![0u8; nbytes];
    pcm.chunks_exact(ns)
        .enumerate()
        .map(|(i, x)| {
            let mut out = vec![0u8; nbytes];
            let rc = unsafe {
                lc3_encode(
                    enc,
                    LC3_PCM_FORMAT_S16,
                    x.as_ptr().cast(),
                    1,
                    nbytes as i32,
                    out.as_mut_ptr().cast(),
                )
            };
            assert_eq!(rc, 0);
            rust.encode(x, &mut o).unwrap();
            assert_eq!(o, out, "encoder {at} frame {i}");
            out
        })
        .collect()
}

#[derive(Default)]
struct Tally {
    frames: usize,
    concealed: usize,
    max_abs: f64,
    err2: f64,
    samples: usize,
}

/// Decodes `frames` (`None`: lost) with both decoders, 16-bit and float; asserts bit-exactness.
fn decode(dt: i32, sr: u32, sr_pcm: u32, frames: &[Option<&[u8]>], at: &str, t: &mut Tally) {
    let ns = unsafe { lc3_frame_samples(dt, sr_pcm as i32) } as usize;
    let c_dec =
        |mem: &mut Mem| unsafe { lc3_setup_decoder(dt, sr as i32, sr_pcm as i32, mem.ptr()) };
    let size = unsafe { lc3_decoder_size(dt, sr_pcm as i32) };
    let (mut m16, mut mf) = (Mem::new(size), Mem::new(size));
    let (c16, cf) = (c_dec(&mut m16), c_dec(&mut mf));
    let mut r16 = Decoder::new(dt as u32, sr, sr_pcm).unwrap();
    let mut rf = Decoder::new(dt as u32, sr, sr_pcm).unwrap();
    let (mut o16, mut p16, mut of, mut pf) = (
        vec![0i16; ns],
        vec![0i16; ns],
        vec![0f32; ns],
        vec![0f32; ns],
    );
    for (i, &frame) in frames.iter().enumerate() {
        let (input, nbytes): (*const c_void, i32) = frame.map_or((std::ptr::null(), 0), |f| {
            (f.as_ptr().cast(), f.len() as i32)
        });
        let rc16 = unsafe {
            lc3_decode(
                c16,
                input,
                nbytes,
                LC3_PCM_FORMAT_S16,
                p16.as_mut_ptr().cast(),
                1,
            )
        };
        let rcf = unsafe {
            lc3_decode(
                cf,
                input,
                nbytes,
                LC3_PCM_FORMAT_FLOAT,
                pf.as_mut_ptr().cast(),
                1,
            )
        };
        let s16 = r16.decode(frame, &mut o16).unwrap();
        let sf = rf.decode_f32(frame, &mut of).unwrap();
        let at = format!("{at} frame {i}");
        assert_eq!((s16 == Status::Concealed) as i32, rc16, "status {at}");
        assert_eq!((sf == Status::Concealed) as i32, rcf, "status {at}");
        assert_eq!(o16, p16, "s16 {at}");
        for (&a, &b) in of.iter().zip(&pf) {
            assert_eq!(a.to_bits(), b.to_bits(), "float {at}");
            let e = (f64::from(a) - f64::from(b)).abs();
            t.max_abs = t.max_abs.max(e);
            t.err2 += e * e;
        }
        t.samples += ns;
        t.frames += 1;
        t.concealed += rc16 as usize;
    }
}

#[test]
fn encoder_on_sig_operating_points() {
    let inputs: Vec<(u32, Vec<i16>)> = [8000, 16000, 24000, 32000, 48000]
        .into_iter()
        .map(|sr| (sr, wav(sr)))
        .collect();
    let mut lines = Vec::new();
    for dt in [2500, 5000, 7500, 10000] {
        let (mut frames, mut ops) = (0, 0);
        for (sr, br10, br7m5) in OPS {
            let br = if dt == 7500 { br7m5 } else { br10 };
            let nbytes = unsafe { lc3_frame_bytes(dt, br) }.clamp(20, 400) as usize;
            for sr_pcm in [sr, 48000] {
                let pcm = &inputs.iter().find(|(s, _)| *s == sr_pcm).unwrap().1;
                let at = format!("dt {dt} sr {sr_pcm}->{sr} {br} b/s ({nbytes} bytes)");
                frames += encode(dt, sr, sr_pcm, nbytes, pcm, &at).len();
                if sr == 48000 {
                    break;
                }
            }
            ops += 1;
        }
        lines.push(format!(
            "lc3 encoder {:>4.1} ms: {ops} SIG operating points x (PCM at the coded rate, at 48 kHz), \
             {frames} frames byte-identical with liblc3",
            dt as f64 / 1000.0,
        ));
    }
    for l in lines {
        eprintln!("{l}");
    }
}

#[test]
fn decoder_on_sig_operating_points() {
    let inputs: Vec<(u32, Vec<i16>)> = [8000, 16000, 24000, 32000, 48000]
        .into_iter()
        .map(|sr| (sr, wav(sr)))
        .collect();
    let mut lines = Vec::new();
    for dt in [2500, 5000, 7500, 10000] {
        let mut t = Tally::default();
        let mut ops = 0;
        for (sr, br10, br7m5) in OPS {
            let pcm = &inputs.iter().find(|(s, _)| *s == sr).unwrap().1;
            let br = if dt == 7500 { br7m5 } else { br10 };
            let nbytes = unsafe { lc3_frame_bytes(dt, br) }.clamp(20, 400) as usize;
            let at = format!("dt {dt} sr {sr} {br} b/s ({nbytes} bytes)");
            let frames = encode(dt, sr, sr, nbytes, pcm, &at);
            let clean: Vec<Option<&[u8]>> = frames.iter().map(|f| Some(&f[..])).collect();
            // losses: bursts of 3 frames, about 1 frame in 10
            let lossy: Vec<Option<&[u8]>> = frames
                .iter()
                .enumerate()
                .map(|(i, f)| (i / 3 * 7919 % 97 >= 10).then_some(&f[..]))
                .collect();
            let at = format!("dt {dt} sr {sr} {br} b/s ({nbytes} bytes)");
            decode(dt, sr, sr, &clean, &at, &mut t);
            decode(dt, sr, 48000, &clean, &format!("{at} -> 48k"), &mut t);
            decode(dt, sr, sr, &lossy, &format!("{at} lossy"), &mut t);
            ops += 1;
        }
        let rms_db = 10.0 * (t.err2 / t.samples as f64).log10();
        assert!(t.max_abs <= MAX_ABS && rms_db <= RMS_DB);
        assert!(t.concealed > 0);
        lines.push(format!(
            "lc3 decoder {:>4.1} ms: {ops} SIG operating points x (clean, 48 kHz PCM, lossy), \
             {} frames ({} concealed) bit-exact vs liblc3 (s16 + float); max abs diff {} \
             (<= {MAX_ABS}), RMS {rms_db} dB (<= {RMS_DB})",
            dt as f64 / 1000.0,
            t.frames,
            t.concealed,
            t.max_abs,
        ));
    }
    for l in lines {
        eprintln!("{l}");
    }
}
