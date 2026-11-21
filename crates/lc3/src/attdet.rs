//! Time-domain attack detector (liblc3 `attdet.c`): block energies of the high-passed,
//! decimated signal against a decaying peak (32 and 48 kHz, 7.5 and 10 ms, at some bitrates).

use crate::common::{DT_7M5, SRATE_32K};

/// `lc3_attdet_analysis_t`.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct AttDet {
    en1: i32,
    an1: i32,
    p_att: i32,
}

/// `lc3_attdet_run`: whether an attack is detected; `x` the PCM ring at `xt` (the current
/// frame, after 6 samples of history).
pub(crate) fn run(
    dt: usize,
    sr: usize,
    nbytes: i32,
    att: &mut AttDet,
    x: &[i16],
    xt: usize,
) -> bool {
    // [dt - 7.5 ms][sr - 32 kHz]: frame sizes enabling the detector
    const NBYTES_RANGES: [[[i32; 2]; 2]; 2] =
        [[[61, 149], [75, 149]], [[81, i32::MAX], [100, i32::MAX]]];
    if dt < DT_7M5 || sr < SRATE_32K {
        return false;
    }
    let [lo, hi] = NBYTES_RANGES[dt - DT_7M5][sr - SRATE_32K];
    if nbytes < lo || nbytes > hi {
        return false;
    }

    let nblk = 4 - usize::from(dt == DT_7M5);
    let mut e = [0i32; 4];
    let mut p = xt;
    // decimated to 16 kHz: the mean of 2 samples (32 kHz) or 3 (48 kHz, as `>> 2`)
    let m = 2 + usize::from(sr > SRATE_32K);
    let dec = |i: usize| (x[i..i + m].iter().map(|&v| i32::from(v)).sum::<i32>() >> (m - 1)) as i16;
    for ei in e.iter_mut().take(nblk) {
        let (mut xn2, mut xn1) = (dec(p - 2 * m), dec(p - m));
        for _ in 0..40 {
            let xn = dec(p);
            let xf = ((3 * i32::from(xn) - 4 * i32::from(xn1) + i32::from(xn2)) >> 3) as i16;
            *ei += (i32::from(xf) * i32::from(xf)) >> 5;
            p += m;
            (xn2, xn1) = (xn1, xn);
        }
    }

    // the attack block, as the normative value + 1 (0: none)
    let mut p_att = 0;
    for (i, &ei) in e.iter().enumerate().take(nblk) {
        let a = (att.an1 >> 2).max(att.en1);
        (att.en1, att.an1) = (ei, a);
        if (ei >> 3) > a + (a >> 4) {
            p_att = i as i32 + 1;
        }
    }
    let detected = att.p_att > (nblk as i32 >> 1) || p_att > 0; // `>= 1 + nblk/2`
    att.p_att = p_att;
    detected
}
