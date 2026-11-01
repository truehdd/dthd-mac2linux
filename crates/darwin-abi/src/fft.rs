//! Real FFT adapters with vDSP scaling.

#[cfg(target_arch = "x86_64")]
mod kernels;

use std::cell::RefCell;
use std::sync::OnceLock;

#[derive(Clone, Copy)]
struct DoubleDouble {
    hi: f64,
    lo: f64,
}

fn sum(a: f64, b: f64) -> DoubleDouble {
    let s = a + b;
    let v = s - a;
    DoubleDouble {
        hi: s,
        lo: (a - (s - v)) + (b - v),
    }
}

fn fast(a: f64, b: f64) -> DoubleDouble {
    let s = a + b;
    DoubleDouble {
        hi: s,
        lo: b - (s - a),
    }
}

fn product(a: f64, b: f64) -> DoubleDouble {
    let p = a * b;
    let t = 134217729.0 * a;
    let ah = t - (t - a);
    let al = a - ah;
    let t = 134217729.0 * b;
    let bh = t - (t - b);
    let bl = b - bh;
    DoubleDouble {
        hi: p,
        lo: ((ah * bh - p) + ah * bl + al * bh) + al * bl,
    }
}

fn add(a: DoubleDouble, b: DoubleDouble) -> DoubleDouble {
    let s = sum(a.hi, b.hi);
    let t = sum(a.lo, b.lo);
    let s = fast(s.hi, s.lo + t.hi);
    fast(s.hi, s.lo + t.lo)
}

fn multiply(a: DoubleDouble, b: DoubleDouble) -> DoubleDouble {
    let p = product(a.hi, b.hi);
    fast(p.hi, p.lo + (a.hi * b.lo + a.lo * b.hi))
}

fn divide(a: DoubleDouble, b: f64) -> DoubleDouble {
    let q = a.hi / b;
    let p = product(q, b);
    let r = add(
        a,
        DoubleDouble {
            hi: -p.hi,
            lo: -p.lo,
        },
    );
    fast(q, (r.hi + r.lo) / b)
}

fn taylor(r: DoubleDouble, odd: bool) -> DoubleDouble {
    let r2 = multiply(r, r);
    let mut term = if odd {
        r
    } else {
        DoubleDouble { hi: 1.0, lo: 0.0 }
    };
    let mut total = term;
    let mut k = if odd { 2 } else { 1 };

    loop {
        term = divide(multiply(term, r2), -(k as f64) * (k + 1) as f64);
        total = add(total, term);
        if term.hi.abs() < 1e-40 {
            break;
        }
        k += 2;
    }

    total
}

fn twiddle(a: f64) -> (f64, f64) {
    let o = if a < f64::from_bits(0x3fe921fb54442d18) {
        0.0
    } else if a < f64::from_bits(0x4002d97c7f3321d2) {
        1.0
    } else {
        2.0
    };

    let mut r = sum(
        a - o * f64::from_bits(0x3ff921fb54400000),
        -o * f64::from_bits(0x3dd0b4611a600000),
    );
    r = add(
        r,
        DoubleDouble {
            hi: -o * f64::from_bits(0x3ba3198a2e000000),
            lo: 0.0,
        },
    );
    r = add(
        r,
        DoubleDouble {
            hi: -o * f64::from_bits(0x397b839a252049c1),
            lo: 0.0,
        },
    );

    let s = taylor(r, true).hi;
    let c = taylor(r, false).hi;
    if o == 0.0 {
        (c, s)
    } else if o == 1.0 {
        (-s, c)
    } else {
        (-c, -s)
    }
}

pub struct Setup {
    logn: usize,
    cosine: Vec<f64>,
    sine: Vec<f64>,
    reversal: [OnceLock<Vec<u32>>; 25],
}

impl Setup {
    pub fn new(logn: usize) -> Self {
        assert!(logn <= 24);

        let top = 1usize << logn;
        let mut cosine = vec![0.0; (top - 1).max(1)];
        let mut sine = cosine.clone();
        for j in 0..top / 2 {
            let (c, s) = twiddle(2.0 * std::f64::consts::PI * j as f64 / top as f64);
            cosine[top / 2 - 1 + j] = c;
            sine[top / 2 - 1 + j] = s;
        }

        let mut span = 2;
        while span < top {
            for j in 0..span / 2 {
                let at = span / 2 - 1 + j;
                let from = top / 2 - 1 + j * (top / span);
                cosine[at] = cosine[from];
                sine[at] = sine[from];
            }
            span *= 2;
        }

        Self {
            logn,
            cosine,
            sine,
            reversal: std::array::from_fn(|_| OnceLock::new()),
        }
    }

    fn reversal(&self, lg: usize) -> &[u32] {
        self.reversal[lg].get_or_init(|| {
            (0..1u32 << lg)
                .map(|v| v.reverse_bits() >> (u32::BITS as usize - lg))
                .collect()
        })
    }

    #[inline(always)]
    fn complex<const ISA: u8>(
        &self,
        re: &mut [f64],
        im: &mut [f64],
        scratch: &mut [f64],
        lg: usize,
        direction: i32,
    ) {
        let n = re.len();
        if n < 2 {
            return;
        }
        if n == 2 {
            let ar = re[0];
            let ai = im[0];
            let br = re[1];
            let bi = im[1];
            re[0] = ar + br;
            im[0] = ai + bi;
            re[1] = ar - br;
            im[1] = ai - bi;
            return;
        }

        let rev = self.reversal(lg);
        let sign = if direction > 0 { -1.0 } else { 1.0 };
        let (gr, gi) = scratch.split_at_mut(n);
        let c0 = self.cosine[1];
        let s0 = sign * self.sine[1];
        let c1 = self.cosine[2];
        let s1 = sign * self.sine[2];

        #[cfg(target_arch = "x86_64")]
        let start = if ISA == 2 {
            // Setup constructs reversal indices below n (and below 2^24).
            unsafe { kernels::gather_avx512((re, im), (gr, gi), rev, [c0, s0, c1, s1]) }
        } else {
            0
        };
        #[cfg(not(target_arch = "x86_64"))]
        let start = 0;

        for i in (start..n).step_by(4) {
            let x0r = re[rev[i] as usize];
            let x0i = im[rev[i] as usize];
            let x1r = re[rev[i + 1] as usize];
            let x1i = im[rev[i + 1] as usize];
            let x2r = re[rev[i + 2] as usize];
            let x2i = im[rev[i + 2] as usize];
            let x3r = re[rev[i + 3] as usize];
            let x3i = im[rev[i + 3] as usize];

            let a0r = x0r + x1r;
            let a0i = x0i + x1i;
            let a1r = x0r - x1r;
            let a1i = x0i - x1i;
            let b0r = x2r + x3r;
            let b0i = x2i + x3i;
            let b1r = x2r - x3r;
            let b1i = x2i - x3i;

            let t0r = c0 * b0r - s0 * b0i;
            let t0i = c0 * b0i + s0 * b0r;
            let t1r = c1 * b1r - s1 * b1i;
            let t1i = c1 * b1i + s1 * b1r;

            gr[i] = a0r + t0r;
            gi[i] = a0i + t0i;
            gr[i + 1] = a1r + t1r;
            gi[i + 1] = a1i + t1i;
            gr[i + 2] = a0r - t0r;
            gi[i + 2] = a0i - t0i;
            gr[i + 3] = a1r - t1r;
            gi[i + 3] = a1i - t1i;
        }

        let mut half = 4;
        #[cfg(target_arch = "x86_64")]
        {
            let mut source = Some((&*gr, &*gi));
            while 4 * half <= n {
                // ISA is fixed by real_with_scratch after runtime feature detection.
                unsafe {
                    if ISA == 2 && half >= 8 {
                        kernels::pair_avx512(self, re, im, source, half, sign);
                    } else if ISA >= 1 {
                        kernels::pair_avx2(self, re, im, source, half, sign);
                    } else {
                        kernels::pair_sse2(self, re, im, source, half, sign);
                    }
                }
                source = None;
                half *= 4;
            }
            if source.is_some() {
                re.copy_from_slice(gr);
                im.copy_from_slice(gi);
            }
        }

        #[cfg(not(target_arch = "x86_64"))]
        {
            re.copy_from_slice(gr);
            im.copy_from_slice(gi);
        }

        while half < n {
            let cs = &self.cosine[half - 1..half * 2 - 1];
            let sn = &self.sine[half - 1..half * 2 - 1];
            for (r, i) in re
                .chunks_exact_mut(2 * half)
                .zip(im.chunks_exact_mut(2 * half))
            {
                let (ar, br) = r.split_at_mut(half);
                let (ai, bi) = i.split_at_mut(half);
                for k in 0..half {
                    let wr = cs[k];
                    let wi = sign * sn[k];
                    let tr = wr * br[k] - wi * bi[k];
                    let ti = wr * bi[k] + wi * br[k];
                    br[k] = ar[k] - tr;
                    bi[k] = ai[k] - ti;
                    ar[k] += tr;
                    ai[k] += ti;
                }
            }
            half *= 2;
        }
    }

    #[inline(always)]
    fn real_kernel<const ISA: u8>(
        &self,
        r: &mut [f64],
        im: &mut [f64],
        scratch: &mut [f64],
        lg: usize,
        direction: i32,
    ) {
        assert!(lg >= 1 && lg <= self.logn && (direction == 1 || direction == -1));
        let n = 1usize << (lg - 1);
        assert_eq!(r.len(), n);
        assert_eq!(im.len(), n);

        let c = &self.cosine[n - 1..];
        let sn = &self.sine[n - 1..];
        if direction == 1 {
            self.complex::<ISA>(r, im, scratch, lg - 1, 1);
        }

        let r0 = r[0];
        let i0 = im[0];
        let scale = if direction == 1 { 2.0 } else { 1.0 };
        r[0] = scale * (r0 + i0);
        im[0] = scale * (r0 - i0);

        #[cfg(target_arch = "x86_64")]
        let start = unsafe {
            match ISA {
                2 => kernels::split_avx512(self, r, im, direction),
                1 => kernels::split_avx2(self, r, im, direction),
                _ => kernels::split_sse2(self, r, im, direction),
            }
        };
        #[cfg(not(target_arch = "x86_64"))]
        let start = 1;

        for k in start..=n / 2 {
            let m = n - k;
            let ar = r[k] + r[m];
            let ai = im[k] - im[m];
            let br = r[k] - r[m];
            let bi = im[k] + im[m];
            let ck = c[k];
            let sk = sn[k];
            if direction == 1 {
                r[k] = ar - (sk * br - ck * bi);
                im[k] = ai - (sk * bi + ck * br);
                if m != k {
                    r[m] = ar + (sk * br - ck * bi);
                    im[m] = -ai - (ck * br + sk * bi);
                }
            } else {
                r[k] = ar - (sk * br + ck * bi);
                im[k] = ai - (sk * bi - ck * br);
                if m != k {
                    r[m] = ar + (sk * br + ck * bi);
                    im[m] = -ai - (sk * bi - ck * br);
                }
            }
        }

        if direction != 1 {
            self.complex::<ISA>(r, im, scratch, lg - 1, -1);
        }
    }
}

impl Setup {
    pub fn real(&self, re: &mut [f64], im: &mut [f64], lg: usize, direction: i32) {
        SCRATCH.with_borrow_mut(|scratch| {
            scratch.resize(2 * re.len(), 0.0);
            self.real_with_scratch(re, im, scratch, lg, direction);
        });
    }

    fn real_with_scratch(
        &self,
        re: &mut [f64],
        im: &mut [f64],
        scratch: &mut [f64],
        lg: usize,
        direction: i32,
    ) {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            match selected_isa() {
                2 => real_avx512(self, re, im, scratch, lg, direction),
                1 => real_avx2(self, re, im, scratch, lg, direction),
                _ => self.real_kernel::<0>(re, im, scratch, lg, direction),
            }
        }

        #[cfg(not(target_arch = "x86_64"))]
        self.real_kernel::<0>(re, im, scratch, lg, direction);
    }
}

#[cfg(target_arch = "x86_64")]
fn selected_isa() -> u8 {
    static CHOSEN: OnceLock<u8> = OnceLock::new();
    *CHOSEN.get_or_init(|| {
        let widest = if std::is_x86_feature_detected!("avx512f") {
            2
        } else if std::is_x86_feature_detected!("avx2") {
            1
        } else {
            0
        };

        match std::env::var("DTHD_FFT_ISA").ok().as_deref() {
            None | Some("") => widest,
            Some("sse2") => 0,
            Some("avx2") if widest >= 1 => 1,
            Some("avx512") if widest >= 2 => 2,
            value => panic!("DTHD_FFT_ISA={value:?} is not supported on this CPU"),
        }
    })
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn real_avx2(
    s: &Setup,
    r: &mut [f64],
    i: &mut [f64],
    scratch: &mut [f64],
    lg: usize,
    d: i32,
) {
    s.real_kernel::<1>(r, i, scratch, lg, d);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
unsafe fn real_avx512(
    s: &Setup,
    r: &mut [f64],
    i: &mut [f64],
    scratch: &mut [f64],
    lg: usize,
    d: i32,
) {
    s.real_kernel::<2>(r, i, scratch, lg, d);
}

thread_local! {
    static SCRATCH: RefCell<Vec<f64>> = const { RefCell::new(Vec::new()) };
    static BUFFER: RefCell<Vec<f64>> = const { RefCell::new(Vec::new()) };
}

#[repr(C)]
pub struct Split<T> {
    pub r: *mut T,
    pub i: *mut T,
}

pub extern "C" fn fft_create(logn: usize, radix: i32) -> *mut Setup {
    assert_eq!(radix, 0);
    Box::into_raw(Box::new(Setup::new(logn)))
}

/// # Safety
/// `s` must be null or an unfreed pointer returned by `fft_create`.
pub unsafe extern "C" fn fft_destroy(s: *mut Setup) {
    if !s.is_null() {
        drop(Box::from_raw(s));
    }
}

/// # Safety
/// `s` must be live; `z` must contain disjoint writable arrays covering the transform.
pub unsafe extern "C" fn fft_real_d(
    s: *const Setup,
    z: *const Split<f64>,
    stride: isize,
    lg: usize,
    direction: i32,
) {
    let s = &*s;
    assert!(lg >= 1 && lg <= s.logn);
    let n = 1usize << (lg - 1);
    let z = &*z;
    if stride == 1 {
        s.real(
            std::slice::from_raw_parts_mut(z.r, n),
            std::slice::from_raw_parts_mut(z.i, n),
            lg,
            direction,
        );
        return;
    }

    BUFFER.with_borrow_mut(|b| {
        b.resize(2 * n, 0.0);
        let (r, i) = b.split_at_mut(n);

        for k in 0..n {
            r[k] = *z.r.offset(k as isize * stride);
            i[k] = *z.i.offset(k as isize * stride);
        }

        s.real(r, i, lg, direction);

        for k in 0..n {
            *z.r.offset(k as isize * stride) = r[k];
            *z.i.offset(k as isize * stride) = i[k];
        }
    });
}

/// # Safety
/// `s` must be live; `z` must contain disjoint writable arrays covering the transform.
pub unsafe extern "C" fn fft_real_f(
    s: *const Setup,
    z: *const Split<f32>,
    stride: isize,
    lg: usize,
    direction: i32,
) {
    let s = &*s;
    assert!(lg >= 1 && lg <= s.logn);
    let n = 1usize << (lg - 1);
    let z = &*z;

    BUFFER.with_borrow_mut(|b| {
        b.resize(2 * n, 0.0);
        let (r, i) = b.split_at_mut(n);

        for k in 0..n {
            r[k] = *z.r.offset(k as isize * stride) as f64;
            i[k] = *z.i.offset(k as isize * stride) as f64;
        }

        s.real(r, i, lg, direction);

        for k in 0..n {
            *z.r.offset(k as isize * stride) = r[k] as f32;
            *z.i.offset(k as isize * stride) = i[k] as f32;
        }
    });
}

macro_rules! split_copy {
    ($ctoz:ident, $ztoc:ident, $sample:ty) => {
        /// # Safety
        /// Pointers must cover `count` complex samples at the supplied strides.
        pub unsafe extern "C" fn $ctoz(
            interleaved: *const $sample,
            interleaved_stride: isize,
            split: *const Split<$sample>,
            split_stride: isize,
            count: usize,
        ) {
            for index in 0..count as isize {
                let input = interleaved.offset(index * interleaved_stride);
                *(*split).r.offset(index * split_stride) = *input;
                *(*split).i.offset(index * split_stride) = *input.add(1);
            }
        }

        /// # Safety
        /// Pointers must cover `count` complex samples at the supplied strides.
        pub unsafe extern "C" fn $ztoc(
            split: *const Split<$sample>,
            split_stride: isize,
            interleaved: *mut $sample,
            interleaved_stride: isize,
            count: usize,
        ) {
            for index in 0..count as isize {
                let output = interleaved.offset(index * interleaved_stride);
                *output = *(*split).r.offset(index * split_stride);
                *output.add(1) = *(*split).i.offset(index * split_stride);
            }
        }
    };
}

split_copy!(ctoz_f, ztoc_f, f32);
split_copy!(ctoz_d, ztoc_d, f64);

/// # Safety
/// Pointers must cover `n` readable and writable samples at the supplied strides.
pub unsafe extern "C" fn vdpsp(a: *const f64, sa: isize, b: *mut f32, sb: isize, n: usize) {
    for i in 0..n {
        *b.offset(i as isize * sb) = *a.offset(i as isize * sa) as f32;
    }
}

/// # Safety
/// Pointers must cover `n` readable and writable samples at the supplied strides.
pub unsafe extern "C" fn vspdp(a: *const f32, sa: isize, b: *mut f64, sb: isize, n: usize) {
    for i in 0..n {
        *b.offset(i as isize * sb) = *a.offset(i as isize * sa) as f64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn available(isa: u8) -> bool {
        #[cfg(target_arch = "x86_64")]
        match isa {
            2 => std::is_x86_feature_detected!("avx512f"),
            1 => std::is_x86_feature_detected!("avx2"),
            _ => true,
        }

        #[cfg(not(target_arch = "x86_64"))]
        {
            isa == 0
        }
    }

    #[test]
    fn real_round_trip_scaling_and_strides() {
        unsafe {
            let setup = fft_create(12, 0);
            for lg in 1..=12 {
                let n = 1usize << lg;
                for stride in [1, 2] {
                    let mut values = vec![0.0; n * stride];
                    for (i, v) in values.iter_mut().enumerate() {
                        *v = (i as f64 * 0.37).sin();
                    }

                    let original = values.clone();
                    let split = Split {
                        r: values.as_mut_ptr(),
                        i: values.as_mut_ptr().add(n / 2 * stride),
                    };

                    fft_real_d(setup, &split, stride as isize, lg, 1);
                    fft_real_d(setup, &split, stride as isize, lg, -1);

                    for i in 0..values.len() {
                        if i % stride == 0 {
                            assert!((values[i] / (2 * n) as f64 - original[i]).abs() < 1e-12);
                        } else {
                            assert_eq!(values[i].to_bits(), original[i].to_bits());
                        }
                    }
                }
            }

            fft_destroy(setup);
        }
    }

    #[test]
    fn paired_stages_match_radix_two() {
        let setup = Setup::new(14);
        let mut seed = 19u32;
        for lg in 0..=14 {
            let n = 1usize << lg;
            let input: Vec<_> = (0..2 * n)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    seed as f64 / u32::MAX as f64 - 0.5
                })
                .collect();

            for direction in [1, -1] {
                let mut expected = input.clone();
                let (r, im) = expected.split_at_mut(n);
                for k in 0..n {
                    let reversed = if lg == 0 {
                        k
                    } else {
                        k.reverse_bits() >> (usize::BITS - lg)
                    };
                    r[k] = input[reversed];
                    im[k] = input[n + reversed];
                }

                let sign = if direction == 1 { -1.0 } else { 1.0 };
                let mut half = 1;
                while half < n {
                    for base in (0..n).step_by(2 * half) {
                        for k in 0..half {
                            let a = base + k;
                            let b = a + half;
                            let (tr, ti) = if half == 1 {
                                (r[b], im[b])
                            } else {
                                let wr = setup.cosine[half - 1 + k];
                                let wi = sign * setup.sine[half - 1 + k];
                                (wr * r[b] - wi * im[b], wr * im[b] + wi * r[b])
                            };
                            r[b] = r[a] - tr;
                            im[b] = im[a] - ti;
                            r[a] += tr;
                            im[a] += ti;
                        }
                    }
                    half *= 2;
                }

                for isa in 0..=2 {
                    if !available(isa) {
                        continue;
                    }
                    let mut actual = input.clone();
                    let (r, im) = actual.split_at_mut(n);
                    let mut scratch = vec![0.0; 2 * n];
                    match isa {
                        2 => setup.complex::<2>(r, im, &mut scratch, lg as usize, direction),
                        1 => setup.complex::<1>(r, im, &mut scratch, lg as usize, direction),
                        _ => setup.complex::<0>(r, im, &mut scratch, lg as usize, direction),
                    }
                    assert!(
                        expected
                            .iter()
                            .zip(&actual)
                            .all(|(a, b)| a.to_bits() == b.to_bits()),
                        "ISA {isa}, lg {lg}, direction {direction}"
                    );
                }
            }
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn instruction_sets_produce_identical_bits() {
        let setup = Setup::new(16);
        let mut seed = 7u32;
        for lg in 2..=16 {
            let n = 1usize << lg;
            for d in [1, -1] {
                let input: Vec<_> = (0..n)
                    .map(|_| {
                        seed ^= seed << 13;
                        seed ^= seed >> 17;
                        seed ^= seed << 5;
                        seed as f64 / u32::MAX as f64 - 0.5
                    })
                    .collect();

                let mut baseline = input.clone();
                let (r, i) = baseline.split_at_mut(n / 2);
                setup.real_kernel::<0>(r, i, &mut vec![0.0; n], lg, d);

                for isa in [1, 2] {
                    if !available(isa) {
                        continue;
                    }

                    let mut candidate = input.clone();
                    let (r, i) = candidate.split_at_mut(n / 2);
                    unsafe {
                        if isa == 1 {
                            real_avx2(&setup, r, i, &mut vec![0.0; n], lg, d);
                        } else {
                            real_avx512(&setup, r, i, &mut vec![0.0; n], lg, d);
                        }
                    }

                    assert!(
                        baseline
                            .iter()
                            .zip(&candidate)
                            .all(|(a, b)| a.to_bits() == b.to_bits()),
                        "ISA {isa}, lg {lg}, direction {d}"
                    );
                }
            }
        }
    }
}
