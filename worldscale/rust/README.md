# worldscale_core: the compiled engine

The same model as `worldscale/forest.py`, written in Rust, for one rank's
share of the world. `worldscale/compiled.py` calls it with the same
functions and arguments as `forest.py`, so `world.py` can use either:

    python -m worldscale.run -w 800 -t 40 -engine rust
    mpiexec -n 4 python -m worldscale.run -mpi -w 800 -t 40 -engine rust

## Building it

It needs Rust (https://rustup.rs) and maturin (`pip install maturin`). From
this folder, with Vida's Python environment active:

    maturin develop --release

That builds the `worldscale_core` module and installs it into the
environment. The tests in `tests/unit/test_worldscale.py` that use it are
skipped when it isn't built.

## Why Rust

- **The same answers on every computer.** Rust never rearranges or fuses
  floating-point sums behind your back (C++ compilers fuse multiply-adds
  by default), and the maths functions (`log`, `pow`, `sin`...) come from
  the `libm` crate, written in Rust, instead of whatever maths library the
  computer has. So the same run gives the same forest, bit for bit, on a
  laptop or a supercomputer. That costs about 7% of the speed:
  `maturin develop --release --features system-maths` uses the computer's
  own maths library instead.
- **As fast as C, without C's memory bugs**, in a code that ecologists
  will look after.
- **Easy to build anywhere**, without administrator rights, and it plugs
  straight into the Python code.

Its weak spot is GPUs. If Vida goes to GPU machines, the few kernels that
matter (the random numbers, finding neighbours and the photons) would be
ported to CUDA or HIP.

## What it does

The same sums as `forest.py`, in the same order, one tree at a time:
germinating, growing, making and throwing seeds, the deaths each tree
decides for itself, finding overlapping stems and who wins, shading
(photons included) and photosynthesis. It works on the numpy arrays in
place, and takes dead trees out by moving the rest to the front of each
column (`compact`), without copying the table.

Its random numbers are exactly `philox.py`'s. The rest can differ from
numpy's in the last digits, because its maths functions are its own; over
22 cycles of a test world it grows exactly the same trees as `forest.py`
(the same births, deaths and crushes), with values the same to within one
part in a billion.

Two things are done differently from `forest.py`, giving the same answers
faster: overlapping circles are found on one grid (a counting sort) with
each circle looking only as far as its own size needs, and a photon is
tested against a canopy by comparing squared distances instead of calling
`hypot`.

## Files

- `src/philox.rs`: the addressed random numbers
- `src/settings.rs`: species and world settings, read from the Python objects
- `src/growth.rs`: germinating, growing, seeds, deaths, photosynthesis
- `src/pairs.rs`: finding overlapping circles, and who wins an overlap
- `src/shading.rs`: shading
- `src/maths.rs`: which maths functions to use
- `src/lib.rs`: what Python sees
