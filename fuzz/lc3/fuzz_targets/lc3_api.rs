//! The public API on any input, no C oracle: any configuration (valid ones build, invalid ones
//! are the matching error), the encoder on any 16-bit or float bit pattern (NaN, ±inf,
//! subnormals) into frames of 0 to 511 bytes, the decoder on any bytes, lost-frame runs and
//! corrupted own frames, PCM buffers short or long. No panic; frame-size and PCM-length errors
//! exactly where the API says; float output within [-1, 1].
#![no_main]

use lc3::{Decoder, Encoder, Error, MAX_FRAME_BYTES, MIN_FRAME_BYTES};
use libfuzzer_sys::fuzz_target;

const DTS: [u32; 9] = [2500, 5000, 7500, 10000, 0, 1, 2499, 10001, u32::MAX];
const SRS: [u32; 9] = [8000, 16000, 24000, 32000, 48000, 0, 44100, 96000, u32::MAX];

fn frame_ok(len: usize) -> bool {
    (MIN_FRAME_BYTES..=MAX_FRAME_BYTES).contains(&len)
}

fuzz_target!(|data: &[u8]| {
    let [a, b, c, rest @ ..] = data else { return };
    let (dt, sr, sp) = (
        DTS[*a as usize % 9],
        SRS[*b as usize % 9],
        SRS[*c as usize % 9],
    );
    let (e, d) = (Encoder::new(dt, sr, sp), Decoder::new(dt, sr, sp));
    let want = if !DTS[..4].contains(&dt) {
        Err(Error::InvalidFrameDuration(dt))
    } else if !SRS[..5].contains(&sr) {
        Err(Error::InvalidSampleRate(sr))
    } else if !SRS[..5].contains(&sp) {
        Err(Error::InvalidSampleRate(sp))
    } else if sr > sp {
        Err(Error::PcmRateBelowSampleRate {
            sample_rate: sr,
            pcm_rate: sp,
        })
    } else {
        Ok(())
    };
    assert_eq!(e.as_ref().map(|_| ()).map_err(|e| *e), want);
    assert_eq!(d.as_ref().map(|_| ()).map_err(|e| *e), want);
    let (Ok(mut enc), Ok(mut dec)) = (e, d) else {
        return;
    };
    let ns = enc.frame_samples();
    assert_eq!(ns, dec.frame_samples());
    let pcm_err = |len: usize| Error::PcmLength {
        expected: ns,
        actual: len,
    };
    let mut rest = rest;
    let mut last = Vec::new();
    while let [op, l0, l1, tail @ ..] = rest {
        let len = (u16::from_le_bytes([*l0, *l1]) & 0x1ff) as usize; // 0..=511
        // PCM buffers: mostly a frame and a few more, sometimes one short
        let n = if op & 0x80 != 0 {
            ns - 1
        } else {
            ns + (*op as usize >> 4 & 7)
        };
        match op % 6 {
            0 | 1 => {
                let k = if op % 6 == 0 { 2 } else { 4 };
                let take = (k * ns).min(tail.len());
                let (src, next) = tail.split_at(take);
                rest = next;
                let src = if src.len() < k { &[0u8; 4][..k] } else { src };
                let mut out = vec![0u8; len];
                let r = if k == 2 {
                    let m = src.len() & !1;
                    let pcm: Vec<i16> = (0..n)
                        .map(|i| {
                            let j = (2 * i) % m;
                            i16::from_le_bytes([src[j], src[j + 1]])
                        })
                        .collect();
                    enc.encode(&pcm, &mut out)
                } else {
                    let m = src.len() & !3;
                    let pcm: Vec<f32> = (0..n)
                        .map(|i| {
                            let j = (4 * i) % m;
                            f32::from_le_bytes(src[j..j + 4].try_into().unwrap())
                        })
                        .collect();
                    enc.encode_f32(&pcm, &mut out)
                };
                let want = if !frame_ok(len) {
                    Err(Error::FrameSize(len))
                } else if n < ns {
                    Err(pcm_err(n))
                } else {
                    Ok(())
                };
                assert_eq!(r, want);
                if r.is_ok() {
                    last = out;
                }
            }
            2 | 3 => {
                let take = len.min(tail.len());
                let (frame, next) = tail.split_at(take);
                rest = next;
                let r = if op % 6 == 2 {
                    dec.decode(Some(frame), &mut vec![0i16; n])
                } else {
                    let mut pcm = vec![0f32; n];
                    let r = dec.decode_f32(Some(frame), &mut pcm);
                    assert!(
                        pcm.iter().all(|v| (-1.0..=1.0).contains(v)),
                        "f32 out of range"
                    );
                    r
                };
                let want_err = if !frame_ok(frame.len()) {
                    Some(Error::FrameSize(frame.len()))
                } else {
                    (n < ns).then(|| pcm_err(n))
                };
                assert_eq!(r.err(), want_err);
            }
            4 => {
                rest = tail;
                let mut pcm = vec![0i16; ns];
                for _ in 0..=(len % 64) {
                    dec.decode(None, &mut pcm).unwrap();
                }
            }
            _ => {
                rest = tail;
                if !last.is_empty() {
                    let i = len % last.len();
                    last[i] ^= *l1;
                    let mut pcm = vec![0f32; ns];
                    dec.decode_f32(Some(&last), &mut pcm).unwrap();
                }
            }
        }
    }
});
