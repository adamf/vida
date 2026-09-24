# vida_fast: an experiment, not for merging

A Rust version of the photon loop in classic shading
(`countPhotonsGettingThrough` in `Vida_Data/vworldr.py`) that gives exactly
the same results as the Python loop:

- a copy of Python's random number generator (the Mersenne Twister from
  CPython's `_randommodule.c`), started from `random.getstate()` and handed
  back with `random.setstate()`;
- `math.hypot` ported step for step from CPython 3.11's `vector_norm`;
- the C library's `cos`, `sin` and `pow`, as Python uses.

Checked: 5,000 random numbers and the generator's state, 3 million `hypot`
inputs, and 3,000 random photon jobs all match Python exactly, and the
characterization tests pass with it installed (and fail with a broken
stand-in, so they really use it).

**Result:** the loop itself is about 4x faster, but it's only about 8% of a
run, so a 100 m, 400-seed, 50-cycle run went from 10.6 s to 10.1 s. Not
worth adding a Rust toolchain to Vida for.

To try it: `cd rust && PYO3_PYTHON=python3 cargo build --release`, then
copy `target/release/libvida_fast.so` to `vida_fast.abi3.so` somewhere on
`PYTHONPATH`.
