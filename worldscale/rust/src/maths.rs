// The maths functions the engine uses. By default they come from the libm
// crate, written in Rust, so every computer gives exactly the same answers.
// Built with `--features system-maths` they come from the computer's own maths
// library instead: often faster, but its last digits can differ from one
// computer (or library version) to another.

#[cfg(not(feature = "system-maths"))]
mod chosen {
    pub fn pow(x: f64, y: f64) -> f64 { libm::pow(x, y) }
    pub fn log(x: f64) -> f64 { libm::log(x) }
    pub fn sin(x: f64) -> f64 { libm::sin(x) }
    pub fn cos(x: f64) -> f64 { libm::cos(x) }
    pub fn sincos(x: f64) -> (f64, f64) { libm::sincos(x) }
    pub fn asin(x: f64) -> f64 { libm::asin(x) }
    pub fn acos(x: f64) -> f64 { libm::acos(x) }
    pub fn hypot(x: f64, y: f64) -> f64 { libm::hypot(x, y) }
}

#[cfg(feature = "system-maths")]
mod chosen {
    pub fn pow(x: f64, y: f64) -> f64 { x.powf(y) }
    pub fn log(x: f64) -> f64 { x.ln() }
    pub fn sin(x: f64) -> f64 { x.sin() }
    pub fn cos(x: f64) -> f64 { x.cos() }
    pub fn sincos(x: f64) -> (f64, f64) { x.sin_cos() }
    pub fn asin(x: f64) -> f64 { x.asin() }
    pub fn acos(x: f64) -> f64 { x.acos() }
    pub fn hypot(x: f64, y: f64) -> f64 { x.hypot(y) }
}

pub use chosen::*;
