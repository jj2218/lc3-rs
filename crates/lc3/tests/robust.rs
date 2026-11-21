//! Hostile input through the public API (the test profile keeps overflow checks on): invalid
//! configurations, frame sizes and PCM buffers are errors that leave the codec unchanged; NaN,
//! ±inf, huge and subnormal samples and garbage frames never panic. Needs no C oracle.

use lc3::{Decoder, Encoder, Error, MAX_FRAME_BYTES, MIN_FRAME_BYTES, Status};

const FRAME_US: [u32; 4] = [2500, 5000, 7500, 10000];
const RATES: [u32; 5] = [8000, 16000, 24000, 32000, 48000];

/// Every configuration the API accepts: (frame_us, sample_rate, pcm_rate).
fn configs() -> impl Iterator<Item = (u32, u32, u32)> {
    FRAME_US.into_iter().flat_map(|us| {
        RATES.into_iter().flat_map(move |sr| {
            RATES
                .into_iter()
                .filter(move |&p| p >= sr)
                .map(move |p| (us, sr, p))
        })
    })
}

/// xorshift64.
fn next(s: &mut u64) -> u64 {
    *s ^= *s << 13;
    *s ^= *s >> 7;
    *s ^= *s << 17;
    *s
}

#[test]
fn invalid_arguments_are_rejected() {
    assert_eq!(
        Encoder::new(2000, 48000, 48000).unwrap_err(),
        Error::InvalidFrameDuration(2000)
    );
    assert_eq!(
        Decoder::new(10000, 44100, 48000).unwrap_err(),
        Error::InvalidSampleRate(44100)
    );
    assert_eq!(
        Encoder::new(10000, 48000, 96000).unwrap_err(),
        Error::InvalidSampleRate(96000)
    );
    assert_eq!(
        Decoder::new(10000, 48000, 16000).unwrap_err(),
        Error::PcmRateBelowSampleRate {
            sample_rate: 48000,
            pcm_rate: 16000
        }
    );
    assert_eq!(configs().count(), 60);
    for (us, sr, p) in configs() {
        assert!(Encoder::new(us, sr, p).is_ok() && Decoder::new(us, sr, p).is_ok());
    }

    let mut enc = Encoder::new(10000, 48000, 48000).unwrap();
    let mut dec = Decoder::new(10000, 48000, 48000).unwrap();
    let ns = enc.frame_samples();
    let (pcm, pcmf) = (vec![1000i16; ns], vec![0.03f32; ns]);
    for n in [0, MIN_FRAME_BYTES - 1, MAX_FRAME_BYTES + 1, 1000] {
        let mut out = vec![0u8; n];
        assert_eq!(enc.encode(&pcm, &mut out), Err(Error::FrameSize(n)));
        assert_eq!(enc.encode_f32(&pcmf, &mut out), Err(Error::FrameSize(n)));
        let mut o16 = vec![0i16; ns];
        let mut of = vec![0f32; ns];
        assert_eq!(dec.decode(Some(&out), &mut o16), Err(Error::FrameSize(n)));
        assert_eq!(
            dec.decode_f32(Some(&out), &mut of),
            Err(Error::FrameSize(n))
        );
    }
    let short = Error::PcmLength {
        expected: ns,
        actual: ns - 1,
    };
    let mut out = [0u8; 100];
    assert_eq!(enc.encode(&pcm[1..], &mut out), Err(short));
    assert_eq!(enc.encode_f32(&pcmf[1..], &mut out), Err(short));
    assert_eq!(dec.decode(None, &mut vec![0i16; ns - 1]), Err(short));
    assert_eq!(dec.decode_f32(None, &mut vec![0f32; ns - 1]), Err(short));
}

/// A rejected call leaves the codec as it was: the next frames are those of a fresh codec.
#[test]
fn errors_leave_the_codec_unchanged() {
    let mut s = 1u64;
    let (mut a, mut b) = (
        Encoder::new(7500, 16000, 48000).unwrap(),
        Encoder::new(7500, 16000, 48000).unwrap(),
    );
    let (mut da, mut db) = (
        Decoder::new(7500, 16000, 48000).unwrap(),
        Decoder::new(7500, 16000, 48000).unwrap(),
    );
    let ns = a.frame_samples();
    for f in 0..20 {
        let pcm: Vec<i16> = (0..ns).map(|_| next(&mut s) as i16 >> 2).collect();
        if f % 3 == 1 {
            assert!(a.encode(&pcm, &mut [0u8; 19]).is_err());
            assert!(a.encode(&pcm[..ns - 1], &mut [0u8; 60]).is_err());
            assert!(da.decode(None, &mut [0i16; 3]).is_err());
            assert!(da.decode(Some(&[0u8; 401]), &mut vec![0i16; ns]).is_err());
        }
        let (mut fa, mut fb) = ([0u8; 60], [0u8; 60]);
        a.encode(&pcm, &mut fa).unwrap();
        b.encode(&pcm, &mut fb).unwrap();
        assert_eq!(fa, fb, "frame {f}");
        let frame = (f % 5 != 4).then_some(&fa[..]);
        let (mut oa, mut ob) = (vec![0f32; ns], vec![0f32; ns]);
        da.decode_f32(frame, &mut oa).unwrap();
        db.decode_f32(frame, &mut ob).unwrap();
        assert_eq!(oa, ob, "frame {f}");
    }
}

#[test]
fn hostile_input_never_panics() {
    let specials = [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::MAX,
        -f32::MAX,
        1e30,
        65536.0,
        -2.0,
        1e-40,
        -0.0,
        f32::MIN_POSITIVE,
    ];
    let mut s = 0x1c3_5eedu64;
    for (us, sr, p) in configs() {
        let mut enc = Encoder::new(us, sr, p).unwrap();
        let mut dec = Decoder::new(us, sr, p).unwrap();
        let ns = enc.frame_samples();
        for (f, &v) in specials.iter().enumerate() {
            let pcm: Vec<f32> = (0..ns)
                .map(|i| match (f + i) % 4 {
                    0 => v,
                    1 => -v,
                    2 => f32::from_bits(next(&mut s) as u32),
                    _ => 0.5,
                })
                .collect();
            let n = [20, 57, 155, 400][f % 4];
            let mut frame = vec![0u8; n];
            enc.encode_f32(&pcm, &mut frame).unwrap();
            let pcm16: Vec<i16> = (0..ns).map(|i| [32767, -32768][(i / 3) % 2]).collect();
            enc.encode(&pcm16, &mut frame).unwrap();
            let mut out = vec![0f32; ns];
            dec.decode_f32(Some(&frame), &mut out).unwrap();
            assert!(out.iter().all(|x| (-1.0..=1.0).contains(x)));
            for fill in [0x00, 0xff, next(&mut s) as u8] {
                let mut garbage = vec![fill; n];
                garbage[n / 2] ^= next(&mut s) as u8;
                dec.decode(Some(&garbage), &mut vec![0i16; ns + 7]).unwrap();
            }
        }
        for _ in 0..30 {
            dec.decode(None, &mut vec![0i16; ns]).unwrap();
        }
    }
}

#[test]
fn errors_display() {
    let msgs = [
        (Error::InvalidFrameDuration(1), "frame duration 1 µs"),
        (Error::InvalidSampleRate(44100), "sample rate 44100 Hz"),
        (
            Error::PcmRateBelowSampleRate {
                sample_rate: 48000,
                pcm_rate: 8000,
            },
            "PCM rate 8000 Hz is below the coded rate 48000 Hz",
        ),
        (Error::FrameSize(19), "frame of 19 bytes, not 20 to 400"),
        (
            Error::PcmLength {
                expected: 480,
                actual: 10,
            },
            "PCM buffer of 10 samples, a frame needs 480",
        ),
    ];
    for (e, m) in msgs {
        assert!(e.to_string().contains(m), "{e}");
        let boxed: Box<dyn std::error::Error + Send + Sync> = Box::new(e);
        assert!(boxed.source().is_none());
    }
}

#[test]
fn debug_is_short() {
    let enc = Encoder::new(10000, 16000, 48000).unwrap();
    let dec = Decoder::new(7500, 48000, 48000).unwrap();
    assert_eq!(
        format!("{enc:?}"),
        "Encoder { frame_us: 10000, sample_rate: 16000, pcm_rate: 48000, .. }"
    );
    assert_eq!(
        format!("{dec:?}"),
        "Decoder { frame_us: 7500, sample_rate: 48000, pcm_rate: 48000, .. }"
    );
    assert_eq!(format!("{:?}", Status::Concealed), "Concealed");
}
