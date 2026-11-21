//! The decoder against liblc3's (strict-IEEE build, `liblc3-sys`): streams from liblc3's
//! encoder over every frame duration × sample rate × PCM rate (every pair, PCM at or above), bitrates from the smallest to
//! the largest frame, a speech-like signal with pitch (LTPF), silence and attacks; bursts of
//! 1-12 lost frames (every concealment attenuation step) and corrupted frames (bit flips: the
//! error paths). Float and 16-bit output
//! compared bit for bit, every frame.
#![allow(unsafe_code)]

use lc3::{Decoder, Encoder, Status};
use liblc3_sys::*;
use std::ffi::c_void;

/// The sample rates (Hz).
const RATES: [u32; 5] = [8000, 16000, 24000, 32000, 48000];

/// xorshift64*, as the crate's test `Rng`.
struct Rng(u64);
impl Rng {
    fn u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn unit(&mut self) -> f32 {
        (self.u64() >> 40) as f32 / (1u64 << 23) as f32 - 1.0
    }
}

/// 16-bit PCM at `sr`: voiced segments (harmonics of a gliding pitch), noise, silence,
/// clicks, band-limited noise (a cutoff from 2 kHz up, for the bandwidth detector), a tone
/// near Nyquist, a lone full-scale impulse in silence (TNS's strongest reflection
/// coefficients) and full-scale squares (±32768, DC to Nyquist), changing every 80 ms, at
/// levels from quiet to clipping.
fn signal(r: &mut Rng, sr: u32, n: usize) -> Vec<i16> {
    let seg = sr as usize * 8 / 100;
    let (mut kind, mut f0, mut amp, mut ph) = (0, 120.0f32, 8000.0f32, 0f32);
    // band-limited noise: 16 partials below a cutoff (2 kHz to Nyquist), as (step, phase)
    let mut partials = [(0f32, 0f32); 16];
    let mut tone = 0.45f32;
    let mut period = 2;
    (0..n)
        .map(|i| {
            if i % seg == 0 {
                kind = r.u64() % 9;
                period = 2 + (r.u64() % 400) as usize;
                let cutoff = (2000 + r.u64() % 22000).min(sr as u64 / 2) as f32 / sr as f32;
                for p in &mut partials {
                    *p = (
                        2.0 * std::f32::consts::PI * cutoff * (r.unit() + 1.0) / 2.0,
                        r.unit() * 3.0,
                    );
                }
                tone = 0.40 + 0.095 * (r.unit() + 1.0) / 2.0;
                f0 = 80.0 + (r.u64() % 300) as f32;
                amp = [300.0, 3000.0, 12000.0, 30000.0][(r.u64() % 4) as usize];
            }
            f0 *= 1.0 + 2e-6 * r.unit();
            ph += 2.0 * std::f32::consts::PI * f0 / sr as f32;
            let v = match kind {
                0 | 1 => {
                    (1..12)
                        .map(|h| (ph * h as f32).sin() / h as f32)
                        .sum::<f32>()
                        * amp
                        * 0.4
                }
                2 => r.unit() * amp,
                3 => 0.0,
                5 => {
                    let mut v = 0.0;
                    for (step, phase) in &mut partials {
                        *phase += *step;
                        v += phase.sin();
                    }
                    v * amp * 0.15
                }
                4 => {
                    if r.u64().is_multiple_of(997) {
                        amp * 3.0
                    } else {
                        r.unit() * 30.0
                    }
                }
                6 => {
                    (i as f32 * 2.0 * std::f32::consts::PI * tone).sin() * amp * 0.5
                        + r.unit() * 10.0
                }
                7 => {
                    if i % seg == seg / 3 {
                        [32767.0, -32768.0][(f0 as usize) % 2]
                    } else {
                        0.0
                    }
                }
                _ => {
                    if (i / period) % 2 == 0 {
                        32767.0
                    } else {
                        -32768.0
                    }
                }
            };
            v.clamp(-32768.0, 32767.0) as i16
        })
        .collect()
}

/// liblc3's encoded streams, with losses and bit flips, through both decoders: bit-exact.
#[test]
fn decoder_matches_liblc3() {
    let mut r = Rng(0x1c3_5eed);
    let (mut frames, mut concealed) = (0, 0);
    for dt_us in [2500, 5000, 7500, 10000] {
        for sr in RATES {
            for sr_pcm in RATES.into_iter().filter(|&p| p >= sr) {
                let ns = unsafe { lc3_frame_samples(dt_us, sr_pcm as i32) } as usize;
                let ns_enc = unsafe { lc3_frame_samples(dt_us, sr as i32) } as usize;
                let mut enc_mem = Mem::new(unsafe { lc3_encoder_size(dt_us, sr as i32) });
                let enc = unsafe { lc3_setup_encoder(dt_us, sr as i32, 0, enc_mem.ptr()) };
                let mut dec_mem = Mem::new(unsafe { lc3_decoder_size(dt_us, sr_pcm as i32) });
                let dec =
                    unsafe { lc3_setup_decoder(dt_us, sr as i32, sr_pcm as i32, dec_mem.ptr()) };
                let mut d16 = Decoder::new(dt_us as u32, sr, sr_pcm).unwrap();
                let mut df = Decoder::new(dt_us as u32, sr, sr_pcm).unwrap();
                let mut dec_mem_f = Mem::new(unsafe { lc3_decoder_size(dt_us, sr_pcm as i32) });
                let decf =
                    unsafe { lc3_setup_decoder(dt_us, sr as i32, sr_pcm as i32, dec_mem_f.ptr()) };
                let pcm = signal(&mut r, sr, ns_enc * 800);
                let mut burst = 0; // lost frames to come: bursts reach every PLC attenuation step
                for (f, chunk) in pcm.chunks_exact(ns_enc).enumerate() {
                    // bitrate sweep: every frame size from 20 to 400 bytes over the stream
                    let nbytes = 20 + (f * 7 + (r.u64() % 7) as usize) % 381;
                    let mut buf = vec![0u8; nbytes];
                    let rc = unsafe {
                        lc3_encode(
                            enc,
                            LC3_PCM_FORMAT_S16,
                            chunk.as_ptr().cast(),
                            1,
                            nbytes as i32,
                            buf.as_mut_ptr().cast(),
                        )
                    };
                    assert_eq!(rc, 0);
                    if burst == 0 && r.u64().is_multiple_of(40) {
                        burst = 1 + r.u64() % 12;
                    }
                    let lost = burst > 0;
                    burst = burst.saturating_sub(1);
                    if r.u64().is_multiple_of(17) {
                        for _ in 0..1 + r.u64() % 4 {
                            let b = (r.u64() as usize) % nbytes;
                            buf[b] ^= 1 << (r.u64() % 8);
                        }
                    }
                    let input: *const c_void = if lost {
                        std::ptr::null()
                    } else {
                        buf.as_ptr().cast()
                    };
                    let frame = (!lost).then_some(&buf[..]);

                    let (mut c16, mut cf) = (vec![0i16; ns], vec![0f32; ns]);
                    let rc16 = unsafe {
                        lc3_decode(
                            dec,
                            input,
                            nbytes as i32,
                            LC3_PCM_FORMAT_S16,
                            c16.as_mut_ptr().cast(),
                            1,
                        )
                    };
                    let rcf = unsafe {
                        lc3_decode(
                            decf,
                            input,
                            nbytes as i32,
                            LC3_PCM_FORMAT_FLOAT,
                            cf.as_mut_ptr().cast(),
                            1,
                        )
                    };
                    let (mut o16, mut of) = (vec![0i16; ns], vec![0f32; ns]);
                    let s16 = d16.decode(frame, &mut o16).unwrap();
                    let sf = df.decode_f32(frame, &mut of).unwrap();
                    let at = format!(
                        "dt {dt_us} sr {sr}->{sr_pcm} frame {f} ({nbytes} bytes, lost {lost})"
                    );
                    assert_eq!((s16 == Status::Concealed) as i32, rc16, "status {at}");
                    assert_eq!((sf == Status::Concealed) as i32, rcf, "status {at}");
                    assert_eq!(o16, c16, "s16 {at}");
                    let bits = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
                    assert_eq!(bits(&of), bits(&cf), "float {at}");
                    frames += 1;
                    concealed += rc16;
                }
            }
        }
    }
    eprintln!("lc3 decoder vs liblc3: {frames} frames bit-exact ({concealed} concealed)");
    assert!(frames > 40_000 && concealed > 4000, "{frames} {concealed}");
}

/// Random bytes as frames (every size, every configuration, 16-bit and float output): mostly
/// rejected by the bitstream checks and concealed, the rest decoded from arbitrary side data
/// and spectra.
#[test]
fn decoder_matches_liblc3_on_random_frames() {
    let mut r = Rng(0xbad_f4a3e);
    let (mut frames, mut decoded) = (0, 0);
    for dt_us in [2500, 5000, 7500, 10000] {
        for sr in RATES {
            for sr_pcm in RATES.into_iter().filter(|&p| p >= sr) {
                let ns = unsafe { lc3_frame_samples(dt_us, sr_pcm as i32) } as usize;
                let size = unsafe { lc3_decoder_size(dt_us, sr_pcm as i32) };
                let (mut m16, mut mf) = (Mem::new(size), Mem::new(size));
                let c16 = unsafe { lc3_setup_decoder(dt_us, sr as i32, sr_pcm as i32, m16.ptr()) };
                let cf = unsafe { lc3_setup_decoder(dt_us, sr as i32, sr_pcm as i32, mf.ptr()) };
                let mut d16 = Decoder::new(dt_us as u32, sr, sr_pcm).unwrap();
                let mut df = Decoder::new(dt_us as u32, sr, sr_pcm).unwrap();
                for f in 0..900 {
                    let nbytes = 20 + (r.u64() % 381) as usize;
                    let buf: Vec<u8> = (0..nbytes).map(|_| r.u64() as u8).collect();
                    let (mut c, mut o) = (vec![0i16; ns], vec![0i16; ns]);
                    let (mut cfl, mut ofl) = (vec![0f32; ns], vec![0f32; ns]);
                    let (rc, rcf) = unsafe {
                        (
                            lc3_decode(
                                c16,
                                buf.as_ptr().cast(),
                                nbytes as i32,
                                LC3_PCM_FORMAT_S16,
                                c.as_mut_ptr().cast(),
                                1,
                            ),
                            lc3_decode(
                                cf,
                                buf.as_ptr().cast(),
                                nbytes as i32,
                                LC3_PCM_FORMAT_FLOAT,
                                cfl.as_mut_ptr().cast(),
                                1,
                            ),
                        )
                    };
                    let s = d16.decode(Some(&buf), &mut o).unwrap();
                    let sf = df.decode_f32(Some(&buf), &mut ofl).unwrap();
                    let at = format!("dt {dt_us} sr {sr}->{sr_pcm} frame {f} ({nbytes} bytes)");
                    assert_eq!((s == Status::Concealed) as i32, rc, "status {at}");
                    assert_eq!((sf == Status::Concealed) as i32, rcf, "status {at}");
                    assert_eq!(o, c, "s16 {at}");
                    let bits = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
                    assert_eq!(bits(&ofl), bits(&cfl), "float {at}");
                    frames += 1;
                    decoded += 1 - rc;
                }
            }
        }
    }
    eprintln!("lc3 decoder vs liblc3, random frames: {frames} bit-exact ({decoded} decoded)");
    assert!(decoded > 1000, "{decoded} of {frames} decoded");
}

/// The encoder against liblc3's: every frame duration × sample rate × PCM rate, the signal
/// above (voiced with pitch for LTPF, noise, silence, clicks for the attack detector, levels to
/// clipping), frame sizes from 20 to 400 bytes; 16-bit and float input. Byte-identical frames.
#[test]
fn encoder_matches_liblc3() {
    let mut r = Rng(0xe1c_0de);
    let mut frames = 0;
    for dt_us in [2500, 5000, 7500, 10000] {
        for sr in RATES {
            for sr_pcm in RATES.into_iter().filter(|&p| p >= sr) {
                let ns = unsafe { lc3_frame_samples(dt_us, sr_pcm as i32) } as usize;
                let size = unsafe { lc3_encoder_size(dt_us, sr_pcm as i32) };
                let (mut m16, mut mf) = (Mem::new(size), Mem::new(size));
                let c16 = unsafe { lc3_setup_encoder(dt_us, sr as i32, sr_pcm as i32, m16.ptr()) };
                let cf = unsafe { lc3_setup_encoder(dt_us, sr as i32, sr_pcm as i32, mf.ptr()) };
                let mut e16 = Encoder::new(dt_us as u32, sr, sr_pcm).unwrap();
                let mut ef = Encoder::new(dt_us as u32, sr, sr_pcm).unwrap();
                let pcm = signal(&mut r, sr_pcm, ns * 400);
                for (f, chunk) in pcm.chunks_exact(ns).enumerate() {
                    let nbytes = 20 + (f * 7 + (r.u64() % 7) as usize) % 381;
                    let at = format!("dt {dt_us} sr {sr_pcm}->{sr} frame {f} ({nbytes} bytes)");
                    let (mut c, mut o) = (vec![0u8; nbytes], vec![0u8; nbytes]);
                    let rc = unsafe {
                        lc3_encode(
                            c16,
                            LC3_PCM_FORMAT_S16,
                            chunk.as_ptr().cast(),
                            1,
                            nbytes as i32,
                            c.as_mut_ptr().cast(),
                        )
                    };
                    assert_eq!(rc, 0);
                    e16.encode(chunk, &mut o).unwrap();
                    assert_eq!(o, c, "s16 {at}");
                    let fl: Vec<f32> = chunk
                        .iter()
                        .map(|&v| f32::from(v) / 32768.0 * 1.01)
                        .collect();
                    let rc = unsafe {
                        lc3_encode(
                            cf,
                            LC3_PCM_FORMAT_FLOAT,
                            fl.as_ptr().cast(),
                            1,
                            nbytes as i32,
                            c.as_mut_ptr().cast(),
                        )
                    };
                    assert_eq!(rc, 0);
                    ef.encode_f32(&fl, &mut o).unwrap();
                    assert_eq!(o, c, "float {at}");
                    frames += 1;
                }
            }
        }
    }
    eprintln!("lc3 encoder vs liblc3: {frames} frames byte-identical (s16 + float)");
    assert!(frames >= 24_000, "{frames}");
}

/// A pinned deviation: after ~5 s of continuous loss liblc3's float output clicks (±1.0:
/// `lc3_ldexpf(s, -15)` wraps the exponent of the fading samples below 2^-112 into huge ones of
/// the opposite sign). The Rust decoder's samples are liblc3's but there, where they are ~0.
#[test]
fn long_loss_differs_from_liblc3_only_by_its_clicks() {
    let mut r = Rng(0x10_55);
    let (mut clicks, mut equal) = (0, 0);
    for (dt_us, sr) in [(10000, 48000u32), (7500, 16000), (2500, 8000)] {
        let ns = unsafe { lc3_frame_samples(dt_us, sr as i32) } as usize;
        let mut em = Mem::new(unsafe { lc3_encoder_size(dt_us, sr as i32) });
        let enc = unsafe { lc3_setup_encoder(dt_us, sr as i32, 0, em.ptr()) };
        let mut dm = Mem::new(unsafe { lc3_decoder_size(dt_us, sr as i32) });
        let dec = unsafe { lc3_setup_decoder(dt_us, sr as i32, 0, dm.ptr()) };
        let mut d = Decoder::new(dt_us as u32, sr, sr).unwrap();
        let pcm = signal(&mut r, sr, ns * 20);
        // 7 s of loss after 20 good frames
        let lost = 7_000_000 / dt_us as usize;
        for f in 0..20 + lost {
            let mut buf = [0u8; 100];
            let input: *const c_void = if f < 20 {
                let rc = unsafe {
                    lc3_encode(
                        enc,
                        LC3_PCM_FORMAT_S16,
                        pcm[f * ns..].as_ptr().cast(),
                        1,
                        100,
                        buf.as_mut_ptr().cast(),
                    )
                };
                assert_eq!(rc, 0);
                buf.as_ptr().cast()
            } else {
                std::ptr::null()
            };
            let mut c = vec![0f32; ns];
            unsafe {
                lc3_decode(
                    dec,
                    input,
                    100,
                    LC3_PCM_FORMAT_FLOAT,
                    c.as_mut_ptr().cast(),
                    1,
                )
            };
            let mut o = vec![0f32; ns];
            d.decode_f32((f < 20).then_some(&buf[..]), &mut o).unwrap();
            for (&a, &b) in o.iter().zip(&c) {
                if a.to_bits() == b.to_bits() {
                    equal += 1;
                } else {
                    let at = format!("dt {dt_us} sr {sr} frame {f}: {a:e} vs liblc3 {b:e}");
                    assert!(f >= 20 && a.abs() < 1e-37, "{at}");
                    clicks += usize::from(b.abs() > 0.5);
                }
            }
        }
    }
    eprintln!("lc3 long loss: {equal} samples as liblc3's, {clicks} of its clicks (±1.0) avoided");
    assert!(clicks > 0 && equal > 400_000, "{clicks} {equal}");
}
