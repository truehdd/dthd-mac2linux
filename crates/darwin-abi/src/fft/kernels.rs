//! x86 vector butterflies. Callers select an instruction set supported by the CPU.

use super::Setup;
use std::arch::x86_64::*;
use std::ops::{Add, Mul, Sub};

trait Vector: Copy + Add<Output = Self> + Sub<Output = Self> + Mul<Output = Self> {
    const WIDTH: usize;

    unsafe fn load(p: *const f64) -> Self;
    unsafe fn store(self, p: *mut f64);
    unsafe fn splat(value: f64) -> Self;
    unsafe fn reverse(self) -> Self;
    unsafe fn negate(self) -> Self;
}

macro_rules! vector {
    ($name:ident, $raw:ty, $width:expr, $load:ident, $store:ident, $splat:ident,
     $add:ident, $sub:ident, $mul:ident, $reverse:expr, $negate:expr) => {
        #[derive(Clone, Copy)]
        struct $name($raw);

        impl Vector for $name {
            const WIDTH: usize = $width;

            #[inline(always)]
            unsafe fn load(p: *const f64) -> Self {
                Self($load(p))
            }

            #[inline(always)]
            unsafe fn store(self, p: *mut f64) {
                $store(p, self.0);
            }

            #[inline(always)]
            unsafe fn splat(value: f64) -> Self {
                Self($splat(value))
            }

            #[inline(always)]
            unsafe fn reverse(self) -> Self {
                Self(($reverse)(self.0))
            }

            #[inline(always)]
            unsafe fn negate(self) -> Self {
                Self(($negate)(self.0))
            }
        }

        impl Add for $name {
            type Output = Self;

            #[inline(always)]
            fn add(self, rhs: Self) -> Self {
                unsafe { Self($add(self.0, rhs.0)) }
            }
        }

        impl Sub for $name {
            type Output = Self;

            #[inline(always)]
            fn sub(self, rhs: Self) -> Self {
                unsafe { Self($sub(self.0, rhs.0)) }
            }
        }

        impl Mul for $name {
            type Output = Self;

            #[inline(always)]
            fn mul(self, rhs: Self) -> Self {
                unsafe { Self($mul(self.0, rhs.0)) }
            }
        }
    };
}

vector!(
    Sse2,
    __m128d,
    2,
    _mm_loadu_pd,
    _mm_storeu_pd,
    _mm_set1_pd,
    _mm_add_pd,
    _mm_sub_pd,
    _mm_mul_pd,
    |v| _mm_shuffle_pd::<1>(v, v),
    |v| _mm_xor_pd(v, _mm_set1_pd(-0.0))
);
vector!(
    Avx2,
    __m256d,
    4,
    _mm256_loadu_pd,
    _mm256_storeu_pd,
    _mm256_set1_pd,
    _mm256_add_pd,
    _mm256_sub_pd,
    _mm256_mul_pd,
    |v| _mm256_permute4x64_pd::<0x1b>(v),
    |v| _mm256_xor_pd(v, _mm256_set1_pd(-0.0))
);
vector!(
    Avx512,
    __m512d,
    8,
    _mm512_loadu_pd,
    _mm512_storeu_pd,
    _mm512_set1_pd,
    _mm512_add_pd,
    _mm512_sub_pd,
    _mm512_mul_pd,
    |v| _mm512_permutexvar_pd(_mm512_set_epi64(0, 1, 2, 3, 4, 5, 6, 7), v),
    |v| _mm512_castsi512_pd(_mm512_xor_si512(
        _mm512_castpd_si512(v),
        _mm512_set1_epi64(i64::MIN)
    ))
);

#[inline(always)]
unsafe fn pair<V: Vector>(
    setup: &Setup,
    re: &mut [f64],
    im: &mut [f64],
    source: Option<(&[f64], &[f64])>,
    half: usize,
    sign: f64,
) {
    let n = re.len();
    assert_eq!(im.len(), n);
    assert!(half >= V::WIDTH && half.is_multiple_of(V::WIDTH) && n.is_multiple_of(4 * half));
    assert!(setup.cosine.len() >= 4 * half - 1 && setup.sine.len() >= 4 * half - 1);

    let dr = re.as_mut_ptr();
    let di = im.as_mut_ptr();
    let (sr, si) = if let Some((r, i)) = source {
        assert_eq!(r.len(), n);
        assert_eq!(i.len(), n);
        (r.as_ptr(), i.as_ptr())
    } else {
        (dr.cast_const(), di.cast_const())
    };
    let c1 = setup.cosine.as_ptr().add(half - 1);
    let s1 = setup.sine.as_ptr().add(half - 1);
    let c2 = setup.cosine.as_ptr().add(2 * half - 1);
    let s2 = setup.sine.as_ptr().add(2 * half - 1);
    let sign = V::splat(sign);

    // Load all four quarters before writing: source may be the destination itself.
    for base in (0..n).step_by(4 * half) {
        for k in (0..half).step_by(V::WIDTH) {
            let p0 = base + k;
            let p1 = p0 + half;
            let p2 = p1 + half;
            let p3 = p2 + half;
            let r0 = V::load(sr.add(p0));
            let r1 = V::load(sr.add(p1));
            let r2 = V::load(sr.add(p2));
            let r3 = V::load(sr.add(p3));
            let i0 = V::load(si.add(p0));
            let i1 = V::load(si.add(p1));
            let i2 = V::load(si.add(p2));
            let i3 = V::load(si.add(p3));

            let wr = V::load(c1.add(k));
            let wi = sign * V::load(s1.add(k));
            let tr = wr * r1 - wi * i1;
            let ti = wr * i1 + wi * r1;
            let a0r = r0 + tr;
            let a0i = i0 + ti;
            let a1r = r0 - tr;
            let a1i = i0 - ti;
            let ur = wr * r3 - wi * i3;
            let ui = wr * i3 + wi * r3;
            let a2r = r2 + ur;
            let a2i = i2 + ui;
            let a3r = r2 - ur;
            let a3i = i2 - ui;

            let vr = V::load(c2.add(k));
            let vi = sign * V::load(s2.add(k));
            let xr = vr * a2r - vi * a2i;
            let xi = vr * a2i + vi * a2r;
            let yr = V::load(c2.add(half + k));
            let yi = sign * V::load(s2.add(half + k));
            let zr = yr * a3r - yi * a3i;
            let zi = yr * a3i + yi * a3r;

            (a0r + xr).store(dr.add(p0));
            (a0i + xi).store(di.add(p0));
            (a0r - xr).store(dr.add(p2));
            (a0i - xi).store(di.add(p2));
            (a1r + zr).store(dr.add(p1));
            (a1i + zi).store(di.add(p1));
            (a1r - zr).store(dr.add(p3));
            (a1i - zi).store(di.add(p3));
        }
    }
}

macro_rules! pair_entry {
    ($name:ident, $feature:literal, $vector:ty) => {
        #[target_feature(enable = $feature)]
        pub(super) unsafe fn $name(
            setup: &Setup,
            re: &mut [f64],
            im: &mut [f64],
            source: Option<(&[f64], &[f64])>,
            half: usize,
            sign: f64,
        ) {
            pair::<$vector>(setup, re, im, source, half, sign);
        }
    };
}

pair_entry!(pair_sse2, "sse2", Sse2);
pair_entry!(pair_avx2, "avx2", Avx2);
pair_entry!(pair_avx512, "avx512f", Avx512);

#[inline(always)]
unsafe fn split<V: Vector>(setup: &Setup, r: &mut [f64], im: &mut [f64], direction: i32) -> usize {
    let n = r.len();
    assert_eq!(im.len(), n);
    assert!(n > 0 && setup.cosine.len() >= 2 * n - 1 && setup.sine.len() >= 2 * n - 1);
    let c = setup.cosine.as_ptr().add(n - 1);
    let s = setup.sine.as_ptr().add(n - 1);
    let r = r.as_mut_ptr();
    let im = im.as_mut_ptr();
    let mut k = 1;

    // The ascending and descending blocks stay disjoint; the midpoint is scalar.
    while k + V::WIDTH <= n / 2 {
        let m = n - k - (V::WIDTH - 1);
        let rk = V::load(r.add(k));
        let ik = V::load(im.add(k));
        let rm = V::load(r.add(m)).reverse();
        let imm = V::load(im.add(m)).reverse();
        let ar = rk + rm;
        let ai = ik - imm;
        let br = rk - rm;
        let bi = ik + imm;
        let ck = V::load(c.add(k));
        let sk = V::load(s.add(k));

        if direction == 1 {
            (ar - (sk * br - ck * bi)).store(r.add(k));
            (ai - (sk * bi + ck * br)).store(im.add(k));
            (ar + (sk * br - ck * bi)).reverse().store(r.add(m));
            (ai.negate() - (ck * br + sk * bi))
                .reverse()
                .store(im.add(m));
        } else {
            (ar - (sk * br + ck * bi)).store(r.add(k));
            (ai - (sk * bi - ck * br)).store(im.add(k));
            (ar + (sk * br + ck * bi)).reverse().store(r.add(m));
            (ai.negate() - (sk * bi - ck * br))
                .reverse()
                .store(im.add(m));
        }
        k += V::WIDTH;
    }
    k
}

macro_rules! split_entry {
    ($name:ident, $feature:literal, $vector:ty) => {
        #[target_feature(enable = $feature)]
        pub(super) unsafe fn $name(
            setup: &Setup,
            r: &mut [f64],
            im: &mut [f64],
            direction: i32,
        ) -> usize {
            split::<$vector>(setup, r, im, direction)
        }
    };
}

split_entry!(split_sse2, "sse2", Sse2);
split_entry!(split_avx2, "avx2", Avx2);
split_entry!(split_avx512, "avx512f", Avx512);

/// The reversal table must contain valid source indices.
#[target_feature(enable = "avx512f")]
pub(super) unsafe fn gather_avx512(
    source: (&[f64], &[f64]),
    destination: (&mut [f64], &mut [f64]),
    reversal: &[u32],
    twiddles: [f64; 4],
) -> usize {
    let (r, im) = source;
    let (gr, gi) = destination;
    let n = r.len();
    assert_eq!(im.len(), n);
    assert_eq!(gr.len(), n);
    assert_eq!(gi.len(), n);
    assert_eq!(reversal.len(), n);
    let [c0, s0, c1, s1] = twiddles;
    let cw = _mm512_set_pd(c1, c0, c1, c0, c1, c0, c1, c0);
    let sw = _mm512_set_pd(s1, s0, s1, s0, s1, s0, s1, s0);
    let mut i = 0;

    while i + 8 <= n {
        let indices = _mm256_loadu_si256(reversal.as_ptr().add(i).cast());
        let xr = _mm512_i32gather_pd::<8>(indices, r.as_ptr());
        let xi = _mm512_i32gather_pd::<8>(indices, im.as_ptr());
        let lr = _mm512_unpacklo_pd(xr, xr);
        let hr = _mm512_unpackhi_pd(xr, xr);
        let li = _mm512_unpacklo_pd(xi, xi);
        let hi = _mm512_unpackhi_pd(xi, xi);
        let ar = _mm512_mask_sub_pd(_mm512_add_pd(lr, hr), 0xaa, lr, hr);
        let ai = _mm512_mask_sub_pd(_mm512_add_pd(li, hi), 0xaa, li, hi);
        let aar = _mm512_permutex_pd::<0x44>(ar);
        let aai = _mm512_permutex_pd::<0x44>(ai);
        let bbr = _mm512_permutex_pd::<0xee>(ar);
        let bbi = _mm512_permutex_pd::<0xee>(ai);
        let tr = _mm512_sub_pd(_mm512_mul_pd(cw, bbr), _mm512_mul_pd(sw, bbi));
        let ti = _mm512_add_pd(_mm512_mul_pd(cw, bbi), _mm512_mul_pd(sw, bbr));

        _mm512_storeu_pd(
            gr.as_mut_ptr().add(i),
            _mm512_mask_sub_pd(_mm512_add_pd(aar, tr), 0xcc, aar, tr),
        );
        _mm512_storeu_pd(
            gi.as_mut_ptr().add(i),
            _mm512_mask_sub_pd(_mm512_add_pd(aai, ti), 0xcc, aai, ti),
        );
        i += 8;
    }
    i
}
