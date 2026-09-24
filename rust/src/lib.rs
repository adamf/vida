//! Vida's optional fast parts, in Rust (see README.md).
//!
//! The only one so far is the photon loop of the classic shading
//! (determineShade in Vida_Data/vworldr.py). It gives exactly the same
//! answers as the Python loop, because it does exactly the same sums:
//!
//! - the random numbers come from a copy of Python's own generator (the
//!   Mersenne Twister in CPython's Modules/_randommodule.c), started from
//!   Python's state and handed back to Python afterwards, so the Python code
//!   carries on with the same random numbers it would have had;
//! - distances are worked out as Python's math.hypot does (vector_norm in
//!   CPython 3.11's Modules/mathmodule.c), which is more careful than the
//!   usual hypot;
//! - cos, sin and the square root (x ** 0.5 in Python) call the same C
//!   library functions that Python calls.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyTuple;
use std::hint::black_box;

extern "C" {
    // the C library's functions, the ones Python's math.cos, math.sin and
    // ** call
    fn cos(x: f64) -> f64;
    fn sin(x: f64) -> f64;
    fn pow(x: f64, y: f64) -> f64;
}

// ---------------------------------------------------------------------------
// Python's random numbers
// ---------------------------------------------------------------------------

const N: usize = 624;
const M: usize = 397;
const MATRIX_A: u32 = 0x9908b0df;
const UPPER_MASK: u32 = 0x80000000;
const LOWER_MASK: u32 = 0x7fffffff;

/// The Mersenne Twister, as in CPython's _randommodule.c.
struct Twister {
    mt: [u32; N],
    index: usize,
}

impl Twister {
    fn next_u32(&mut self) -> u32 {
        let mag01 = [0u32, MATRIX_A];
        if self.index >= N {
            // make the next N numbers all at once
            let mt = &mut self.mt;
            let mut kk = 0;
            while kk < N - M {
                let y = (mt[kk] & UPPER_MASK) | (mt[kk + 1] & LOWER_MASK);
                mt[kk] = mt[kk + M] ^ (y >> 1) ^ mag01[(y & 1) as usize];
                kk += 1;
            }
            while kk < N - 1 {
                let y = (mt[kk] & UPPER_MASK) | (mt[kk + 1] & LOWER_MASK);
                mt[kk] = mt[kk + M - N] ^ (y >> 1) ^ mag01[(y & 1) as usize];
                kk += 1;
            }
            let y = (mt[N - 1] & UPPER_MASK) | (mt[0] & LOWER_MASK);
            mt[N - 1] = mt[M - 1] ^ (y >> 1) ^ mag01[(y & 1) as usize];
            self.index = 0;
        }
        let mut y = self.mt[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c5680;
        y ^= (y << 15) & 0xefc60000;
        y ^= y >> 18;
        y
    }

    /// random.random(): a number from 0 up to (not including) 1
    fn random(&mut self) -> f64 {
        let a = self.next_u32() >> 5;
        let b = self.next_u32() >> 6;
        (a as f64 * 67108864.0 + b as f64) * (1.0 / 9007199254740992.0)
    }
}

// ---------------------------------------------------------------------------
// Python's arithmetic
// ---------------------------------------------------------------------------

/// frexp: x = m * 2**e with 0.5 <= |m| < 1; gives e (x finite and not 0)
fn frexp_exponent(x: f64) -> i32 {
    let bits = x.to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as i32;
    if exponent == 0 {
        // a subnormal number: scale it up to a normal one first
        return frexp_exponent(x * 18014398509481984.0) - 54; // 2**54
    }
    exponent - 1022
}

/// ldexp(1.0, k): exactly 2**k
fn two_to_the(k: i32) -> f64 {
    if k > 1023 {
        f64::INFINITY
    } else if k >= -1022 {
        f64::from_bits(((k + 1023) as u64) << 52)
    } else if k >= -1074 {
        f64::from_bits(1u64 << (k + 1074))
    } else {
        0.0
    }
}

/// math.hypot(x, y), as CPython 3.11 works it out (vector_norm in
/// Modules/mathmodule.c), step for step.
fn python_hypot(x: f64, y: f64) -> f64 {
    let vec = [x.abs(), y.abs()];
    let mut max = 0.0;
    for &value in vec.iter() {
        if value > max {
            max = value;
        }
    }
    let found_nan = vec[0].is_nan() || vec[1].is_nan();
    if max.is_infinite() {
        return max;
    }
    if found_nan {
        return f64::NAN;
    }
    if max == 0.0 {
        return max;
    }
    const T27: f64 = 134217729.0; // 2**27 + 1
    let mut csum = 1.0;
    let mut frac1 = 0.0;
    let mut frac2 = 0.0;
    let mut frac3 = 0.0;
    let max_e = frexp_exponent(max);
    if max_e >= -1023 {
        let scale = two_to_the(-max_e);
        for &value in vec.iter() {
            let mut x = value * scale;
            let t = x * T27;
            let hi = t - (t - x);
            let lo = x - hi;
            x = hi * hi;
            let mut oldcsum = csum;
            csum += x;
            frac1 += (oldcsum - csum) + x;
            x = 2.0 * hi * lo;
            oldcsum = csum;
            csum += x;
            frac2 += (oldcsum - csum) + x;
            frac3 += lo * lo;
        }
        let h = (csum - 1.0 + (frac1 + frac2 + frac3)).sqrt();
        let mut x = h;
        let t = x * T27;
        let hi = t - (t - x);
        let lo = x - hi;
        x = -hi * hi;
        let mut oldcsum = csum;
        csum += x;
        frac1 += (oldcsum - csum) + x;
        x = -2.0 * hi * lo;
        oldcsum = csum;
        csum += x;
        frac2 += (oldcsum - csum) + x;
        x = -lo * lo;
        oldcsum = csum;
        csum += x;
        frac3 += (oldcsum - csum) + x;
        x = csum - 1.0 + (frac1 + frac2 + frac3);
        return (h + x / (2.0 * h)) / scale;
    }
    // tiny numbers: divide by the largest instead of scaling
    for &value in vec.iter() {
        let mut x = value / max;
        x = x * x;
        let oldcsum = csum;
        csum += x;
        frac1 += (oldcsum - csum) + x;
    }
    max * (csum - 1.0 + frac1).sqrt()
}

/// x ** 0.5 for a number that isn't negative, as Python's float_pow works it
/// out: the C library's pow (black_box stops the compiler turning it into a
/// square root, which can differ in the last digit).
fn python_square_root_power(x: f64) -> f64 {
    if x == 0.0 {
        return 0.0;
    }
    unsafe { pow(x, black_box(0.5)) }
}

// ---------------------------------------------------------------------------
// The photon loop
// ---------------------------------------------------------------------------

/// Drops photons on plants, for determineShade in vworldr.py, using (and
/// moving on) a copy of Python's random numbers. Make one from
/// random.getstate() before the first plant, and give its state() back to
/// random.setstate() after the last, with no other random numbers used in
/// between.
#[pyclass]
struct Photons {
    twister: Twister,
    version: Py<PyAny>,
    gauss_next: Py<PyAny>,
}

#[pymethods]
impl Photons {
    #[new]
    fn new(state: &Bound<'_, PyTuple>) -> PyResult<Self> {
        // random.getstate() is (version, (624 numbers and the index), gauss_next)
        if state.len() != 3 {
            return Err(PyValueError::new_err("expected random.getstate()"));
        }
        let version = state.get_item(0)?;
        let internal: Vec<u64> = state.get_item(1)?.extract()?;
        let gauss_next = state.get_item(2)?;
        if internal.len() != N + 1 || internal[N] > N as u64 {
            return Err(PyValueError::new_err("expected random.getstate()"));
        }
        let mut mt = [0u32; N];
        for i in 0..N {
            mt[i] = internal[i] as u32;
        }
        Ok(Photons {
            twister: Twister { mt, index: internal[N] as usize },
            version: version.unbind(),
            gauss_next: gauss_next.unbind(),
        })
    }

    /// the state to give back to random.setstate()
    fn state<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        let mut internal: Vec<u64> = Vec::with_capacity(N + 1);
        for &value in self.twister.mt.iter() {
            internal.push(value as u64);
        }
        internal.push(self.twister.index as u64);
        let internal = PyTuple::new(py, internal)?;
        PyTuple::new(py, [self.version.bind(py).clone(), internal.into_any(), self.gauss_next.bind(py).clone()])
    }

    /// the next random number, as random.random() would give it (for tests)
    fn random(&mut self) -> f64 {
        self.twister.random()
    }

    /// math.hypot(x, y), worked out here (for tests)
    #[staticmethod]
    fn hypot(x: f64, y: f64) -> f64 {
        python_hypot(x, y)
    }

    /// How many of `count` photons, dropped at random on a plant at (x, y)
    /// with radius r, get through the canopies above it. covers is a list
    /// of (x, y, radius, transmittance), in the order of the overlap list:
    /// a photon stops at the first canopy it lands in, unless it gets
    /// through that canopy, and either way isn't checked against the rest.
    fn count(&mut self, x: f64, y: f64, r: f64, count: i64, covers: Vec<(f64, f64, f64, f64)>) -> i64 {
        let two_pi = std::f64::consts::PI * 2.0;
        let mut hits = 0;
        for _ in 0..count {
            let mut randr = self.twister.random() * r;
            let rand_angle = self.twister.random() * two_pi;
            randr = python_square_root_power(randr);
            let photon_x = (randr * unsafe { cos(rand_angle) }) + x;
            let photon_y = (randr * unsafe { sin(rand_angle) }) + y;
            let mut blocked = false;
            for &(cover_x, cover_y, cover_r, cover_transmittance) in covers.iter() {
                if python_hypot(cover_x - photon_x, cover_y - photon_y) <= cover_r {
                    if self.twister.random() > cover_transmittance {
                        blocked = true;
                    }
                    break;
                }
            }
            if !blocked {
                hits += 1;
            }
        }
        hits
    }
}

#[pymodule]
fn vida_fast(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Photons>()?;
    Ok(())
}
