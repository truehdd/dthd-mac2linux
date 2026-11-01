//! Scalar algorithms with Darwin-compatible rounding. No fused operations.

const POW_LOG_TABLE: [[f64; 2]; 33] = [
    [
        f64::from_bits(0x3ff0000000000000),
        f64::from_bits(0x0000000000000000),
    ],
    [
        f64::from_bits(0x3fef07c1f0000000),
        f64::from_bits(0x3fa6bad38119a13a),
    ],
    [
        f64::from_bits(0x3fee1e1e1e000000),
        f64::from_bits(0x3fb663f6fc3a678d),
    ],
    [
        f64::from_bits(0x3fed41d41d000000),
        f64::from_bits(0x3fc08c588e79f8e9),
    ],
    [
        f64::from_bits(0x3fec71c71c000000),
        f64::from_bits(0x3fc5c01a3cde7f74),
    ],
    [
        f64::from_bits(0x3febacf915000000),
        f64::from_bits(0x3fcacf5e2c156d8f),
    ],
    [
        f64::from_bits(0x3feaf286bd000000),
        f64::from_bits(0x3fcfbc16b67c143b),
    ],
    [
        f64::from_bits(0x3fea41a41a000000),
        f64::from_bits(0x3fd24407abf4dc03),
    ],
    [
        f64::from_bits(0x3fe999999a000000),
        f64::from_bits(0x3fd49a784a5bc715),
    ],
    [
        f64::from_bits(0x3fe8f9c190000000),
        f64::from_bits(0x3fd6e221cc2bb868),
    ],
    [
        f64::from_bits(0x3fe8618618000000),
        f64::from_bits(0x3fd91bba8a906b7f),
    ],
    [
        f64::from_bits(0x3fe7d05f41000000),
        f64::from_bits(0x3fdb47ebf91d417c),
    ],
    [
        f64::from_bits(0x3fe745d174000000),
        f64::from_bits(0x3fdd6753e1a43e85),
    ],
    [
        f64::from_bits(0x3fe6c16c17000000),
        f64::from_bits(0x3fdf7a8567cd1cbd),
    ],
    [
        f64::from_bits(0x3fe642c859000000),
        f64::from_bits(0x3fe0c10500ed4fee),
    ],
    [
        f64::from_bits(0x3fe5c9882c000000),
        f64::from_bits(0x3fe1bf311daefb44),
    ],
    [
        f64::from_bits(0x3fe5555555000000),
        f64::from_bits(0xbfda8ff9700fb5e8),
    ],
    [
        f64::from_bits(0x3fe4e5e0a7000000),
        f64::from_bits(0xbfd8a89809effdb0),
    ],
    [
        f64::from_bits(0x3fe47ae148000000),
        f64::from_bits(0xbfd6cb0f69d71d60),
    ],
    [
        f64::from_bits(0x3fe4141414000000),
        f64::from_bits(0xbfd4f6fbb272707b),
    ],
    [
        f64::from_bits(0x3fe3b13b14000000),
        f64::from_bits(0xbfd32bfee4e242df),
    ],
    [
        f64::from_bits(0x3fe3521cfb000000),
        f64::from_bits(0xbfd169c0529431d6),
    ],
    [
        f64::from_bits(0x3fe2f684be000000),
        f64::from_bits(0xbfcf5fd8aca1915c),
    ],
    [
        f64::from_bits(0x3fe29e412a000000),
        f64::from_bits(0xbfcbfc67a914f425),
    ],
    [
        f64::from_bits(0x3fe2492492000000),
        f64::from_bits(0xbfc8a89807dd1446),
    ],
    [
        f64::from_bits(0x3fe1f7047e000000),
        f64::from_bits(0xbfc563dc2c860081),
    ],
    [
        f64::from_bits(0x3fe1a7b961000000),
        f64::from_bits(0xbfc22dadc196353e),
    ],
    [
        f64::from_bits(0x3fe15b1e5f000000),
        f64::from_bits(0xbfbe0b1adf360338),
    ],
    [
        f64::from_bits(0x3fe1111111000000),
        f64::from_bits(0xbfb7d604955e66d6),
    ],
    [
        f64::from_bits(0x3fe0c97150000000),
        f64::from_bits(0xbfb1bb32abc5a676),
    ],
    [
        f64::from_bits(0x3fe0842108000000),
        f64::from_bits(0xbfa77394be4eb523),
    ],
    [
        f64::from_bits(0x3fe0410410000000),
        f64::from_bits(0xbf9743ee6f09edf1),
    ],
    [
        f64::from_bits(0x3fe0000000000000),
        f64::from_bits(0x0000000000000000),
    ],
];

const POW_EXP_TABLE: [f64; 32] = [
    f64::from_bits(0x3ff0000000000000),
    f64::from_bits(0x3ff059b0d3158574),
    f64::from_bits(0x3ff0b5586cf9890f),
    f64::from_bits(0x3ff11301d0125b51),
    f64::from_bits(0x3ff172b83c7d517b),
    f64::from_bits(0x3ff1d4873168b9aa),
    f64::from_bits(0x3ff2387a6e756238),
    f64::from_bits(0x3ff29e9df51fdee1),
    f64::from_bits(0x3ff306fe0a31b715),
    f64::from_bits(0x3ff371a7373aa9cb),
    f64::from_bits(0x3ff3dea64c123422),
    f64::from_bits(0x3ff44e086061892d),
    f64::from_bits(0x3ff4bfdad5362a27),
    f64::from_bits(0x3ff5342b569d4f82),
    f64::from_bits(0x3ff5ab07dd485429),
    f64::from_bits(0x3ff6247eb03a5585),
    f64::from_bits(0x3ff6a09e667f3bcd),
    f64::from_bits(0x3ff71f75e8ec5f74),
    f64::from_bits(0x3ff7a11473eb0187),
    f64::from_bits(0x3ff82589994cce13),
    f64::from_bits(0x3ff8ace5422aa0db),
    f64::from_bits(0x3ff93737b0cdc5e5),
    f64::from_bits(0x3ff9c49182a3f090),
    f64::from_bits(0x3ffa5503b23e255d),
    f64::from_bits(0x3ffae89f995ad3ad),
    f64::from_bits(0x3ffb7f76f2fb5e47),
    f64::from_bits(0x3ffc199bdd85529c),
    f64::from_bits(0x3ffcb720dcef9069),
    f64::from_bits(0x3ffd5818dcfba487),
    f64::from_bits(0x3ffdfc97337b9b5f),
    f64::from_bits(0x3ffea4afa2a490da),
    f64::from_bits(0x3fff50765b6e4540),
];

const SINCOS_REDUCE_TABLE: [[f64; 2]; 63] = [
    [
        f64::from_bits(0x3fe45f306e000000),
        f64::from_bits(0xbdfb1bbead603d8b),
    ],
    [
        f64::from_bits(0x3fe45f306e000000),
        f64::from_bits(0xbdfb1bbead603d8b),
    ],
    [
        f64::from_bits(0x3fe45f306e000000),
        f64::from_bits(0xbdfb1bbead603d8b),
    ],
    [
        f64::from_bits(0x3fe45f306e000000),
        f64::from_bits(0xbdfb1bbead603d8b),
    ],
    [
        f64::from_bits(0x3fe45f306e000000),
        f64::from_bits(0xbdfb1bbead603d8b),
    ],
    [
        f64::from_bits(0x3fe45f306e000000),
        f64::from_bits(0xbdfb1bbead603d8b),
    ],
    [
        f64::from_bits(0x3fe45f306e000000),
        f64::from_bits(0xbdfb1bbead603d8b),
    ],
    [
        f64::from_bits(0x3fe45f306e000000),
        f64::from_bits(0xbdfb1bbead603d8b),
    ],
    [
        f64::from_bits(0x3fe45f306e000000),
        f64::from_bits(0xbdfb1bbead603d8b),
    ],
    [
        f64::from_bits(0x3fe45f306e000000),
        f64::from_bits(0xbdfb1bbead603d8b),
    ],
    [
        f64::from_bits(0x3fe45f306e000000),
        f64::from_bits(0xbdfb1bbead603d8b),
    ],
    [
        f64::from_bits(0xbfd7419f24000000),
        f64::from_bits(0xbdfb1bbead603d8b),
    ],
    [
        f64::from_bits(0xbfbd067c92000000),
        f64::from_bits(0x3dd391054a7f09d6),
    ],
    [
        f64::from_bits(0x3f87cc1b72000000),
        f64::from_bits(0x3dac882a53f84eb0),
    ],
    [
        f64::from_bits(0xbf7067c91b000000),
        f64::from_bits(0xbd7bbead603d8a83),
    ],
    [
        f64::from_bits(0xbf19f246c7000000),
        f64::from_bits(0x3d1054a7f09d5f48),
    ],
    [
        f64::from_bits(0xbf19f246c7000000),
        f64::from_bits(0x3d1054a7f09d5f48),
    ],
    [
        f64::from_bits(0xbf19f246c7000000),
        f64::from_bits(0x3d1054a7f09d5f48),
    ],
    [
        f64::from_bits(0x3ef836e4e4000000),
        f64::from_bits(0x3d1054a7f09d5f48),
    ],
    [
        f64::from_bits(0xbedf246c6f000000),
        f64::from_bits(0x3cb529fc2757d1f5),
    ],
    [
        f64::from_bits(0x3e8b727221000000),
        f64::from_bits(0xbca5ac07b1505c16),
    ],
    [
        f64::from_bits(0x3e8b727221000000),
        f64::from_bits(0xbca5ac07b1505c16),
    ],
    [
        f64::from_bits(0xbe6236377d000000),
        f64::from_bits(0xbc86b01ec5417056),
    ],
    [
        f64::from_bits(0x3e5b939105000000),
        f64::from_bits(0x3c729fc2757d1f53),
    ],
    [
        f64::from_bits(0xbe31b1bbeb000000),
        f64::from_bits(0x3c44fe13abe8fa9a),
    ],
    [
        f64::from_bits(0xbdfb1bbead000000),
        f64::from_bits(0xbc180f62a0b82b2d),
    ],
    [
        f64::from_bits(0xbdfb1bbead000000),
        f64::from_bits(0xbc180f62a0b82b2d),
    ],
    [
        f64::from_bits(0x3dd391054a000000),
        f64::from_bits(0x3bffc2757d1f534e),
    ],
    [
        f64::from_bits(0x3dac882a54000000),
        f64::from_bits(0xbb8ec54170565912),
    ],
    [
        f64::from_bits(0xbd7bbead60000000),
        f64::from_bits(0xbb8ec54170565912),
    ],
    [
        f64::from_bits(0xbd7bbead60000000),
        f64::from_bits(0xbb8ec54170565912),
    ],
    [
        f64::from_bits(0x3d51054a7f000000),
        f64::from_bits(0x3b43abe8fa9a6ee0),
    ],
    [
        f64::from_bits(0x3d1054a7f1000000),
        f64::from_bits(0xbb38a82e0acb223f),
    ],
    [
        f64::from_bits(0x3d1054a7f1000000),
        f64::from_bits(0xbb38a82e0acb223f),
    ],
    [
        f64::from_bits(0x3cb529fc27000000),
        f64::from_bits(0x3ad5f47d4d377037),
    ],
    [
        f64::from_bits(0x3cb529fc27000000),
        f64::from_bits(0x3ad5f47d4d377037),
    ],
    [
        f64::from_bits(0x3cb529fc27000000),
        f64::from_bits(0x3ad5f47d4d377037),
    ],
    [
        f64::from_bits(0x3c94a7f09d000000),
        f64::from_bits(0x3ab7d1f534ddc0db),
    ],
    [
        f64::from_bits(0x3c729fc275000000),
        f64::from_bits(0x3a9f47d4d377036e),
    ],
    [
        f64::from_bits(0x3c44fe13ac000000),
        f64::from_bits(0xba470565911f924f),
    ],
    [
        f64::from_bits(0xbc3603d8a8000000),
        f64::from_bits(0xba470565911f924f),
    ],
    [
        f64::from_bits(0xbc180f62a1000000),
        f64::from_bits(0x3a31f534ddc0db63),
    ],
    [
        f64::from_bits(0x3bffc2757d000000),
        f64::from_bits(0x39ff534ddc0db629),
    ],
    [
        f64::from_bits(0xbb8ec54170000000),
        f64::from_bits(0xb9a596447e493ad5),
    ],
    [
        f64::from_bits(0xbb8ec54170000000),
        f64::from_bits(0xb9a596447e493ad5),
    ],
    [
        f64::from_bits(0xbb8ec54170000000),
        f64::from_bits(0xb9a596447e493ad5),
    ],
    [
        f64::from_bits(0x3b43abe8fb000000),
        f64::from_bits(0xb9696447e493ad4d),
    ],
    [
        f64::from_bits(0x3b43abe8fb000000),
        f64::from_bits(0xb9696447e493ad4d),
    ],
    [
        f64::from_bits(0xbb38a82e0b000000),
        f64::from_bits(0x394a6ee06db14acd),
    ],
    [
        f64::from_bits(0x3b1d5f47d5000000),
        f64::from_bits(0xb926447e493ad4ce),
    ],
    [
        f64::from_bits(0xbae505c159000000),
        f64::from_bits(0xb90911f924eb5336),
    ],
    [
        f64::from_bits(0x3ad5f47d4d000000),
        f64::from_bits(0x38ebb81b6c52b328),
    ],
    [
        f64::from_bits(0x3ab7d1f535000000),
        f64::from_bits(0xb8c11f924eb53362),
    ],
    [
        f64::from_bits(0x3a9f47d4d3000000),
        f64::from_bits(0x38bdc0db6295993c),
    ],
    [
        f64::from_bits(0xba47056591000000),
        f64::from_bits(0xb84f924eb53361de),
    ],
    [
        f64::from_bits(0xba47056591000000),
        f64::from_bits(0xb84f924eb53361de),
    ],
    [
        f64::from_bits(0x3a31f534de000000),
        f64::from_bits(0xb84f924eb53361de),
    ],
    [
        f64::from_bits(0x39ff534ddc000000),
        f64::from_bits(0x37eb6c52b3278872),
    ],
    [
        f64::from_bits(0x39ff534ddc000000),
        f64::from_bits(0x37eb6c52b3278872),
    ],
    [
        f64::from_bits(0xb9a596447e000000),
        f64::from_bits(0xb7c24eb53361de38),
    ],
    [
        f64::from_bits(0xb9a596447e000000),
        f64::from_bits(0xb7c24eb53361de38),
    ],
    [
        f64::from_bits(0x3994d37703000000),
        f64::from_bits(0x37bb6295993c4390),
    ],
    [
        f64::from_bits(0x39734ddc0e000000),
        f64::from_bits(0xb79275a99b0ef1bf),
    ],
];

#[link(name = "m")]
unsafe extern "C" {
    fn powf(x: f32, y: f32) -> f32;
    fn ldexp(x: f64, exponent: i32) -> f64;
}

/// Two floats occupy one SSE register in the Darwin/SysV return ABI.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FloatPair {
    pub s: f32,
    pub c: f32,
}

#[inline]
fn h(bits: u64) -> f64 {
    f64::from_bits(bits)
}

pub extern "C" fn darwin_powf(mut x: f32, y: f32) -> f32 {
    if y == 0.0 || x == 1.0 {
        return 1.0;
    }
    if !x.is_finite() || !y.is_finite() || x == 0.0 {
        return unsafe { powf(x, y) };
    }

    let mut negative = false;
    if x < 0.0 {
        if y.trunc() != y {
            return unsafe { powf(x, y) };
        }
        negative = y.abs() < 16777216.0 && (y as i32 & 1) != 0;
        x = -x;
    }

    let mut bits = x.to_bits();
    let mut exponent = (bits >> 23) as i32 - 127;
    if exponent == -127 {
        x *= 16777216.0;
        bits = x.to_bits();
        exponent = (bits >> 23) as i32 - 127 - 24;
    }
    let mantissa = bits & 0x7fffff;
    let mut logarithm = exponent as f64;
    if mantissa != 0 {
        let index = ((mantissa + 0x20000) >> 18) as usize;
        let m = f32::from_bits(mantissa | 0x3f800000);
        let r = m as f64 * POW_LOG_TABLE[index][0] - 1.0;
        let a = (r - h(0x40034ed3f66c1ddf)) * r + h(0x40029001100aec6f);
        let b = (r + h(0x3ff29e2d4399cff4)) * r + h(0x40013bd91bcbc3c4);
        let correction = ((r * h(0x3fd278ddbc785752)) * a) * b;
        logarithm =
            correction + ((exponent + i32::from(index >= 16)) as f64 + POW_LOG_TABLE[index][1]);
    }

    let z = (logarithm * (y as f64 * 32.0)).clamp(-8192.0, 8192.0);
    let n = (z + 0.5f64.copysign(z)) as i32;
    let r = z - n as f64;
    let a = r + h(0x406012cb54412271);
    let b = r + h(0x404c09f044bf8506);
    let correction = ((r * h(0x3e43b2c9b86ef214)) * a) * (r * b + h(0x40d1ef18d453f7dd));
    let index = n as u32 & 31;
    let scale = POW_EXP_TABLE[index as usize];
    let value = scale + correction * scale;
    let result = unsafe { ldexp(value, (n - index as i32) / 32) } as f32;

    if negative {
        -result
    } else {
        result
    }
}

#[allow(clippy::eq_op)] // x - x preserves native nonfinite arithmetic.
pub extern "C" fn sincos_f(x: f32) -> FloatPair {
    let bits = x.to_bits() & 0x7fffffff;
    if bits < 0x800000 {
        return FloatPair { s: x, c: 1.0 };
    }
    if bits >= 0x7f800000 {
        return FloatPair { s: x - x, c: x - x };
    }

    let (n, r) = if bits <= 0x41000000 {
        let z = x as f64 * h(0x3fe45f306dc9c883);
        let n = z.round_ties_even();
        (n, z - n)
    } else {
        let index = ((bits - 0x41000000) >> 24) as usize;
        let high = x as f64 * SINCOS_REDUCE_TABLE[index][0];
        let n = high.round_ties_even();
        (n, x as f64 * SINCOS_REDUCE_TABLE[index][1] + (high - n))
    };

    let t = r * r;
    let mut s =
        ((t + -h(0x402c3d3ee4f8713f)) * t + h(0x40444921a30f0d80)) * (h(0x3f24bc899499342c) * r);
    let mut c = ((t + -h(0x4020bc0396540c13)) * t + h(0x401d78212789bc11)) * h(0x3f4d9c364e9721d5);
    s *= (t + -h(0x402ef09ab49e3a5b)) * t + h(0x406e97264e36bd93);
    c *= (t + -h(0x402d6f5ade6c8c37)) * t + h(0x4062c6b9c7f88fa0);

    let mut pair = FloatPair {
        s: s as f32,
        c: c as f32,
    };
    let quadrant = (n % 4.0) as i32;
    if quadrant & 1 != 0 {
        pair = FloatPair {
            s: pair.c,
            c: -pair.s,
        };
    }
    if quadrant & 2 != 0 {
        pair.s = -pair.s;
        pair.c = -pair.c;
    }
    pair
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_rounding_vectors() {
        let powers = [
            [0x3f8bb6b9, 0x3e4ccccd, 0x3f8242ed],
            [0x4ad3f640, 0x3e4ccccd, 0x41bad2d3],
            [0x49b79058, 0x3e4ccccd, 0x4189920e],
            [0x42de85b2, 0x3e4ccccd, 0x40243a82],
            [0x48ac17f5, 0x3e4ccccd, 0x414dd827],
            [0x3f8ce56c, 0x3e4ccccd, 0x3f827b2e],
            [0x484847b0, 0x3e4ccccd, 0x4138b7a0],
        ];
        let sines = [
            [0x3e7dfb0f, 0x3e7b6277],
            [0x3f154331, 0x3f0cf2af],
            [0x3f33ce9e, 0x3f256233],
            [0x3e7d08e8, 0x3e7a77b2],
            [0x3e942eee, 0x3e921f8b],
            [0x3e44ea48, 0x3e43b42a],
            [0x3ee14839, 0x3eda153b],
            [0x3ee72798, 0x3edf61b4],
        ];
        let pairs = [
            [0x00000000, 0x00000000, 0x3f800000],
            [0x80000000, 0x80000000, 0x3f800000],
            [0x00000001, 0x00000001, 0x3f800000],
            [0x80000001, 0x80000001, 0x3f800000],
            [0x007fffff, 0x007fffff, 0x3f800000],
            [0x00800000, 0x00800000, 0x3f800000],
            [0x40ffffff, 0x3f7d4696, 0xbe14fdd6],
            [0x41000000, 0x3f7d4695, 0xbe14fdf6],
            [0x41000001, 0x3f7d4693, 0xbe14fe35],
            [0xc1000001, 0xbf7d4693, 0xbe14fe35],
            [0x7f7fffff, 0xbf0599b3, 0x3f5a5f96],
            [0xff7fffff, 0x3f0599b3, 0x3f5a5f96],
        ];
        for [x, y, r] in powers {
            assert_eq!(
                darwin_powf(f32::from_bits(x), f32::from_bits(y)).to_bits(),
                r
            );
        }
        for [x, r] in sines {
            assert_eq!(sincos_f(f32::from_bits(x)).s.to_bits(), r);
        }
        for [x, s, c] in pairs {
            let p = sincos_f(f32::from_bits(x));
            assert_eq!((p.s.to_bits(), p.c.to_bits()), (s, c));
        }
    }

    #[test]
    fn special_values() {
        assert_eq!(darwin_powf(-2.0, 3.0), -8.0);
        assert_eq!(darwin_powf(-2.0, -3.0), -0.125);
        assert_eq!(darwin_powf(f32::from_bits(1), 1.0).to_bits(), 1);
        assert_eq!(darwin_powf(f32::MIN_POSITIVE, 0.5).to_bits(), 0x20000000);
        assert_eq!(darwin_powf(-0.0, 3.0).to_bits(), 0x80000000);
        assert!(darwin_powf(f32::MAX, 2.0).is_infinite());
        assert_eq!(darwin_powf(f32::MIN_POSITIVE, 2.0), 0.0);
        for n in [f32::NAN, f32::from_bits(0x7f800001)] {
            assert_eq!(darwin_powf(n, 0.0), 1.0);
            assert_eq!(darwin_powf(1.0, n), 1.0);
        }
        assert!(darwin_powf(-2.0, 0.5).is_nan());
        for n in [f32::NAN, f32::INFINITY] {
            let p = sincos_f(n);
            assert!(p.s.is_nan() && p.c.is_nan());
        }
    }
}
