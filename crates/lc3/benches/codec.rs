//! Codec speed: Rust vs liblc3 1.1.3 as distributions build it (`-O3 -ffast-math`, LTO: "c") and
//! the strict-IEEE oracle build ("strict"), on the ETSI LC3plus items (`just lc3plus`), 16-bit
//! PCM, per operating point: 16 kHz / 7.5 ms at 32 kbit/s (SIG speech) and 48 kHz / 10 ms at
//! 96 kbit/s (SIG music). The decoder decodes the strict build's frames (the Rust encoder's).
//! `BENCH=dec|enc`: one side; `BENCH_MODE=16k7.5|48k10`: one point. Run via
//! `CRATE=lc3 just bench` (pins one core). Prints µs/frame (best of BENCH_REPS, default 15).
#![allow(unsafe_code)] // FFI to the C baseline

use lc3::{Decoder, Encoder};
use liblc3_sys::{Api, FAST, LC3_PCM_FORMAT_S16, Mem, STRICT};
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

/// (name, frame µs, sample rate, bitrate).
const POINTS: [(&str, u32, u32, u32); 2] = [
    ("16k7.5", 7500, 16000, 32000),
    ("48k10", 10000, 48000, 96000),
];

fn reps() -> usize {
    std::env::var("BENCH_REPS")
        .ok()
        .and_then(|r| r.parse().ok())
        .unwrap_or(15)
}

fn wanted(name: &str) -> bool {
    std::env::var("BENCH_MODE").map_or(true, |m| m == name)
}

fn best<F: FnMut()>(mut f: F) -> f64 {
    (0..reps())
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed().as_secs_f64()
        })
        .fold(f64::INFINITY, f64::min)
}

/// `thetest<sr/1000>.wav`'s 16-bit mono samples.
fn wav(sr: u32) -> Vec<i16> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../.cache/lc3plus/wav/thetest{}.wav", sr / 1000));
    let d = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("{}: {e} (run `just lc3plus`)", path.display()));
    let mut i = 12;
    while i + 8 <= d.len() {
        let size = u32::from_le_bytes(d[i + 4..i + 8].try_into().unwrap()) as usize;
        if &d[i..i + 4] == b"data" {
            return d[i + 8..i + 8 + size]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&b| i16::from_le_bytes(b))
                .collect();
        }
        i += 8 + size + (size & 1);
    }
    panic!("{}: no data chunk", path.display());
}

/// liblc3's encoder over `pcm`: the frames.
fn c_encode(api: &Api, dt: u32, sr: u32, nbytes: usize, pcm: &[i16]) -> Vec<Vec<u8>> {
    // SAFETY: memory of the size liblc3 asks for; pcm holds whole frames; out holds nbytes.
    unsafe {
        let mut mem = Mem::new((api.encoder_size)(dt as i32, sr as i32));
        let e = (api.setup_encoder)(dt as i32, sr as i32, sr as i32, mem.ptr());
        assert!(!e.is_null(), "liblc3 rejects {dt} us at {sr} Hz");
        let ns = (dt * sr / 1_000_000) as usize;
        pcm.chunks_exact(ns)
            .map(|f| {
                let mut o = vec![0u8; nbytes];
                let r = (api.encode)(
                    e,
                    LC3_PCM_FORMAT_S16,
                    f.as_ptr().cast(),
                    1,
                    nbytes as i32,
                    o.as_mut_ptr().cast(),
                );
                assert_eq!(r, 0, "lc3_encode");
                o
            })
            .collect()
    }
}

/// liblc3's encoder over `pcm`, one output buffer (timed as the Rust loop).
fn c_encode_run(api: &Api, dt: u32, sr: u32, nbytes: usize, pcm: &[i16]) {
    // SAFETY: as `c_encode`.
    unsafe {
        let mut mem = Mem::new((api.encoder_size)(dt as i32, sr as i32));
        let e = (api.setup_encoder)(dt as i32, sr as i32, sr as i32, mem.ptr());
        assert!(!e.is_null(), "liblc3 rejects {dt} us at {sr} Hz");
        let ns = (dt * sr / 1_000_000) as usize;
        let mut o = vec![0u8; nbytes];
        for f in pcm.chunks_exact(ns) {
            let r = (api.encode)(
                e,
                LC3_PCM_FORMAT_S16,
                f.as_ptr().cast(),
                1,
                nbytes as i32,
                o.as_mut_ptr().cast(),
            );
            debug_assert_eq!(r, 0);
            black_box(black_box(r));
            black_box(&o);
        }
    }
}

/// liblc3's decoder over `frames`.
fn c_decode(api: &Api, dt: u32, sr: u32, frames: &[Vec<u8>]) {
    // SAFETY: as `c_encode`; pcm holds one frame.
    unsafe {
        let mut mem = Mem::new((api.decoder_size)(dt as i32, sr as i32));
        let d = (api.setup_decoder)(dt as i32, sr as i32, sr as i32, mem.ptr());
        assert!(!d.is_null(), "liblc3 rejects {dt} us at {sr} Hz");
        let mut pcm = [0i16; 480];
        for f in frames {
            let r = (api.decode)(
                d,
                f.as_ptr().cast(),
                f.len() as i32,
                LC3_PCM_FORMAT_S16,
                pcm.as_mut_ptr().cast(),
                1,
            );
            black_box(black_box(r));
            black_box(&pcm);
        }
    }
}

/// One row: µs/frame of Rust, the distribution build ("n/a" where it isn't built) and the
/// strict build, and the speedups.
fn row(name: &str, t: [f64; 3], frames: usize) {
    let u = t.map(|v| v / frames as f64 * 1e6);
    let c = if u[1].is_nan() {
        format!("{:>9} {:>8}", "n/a", "n/a")
    } else {
        format!("{:>9.3} {:>7.2}x", u[1], u[1] / u[0])
    };
    println!(
        "{name:<7} {:>9.3} {c} {:>9.3} {:>7.2}x",
        u[0],
        u[2],
        u[2] / u[0]
    );
}

fn side(enc: bool) {
    println!(
        "{} {:>9} {:>9} {:>8} {:>9} {:>8}",
        if enc { "encoder" } else { "decoder" },
        "rust µs",
        "c µs",
        "speedup",
        "strict µs",
        "speedup"
    );
    let (mut tot, mut tn) = ([0.0; 3], 0);
    for &(name, dt, sr, bitrate) in POINTS.iter().filter(|p| wanted(p.0)) {
        let nbytes = (bitrate * dt / 8_000_000) as usize;
        let pcm = wav(sr);
        let frames = c_encode(&STRICT, dt, sr, nbytes, &pcm);
        let ns = (dt * sr / 1_000_000) as usize;
        let t = if enc {
            [
                best(|| {
                    let mut e = Encoder::new(dt, sr, sr).unwrap();
                    let mut o = vec![0u8; nbytes];
                    for f in pcm.chunks_exact(ns) {
                        e.encode(f, &mut o).unwrap();
                        black_box(&o);
                    }
                }),
                FAST.map_or(f64::NAN, |api| {
                    best(|| c_encode_run(&api, dt, sr, nbytes, &pcm))
                }),
                best(|| c_encode_run(&STRICT, dt, sr, nbytes, &pcm)),
            ]
        } else {
            [
                best(|| {
                    let mut d = Decoder::new(dt, sr, sr).unwrap();
                    let mut o = [0i16; 480];
                    for f in &frames {
                        d.decode(Some(f), &mut o[..ns]).unwrap();
                        black_box(&o);
                    }
                }),
                FAST.map_or(f64::NAN, |api| best(|| c_decode(&api, dt, sr, &frames))),
                best(|| c_decode(&STRICT, dt, sr, &frames)),
            ]
        };
        row(name, t, frames.len());
        tn += frames.len();
        for i in 0..3 {
            tot[i] += t[i];
        }
    }
    if std::env::var("BENCH_MODE").is_err() {
        row("all", tot, tn);
        println!("({tn} frames/rep, best of {})", reps());
    }
}

fn main() {
    if let Ok(m) = std::env::var("BENCH_MODE")
        && !POINTS.iter().any(|p| p.0 == m)
    {
        let names: Vec<_> = POINTS.iter().map(|p| p.0).collect();
        eprintln!(
            "bench: unknown BENCH_MODE '{m}' (points: {})",
            names.join(" ")
        );
        std::process::exit(2);
    }
    let which = std::env::var("BENCH").unwrap_or_default();
    if !["", "dec", "enc"].contains(&which.as_str()) {
        eprintln!("bench: unknown BENCH '{which}' (dec, enc; unset: both)");
        std::process::exit(2);
    }
    if which != "enc" {
        side(false);
    }
    if which != "dec" {
        side(true);
    }
}
