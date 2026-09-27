// worldscale-engine: Vida's world-scale prototype in Rust.
//
// The model (the same sums as worldscale/forest.py, which follows Vida's
// own) and one rank's share of the world (worldscale/world.py). The Python
// module (../python) and the worldscale command (../cli) are both built on
// it.
//
// The maths functions (log, pow, sin...) come from the libm crate, written
// in Rust, so the answers are the same on every computer; and they don't
// depend on how the world is split up, or how many cores do the work.

// Vida uses 3.14 for pi and 0.7071067812 for one over the square root of
// two, so the engine does too, to give the same answers.
#![allow(clippy::approx_constant)]
// Loops over row numbers are easier to follow than iterator chains, and
// !(a > b) is written on purpose where a number might not be a number (NaN),
// as numpy's comparisons are.
#![allow(clippy::needless_range_loop, clippy::neg_cmp_op_on_partial_ord)]

pub mod comm;
pub mod forest;
pub mod growth;
pub mod maths;
pub mod pairs;
pub mod philox;
pub mod settings;
pub mod shading;
pub mod world;
