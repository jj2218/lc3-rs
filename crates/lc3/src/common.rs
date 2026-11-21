//! Frame geometry (liblc3 `tables.h`, `common.h`): durations and rates as the C enums' indices.

use crate::tables::{LC3_NE_2M5, LC3_NS_2M5, LC3_NS_4M};

/// `enum lc3_dt`: 2.5, 5, 7.5 and 10 ms.
pub(crate) const DT_2M5: usize = 0;
pub(crate) const DT_5M: usize = 1;
pub(crate) const DT_7M5: usize = 2;
pub(crate) const DT_10M: usize = 3;

/// `LC3_SRATE_32K` of `enum lc3_srate`: 8, 16, 24, 32, 48 kHz as 0..=4 (the high-resolution
/// rates are not ported); `LC3_BANDWIDTH_SWB` of `enum lc3_bandwidth`, the same indices.
pub(crate) const SRATE_32K: usize = 3;
pub(crate) const BANDWIDTH_SWB: usize = 3;

/// The frame durations of `enum lc3_dt`, in µs.
pub(crate) const FRAME_US: [u32; 4] = [2500, 5000, 7500, 10000];
/// The sample rates of `enum lc3_srate` (no high-resolution mode), in Hz.
pub(crate) const SAMPLE_RATES: [u32; 5] = [8000, 16000, 24000, 32000, 48000];

/// `LC3_MAX_NS` without the high-resolution rates: 10 ms at 48 kHz.
pub(crate) const MAX_NS: usize = 480;

/// `lc3_ns`: samples (and coefficients) per frame.
pub(crate) fn ns(dt: usize, sr: usize) -> usize {
    LC3_NS_2M5[sr] * (1 + dt)
}

/// `lc3_ne`: encoded coefficients per frame.
pub(crate) fn ne(dt: usize, sr: usize) -> usize {
    LC3_NE_2M5[sr] * (1 + dt)
}

/// `lc3_nd`: MDCT delayed samples (half a frame plus the 1.25 ms, or 2 ms at 7.5 ms, overlap).
pub(crate) fn nd(dt: usize, sr: usize) -> usize {
    (ns(dt, sr)
        + if dt == DT_7M5 {
            LC3_NS_4M[sr]
        } else {
            LC3_NS_2M5[sr]
        })
        >> 1
}

/// `lc3_nh`: 18 ms of history, aligned on a frame.
pub(crate) fn nh(dt: usize, sr: usize) -> usize {
    (8 + usize::from(dt == DT_7M5)) * LC3_NS_2M5[sr]
}
