use dthd_darwin_abi::math::{darwin_powf, sincos_f};
use std::io::{self, Write};

fn random(s: &mut u32) -> u32 {
    *s ^= *s << 13;
    *s ^= *s >> 17;
    *s ^= *s << 5;
    *s
}

fn output(w: &mut impl Write, x: f32) -> io::Result<()> {
    w.write_all(&(if x.is_nan() { 0x7fc00000 } else { x.to_bits() }).to_le_bytes())
}

fn main() -> io::Result<()> {
    let mut w = io::BufWriter::new(io::stdout().lock());
    let mut s = 123456789;
    for i in 0..4_000_000 {
        let x = f32::from_bits(random(&mut s));
        let y = match i % 4 {
            0 => f32::from_bits(random(&mut s)),
            1 => 0.2,
            2 => 0.4,
            _ => (random(&mut s) % 65) as f32 - 32.0,
        };
        output(&mut w, darwin_powf(x, y))?;
    }
    s = 123456789;
    for i in 0..4_000_000 {
        let x = if i % 2 != 0 {
            f32::from_bits(random(&mut s))
        } else {
            random(&mut s) as i32 as f32 * f32::from_bits(0x31800000)
        };
        let p = sincos_f(x);
        output(&mut w, p.s)?;
        output(&mut w, p.c)?;
    }
    w.flush()
}
