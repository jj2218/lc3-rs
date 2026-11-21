//! The only non-test module allowed to use `unsafe`: SSE2 intrinsics bodies where LLVM's lanes stay
//! poor. SSE2 is part of the x86-64 baseline, so there is no runtime check; each kernel's
//! portable body (in its module) is the contract and the differential oracle, and
//! `--cfg portable` compiles these out (`just test` runs both paths).
#![allow(unsafe_code)] // the approved scope: SIMD kernels, below

/// Σ a[i]·b[i], exact, for equal lengths that are a multiple of 16 (≤ 2^11).
#[cfg(all(target_arch = "x86_64", target_feature = "sse2", not(portable)))]
pub(crate) fn dot16(a: &[i16], b: &[i16]) -> i64 {
    assert!(a.len() == b.len() && a.len().is_multiple_of(16) && a.len() <= 1 << 11);
    // SAFETY: SSE2 is enabled (the `target_feature = "sse2"` gate; x86-64 baseline).
    unsafe { dot16_sse2(a, b) }
}

/// `ltpf::dot`'s exact sum with SSE2 `pmaddwd` (x86-64 baseline): tap pairs as
/// wrapping i32 sums, each in [−2^31 + 2^16, 2^31], so its offset from the lower end, mod
/// 2^32, is the exact pair sum as a u32; its 16-bit halves are summed apart in 32-bit lanes
/// (they fit for n ≤ 2^11). Equal to `ltpf::dot_portable`'s sum (its contract) for lengths a multiple of 16.
#[cfg(all(target_arch = "x86_64", target_feature = "sse2", not(portable)))]
#[target_feature(enable = "sse2")]
// SAFETY: (to call) needs SSE2, which every x86-64 CPU has; `dot16` is the safe entry.
unsafe fn dot16_sse2(a: &[i16], b: &[i16]) -> i64 {
    use std::arch::x86_64::*;
    const LO: i32 = i32::MIN + (1 << 16);
    debug_assert!(a.len() <= 1 << 11);
    let (lo_mask, off) = (_mm_set1_epi32(0xffff), _mm_set1_epi32(LO));
    let (mut hi, mut lo) = (_mm_setzero_si128(), _mm_setzero_si128());
    for (x, y) in a.as_chunks::<8>().0.iter().zip(b.as_chunks::<8>().0) {
        let (x, y) = (
            _mm_set_epi16(x[7], x[6], x[5], x[4], x[3], x[2], x[1], x[0]),
            _mm_set_epi16(y[7], y[6], y[5], y[4], y[3], y[2], y[1], y[0]),
        );
        let u = _mm_sub_epi32(_mm_madd_epi16(x, y), off);
        hi = _mm_add_epi32(hi, _mm_srli_epi32::<16>(u));
        lo = _mm_add_epi32(lo, _mm_and_si128(u, lo_mask));
    }
    let hi = _mm_add_epi32(hi, _mm_shuffle_epi32::<0b01_00_11_10>(hi));
    let hi = _mm_add_epi32(hi, _mm_shuffle_epi32::<0b10_11_00_01>(hi));
    let lo = _mm_add_epi32(lo, _mm_shuffle_epi32::<0b01_00_11_10>(lo));
    let lo = _mm_add_epi32(lo, _mm_shuffle_epi32::<0b10_11_00_01>(lo));
    let (hi, lo) = (
        i64::from(_mm_cvtsi128_si32(hi) as u32),
        i64::from(_mm_cvtsi128_si32(lo) as u32),
    );
    (hi << 16) + lo + i64::from(LO) * (a.len() / 2) as i64
}

/// Σ x[k]·h[k] as a wrapping i32 (`ltpf::dot_wrap_portable` is the contract): `pmaddwd`
/// pairs wrap mod 2^32 as the plain sum does, so any order is exact.
#[cfg(all(target_arch = "x86_64", target_feature = "sse2", not(portable)))]
pub(crate) fn dot_wrap(x: &[i16], h: &[i16]) -> i32 {
    let n = x.len().min(h.len());
    // SAFETY: SSE2 is enabled (the `target_feature = "sse2"` gate; x86-64 baseline).
    let v = unsafe { dot_wrap_sse2(&x[..n - n % 8], &h[..n - n % 8]) };
    x[n - n % 8..n]
        .iter()
        .zip(&h[n - n % 8..n])
        .fold(v, |s, (&a, &b)| s.wrapping_add(i32::from(a) * i32::from(b)))
}

#[cfg(all(target_arch = "x86_64", target_feature = "sse2", not(portable)))]
#[target_feature(enable = "sse2")]
// SAFETY: (to call) needs SSE2, which every x86-64 CPU has; `dot_wrap` is the safe entry.
unsafe fn dot_wrap_sse2(x: &[i16], h: &[i16]) -> i32 {
    use std::arch::x86_64::*;
    let mut acc = _mm_setzero_si128();
    for (a, b) in x.as_chunks::<8>().0.iter().zip(h.as_chunks::<8>().0) {
        let a = _mm_set_epi16(a[7], a[6], a[5], a[4], a[3], a[2], a[1], a[0]);
        let b = _mm_set_epi16(b[7], b[6], b[5], b[4], b[3], b[2], b[1], b[0]);
        acc = _mm_add_epi32(acc, _mm_madd_epi16(a, b));
    }
    let acc = _mm_add_epi32(acc, _mm_shuffle_epi32::<0b01_00_11_10>(acc));
    let acc = _mm_add_epi32(acc, _mm_shuffle_epi32::<0b10_11_00_01>(acc));
    _mm_cvtsi128_si32(acc)
}
