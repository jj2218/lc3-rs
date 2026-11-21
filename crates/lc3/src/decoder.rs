//! The LC3 decoder (liblc3 `lc3.c`, decoder side): bitstream → spectrum → TNS, SNS → inverse
//! MDCT → LTPF, with packet loss concealment.

use crate::bits::Bits;
use crate::bwdet;
use crate::common::{MAX_NS, nd, ne, nh, ns};
use crate::fastmath::ldexpf;
use crate::ltpf::{self, LtpfData, LtpfSynthesis};
use crate::mdct::mdct_inverse;
use crate::plc::PlcState;
use crate::sns::{self, SnsData};
use crate::spec;
use crate::tns::{self, TnsData};
use crate::{Error, check_frame, check_pcm, debug_config, setup};

/// What [`Decoder::decode`] did with a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Status {
    /// The frame was decoded.
    Decoded,
    /// The frame was missing or corrupt: concealed from the previous ones.
    Concealed,
}

/// `struct side_data` of `lc3.c`.
#[derive(Clone, Copy, Debug, Default)]
struct Side {
    bw: usize,
    tns: TnsData,
    pitch_present: bool,
    sns: SnsData,
    ltpf: LtpfData,
}

/// `lc3_ldexpf(s, -15)` of `store_float`, but where it wraps: the C subtracts 15 from a
/// non-zero exponent field even below 15, turning a tiny sample (2^-126 ≤ |s| < 2^-112) into a
/// huge one of the opposite sign, which the clamp makes a full-scale click (after ~5 s of
/// concealment the fading output reaches such samples). Those scale exactly instead (s · 2^-15,
/// ~0); every other sample is the C's.
fn to_unit(s: f32) -> f32 {
    let e = s.to_bits() & 0x7f80_0000;
    if e != 0 && e < 15 << 23 {
        s * (1.0 / 32768.0)
    } else {
        ldexpf(s, -15)
    }
}

/// An LC3 decoder for one channel (`struct lc3_decoder`).
///
/// Its output is liblc3's bit for bit, but for two cases where liblc3's is undefined or broken:
/// a 16-bit sample beyond the int range (a crafted or corrupt frame's huge gain) saturates to
/// ±32767, where liblc3 converts with an undefined cast (x86: −32768 for a huge positive
/// sample); and in float output, a sample below 2^-112 (the concealment's fade after ~5 s of
/// lost frames) comes out as ~0, where liblc3's exponent arithmetic wraps it into a ±1 click.
#[derive(Clone)]
pub struct Decoder {
    dt: usize,
    sr: usize,
    sr_pcm: usize,
    ltpf: LtpfSynthesis,
    plc: PlcState,
    /// History ring (nh + ns), MDCT delay (nd), last shaped spectrum (ns), as the C's `x`.
    x: Vec<f32>,
    xs_off: usize,
}

impl Decoder {
    /// A decoder for frames of `frame_us` µs (2500, 5000, 7500 or 10000) coded at
    /// `sample_rate` Hz (8000, 16000, 24000, 32000 or 48000), output at `pcm_rate` Hz
    /// (≥ `sample_rate`, from the same list; liblc3's `lc3_setup_decoder`).
    ///
    /// # Errors
    /// [`Error::InvalidFrameDuration`], [`Error::InvalidSampleRate`] or
    /// [`Error::PcmRateBelowSampleRate`].
    pub fn new(frame_us: u32, sample_rate: u32, pcm_rate: u32) -> Result<Self, Error> {
        let (dt, sr, sr_pcm) = setup(frame_us, sample_rate, pcm_rate)?;
        let (nh, ns, nd) = (nh(dt, sr_pcm), ns(dt, sr_pcm), nd(dt, sr_pcm));
        Ok(Decoder {
            dt,
            sr,
            sr_pcm,
            ltpf: LtpfSynthesis::default(),
            plc: PlcState::new(),
            x: vec![0.0; nh + ns + nd + ns],
            xs_off: nh,
        })
    }

    /// PCM samples per frame.
    #[must_use]
    pub fn frame_samples(&self) -> usize {
        ns(self.dt, self.sr_pcm)
    }

    /// `decode` of `lc3.c`: the frame's side data, its spectrum into `xf`.
    fn read(&mut self, data: &[u8]) -> Result<Side, ()> {
        let (dt, sr) = (self.dt, self.sr);
        let nbytes = data.len() as i32;
        let (ns, ne) = (ns(dt, sr), ne(dt, sr));
        let mut bits = Bits::new(data);
        let bw = bwdet::get_bw(&mut bits, sr)?;
        let spec = spec::get_side(&mut bits, dt, sr)?;
        let tns = tns::get_data(&mut bits, dt, bw, nbytes)?;
        let pitch_present = bits.get_bit() != 0;
        let sns = sns::get_data(&mut bits)?;
        let ltpf = if pitch_present {
            ltpf::get_data(&mut bits)
        } else {
            LtpfData::default()
        };
        let xf = &mut self.x[self.xs_off..];
        spec::decode(&mut bits, dt, sr, bw, nbytes, &spec, xf)?;
        xf[ne..ns].fill(0.0);
        if bits.check() {
            Ok(Side {
                bw,
                tns,
                pitch_present,
                sns,
                ltpf,
            })
        } else {
            Err(())
        }
    }

    /// `synthesize` of `lc3.c`; `side` `None` conceals.
    fn synthesize(&mut self, side: Option<&Side>, nbytes: i32) {
        let (dt, sr, sr_pcm) = (self.dt, self.sr, self.sr_pcm);
        let ns = ns(dt, sr_pcm);
        let ring_len = nh(dt, sr_pcm) + ns;
        let xg_off = ring_len + nd(dt, sr_pcm);
        let xs = self.xs_off;
        let (ring, rest) = self.x.split_at_mut(ring_len);
        let (xd, xg) = rest.split_at_mut(xg_off - ring_len);
        match side {
            Some(side) => {
                self.plc.suspend();
                tns::synthesize(dt, side.bw, &side.tns, &mut ring[xs..]);
                sns::synthesize(dt, sr, &side.sns, &ring[xs..], xg);
                mdct_inverse(dt, sr_pcm, sr, xg, xd, &mut ring[xs..xs + ns]);
            }
            None => {
                let mut xf = [0f32; MAX_NS];
                self.plc.synthesize(dt, sr, xg, &mut xf);
                mdct_inverse(dt, sr_pcm, sr, &xf[..ns], xd, &mut ring[xs..xs + ns]);
            }
        }
        let data = side.filter(|s| s.pitch_present).map(|s| &s.ltpf);
        ltpf::synthesize(dt, sr_pcm, nbytes, &mut self.ltpf, data, ring, xs);
    }

    /// `complete` of `lc3.c`: advances the ring.
    fn complete(&mut self) {
        let (nh, ns) = (nh(self.dt, self.sr_pcm), ns(self.dt, self.sr_pcm));
        self.xs_off = if self.xs_off < nh {
            self.xs_off + ns
        } else {
            0
        };
    }

    /// Decodes one frame (`None`: a lost frame, concealed) into `frame_samples()` samples of
    /// float PCM in [-1, 1] (liblc3's `LC3_PCM_FORMAT_FLOAT`); samples of `pcm` past
    /// `frame_samples()` are left alone.
    ///
    /// # Errors
    /// [`Error::FrameSize`] or [`Error::PcmLength`]; the decoder is then unchanged.
    // `fminf(fmaxf(s, -1), 1)`: NaN gives -1, where `clamp` would keep NaN
    #[allow(clippy::manual_clamp)]
    pub fn decode_f32(&mut self, frame: Option<&[u8]>, pcm: &mut [f32]) -> Result<Status, Error> {
        let ns = self.frame_samples();
        let status = self.run(frame, pcm.len())?;
        for (o, &s) in pcm[..ns]
            .iter_mut()
            .zip(&self.x[self.xs_off..self.xs_off + ns])
        {
            *o = to_unit(s).max(-1.0).min(1.0);
        }
        self.complete();
        Ok(status)
    }

    /// Decodes one frame (`None`: a lost frame, concealed) into `frame_samples()` 16-bit
    /// samples (liblc3's `LC3_PCM_FORMAT_S16`); samples of `pcm` past `frame_samples()` are
    /// left alone.
    ///
    /// # Errors
    /// [`Error::FrameSize`] or [`Error::PcmLength`]; the decoder is then unchanged.
    pub fn decode(&mut self, frame: Option<&[u8]>, pcm: &mut [i16]) -> Result<Status, Error> {
        let ns = self.frame_samples();
        let status = self.run(frame, pcm.len())?;
        for (o, &s) in pcm[..ns]
            .iter_mut()
            .zip(&self.x[self.xs_off..self.xs_off + ns])
        {
            // The C converts with `(int)`: out of the int range it is undefined (x86: INT_MIN,
            // so a huge positive sample would come out -32768); this saturates.
            let v = if s >= 0.0 {
                (s + 0.5) as i32
            } else {
                (s - 0.5) as i32
            };
            *o = v.clamp(-32768, 32767) as i16;
        }
        self.complete();
        Ok(status)
    }

    /// The frame check, read and synthesis of `lc3_decode` (and the PCM buffer's check).
    fn run(&mut self, frame: Option<&[u8]>, pcm_len: usize) -> Result<Status, Error> {
        if let Some(f) = frame {
            check_frame(f.len())?;
        }
        check_pcm(pcm_len, self.frame_samples())?;
        let side = frame.and_then(|f| self.read(f).ok());
        let nbytes = frame.map_or(0, |f| f.len() as i32);
        self.synthesize(side.as_ref(), nbytes);
        Ok(if side.is_some() {
            Status::Decoded
        } else {
            Status::Concealed
        })
    }
}

impl std::fmt::Debug for Decoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        debug_config(f, "Decoder", (self.dt, self.sr, self.sr_pcm))
    }
}
