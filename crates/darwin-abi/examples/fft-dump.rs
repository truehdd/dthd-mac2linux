use dthd_darwin_abi::fft::*;
use std::io::{self, Write};

fn main() -> io::Result<()> {
    unsafe {
        let s = fft_create(16, 0);
        let mut b = vec![0.0; 1 << 16];
        let mut f = vec![0.0f32; 1 << 16];
        let mut out = io::BufWriter::new(io::stdout().lock());
        libc::srand(7);
        for r in 0..30 {
            for lg in 1..=16 {
                for direction in [1, -1] {
                    let n = 1usize << lg;
                    for i in 0..n {
                        b[i] = (libc::rand() as f64 / libc::RAND_MAX as f64 - 0.5)
                            * if r % 3 != 0 { 1.0 } else { 1e6 };
                        f[i] = b[i] as f32;
                    }
                    let z = Split {
                        r: b.as_mut_ptr(),
                        i: b.as_mut_ptr().add(n / 2),
                    };
                    fft_real_d(s, &z, 1, lg, direction);
                    for x in &b[..n] {
                        out.write_all(&x.to_le_bytes())?;
                    }
                    let z = Split {
                        r: f.as_mut_ptr(),
                        i: f.as_mut_ptr().add(n / 2),
                    };
                    fft_real_f(s, &z, 1, lg, direction);
                    for x in &f[..n] {
                        out.write_all(&x.to_le_bytes())?;
                    }
                }
            }
        }
        fft_destroy(s);
        out.flush()
    }
}
