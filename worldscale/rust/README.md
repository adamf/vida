# The world-scale prototype in Rust

The same model as `worldscale/forest.py` and the same world as
`worldscale/world.py`, written in Rust. Three crates build together:

- `engine`: the model, and one rank's whole share of the world: the tiles,
  starting seeds, growth, seeds going to other ranks, crushing in rounds,
  shading with halos, the fingerprint. No Python and no MPI in it.
- `python`: the `worldscale_core` Python module, built on the engine.
- `cli`: the `worldscale` command, which runs a world with no Python at
  all, on one computer or over MPI.

Each rank's work is shared out between the cores of its computer (with
the `rayon` crate), so a run can use MPI between computers and threads
inside each one. Every way of running it grows exactly the same forest as
`python -m worldscale.run -engine rust`, bit for bit, on any number of
ranks and cores.

## From Python

    python -m worldscale.run -w 800 -t 40 -engine rust
    python -m worldscale.run -w 800 -t 40 -engine rust-world
    mpiexec -n 4 python -m worldscale.run -mpi -w 800 -t 40 -engine rust-world -threads 1

- `-engine rust`: `world.py` runs the cycle, and the Rust engine does the
  sums (`compiled.py`'s functions, which work on the numpy arrays in place).
- `-engine rust-world`: every step of the cycle is done in Rust
  (`compiled.py`'s `CompiledWorld`). Python only starts it and passes the
  letters between ranks, through the same comm as `world.py`
  (`worldscale/comm.py`), so it runs as threads in the tests and over MPI
  with mpi4py.
- `-threads`: how many cores each rank uses (all of them if it isn't given).
  With several ranks on one computer, give each rank its share.

To build the module, you need Rust (https://rustup.rs) and maturin
(`pip install maturin`). From `python/`, with Vida's Python environment
active:

    maturin develop --release

The tests in `tests/unit/test_worldscale.py` that use it are skipped when
it isn't built.

## Without Python: the worldscale command

From this folder:

    cargo build --release -p worldscale-cli

Then, from the folder with `Vida.py` in it (it reads Vida's species files
and `Vida World Preferences.yml` itself):

    worldscale/rust/target/release/worldscale -w 800 -t 40
    worldscale/rust/target/release/worldscale -w 800 -t 40 -ranks 4

It takes the same options as `worldscale.run` (`-w`, `-tile`, `-s`, `-t`,
`-rngstart`, `-photons`, `-partition`, `-shuffle`, `-threads`, `-species`,
`-json`, `-quiet`), and `-ranks N` runs N ranks as threads of one process
(to check that the answer doesn't change). `-help` lists them.

For MPI, build it with the `mpi` feature. That needs an MPI library and
libclang (for `bindgen`); `pip install mpich` gives an MPI library that
works, with its `mpicc` on the `PATH`:

    cargo build --release -p worldscale-cli --features mpi
    mpiexec -n 4 worldscale/rust/target/release/worldscale -w 800 -t 40 -threads 1

## The tests

    cargo test --release -p worldscale-engine

They check the random numbers against Random123's, that the forest is the
same on 1 to 4 ranks and on 1 to 8 cores, that crushing across ranks gives
exactly the one-at-a-time strongest-first answer, and that the forest is
the one the Python prototype grows (its fingerprint).

## Why Rust

- **The same answers on every computer.** Rust never rearranges or fuses
  floating-point sums behind your back (C++ compilers fuse multiply-adds
  by default), and the maths functions (`log`, `pow`, `sin`...) come from
  the `libm` crate, written in Rust, instead of whatever maths library the
  computer has. So the same run gives the same forest, bit for bit, on a
  laptop or a supercomputer. That costs about 7% of the speed:
  `--features system-maths` uses the computer's own maths library instead.
- **As fast as C, without C's memory bugs**, and threads that can't
  trample each other's memory: the compiler checks that two cores never
  write the same thing at once.
- **Easy to build anywhere**, without administrator rights, and it plugs
  straight into the Python code.

Its weak spot is GPUs. If Vida goes to GPU machines, the few kernels that
matter (the random numbers, finding neighbours and the photons) would be
ported to CUDA or HIP.

## How it does it

The same sums as `forest.py`, in the same order, one tree at a time. The
table of trees is cut into pieces of 2,048 rows, and the cores take
pieces; what they give back is put together in row order, so nothing
depends on how many cores there are. Shading is shared out a plant at a
time, and finding overlapping circles a run of the grid at a time.

The starting seeds, and each cycle's new seeds, are put in the table in
order of where they stand (along a Z-order curve), so trees near each other
on the ground are mostly near each other in memory. On a 3.2 km world that
made a run 18% faster, as finding neighbours waits less for memory. The
order of the rows never changes the answer.

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

Ranks send each other letters of bytes: new seeds, and copies of the trees
near their tiles' edges (just the values needed). A letter must be under
2 GB, and each rank keeps a table of which rank has each tile, which is
fine up to tens of millions of tiles.

## Files

- `engine/src/philox.rs`: the addressed random numbers
- `engine/src/settings.rs`: species and world settings, read from Vida's files
- `engine/src/forest.rs`: the table of trees, and letters between ranks
- `engine/src/growth.rs`: germinating, growing, seeds, deaths, photosynthesis
- `engine/src/pairs.rs`: finding overlapping circles, and who wins an overlap
- `engine/src/shading.rs`: shading
- `engine/src/comm.rs`: how ranks talk (one rank, or ranks as threads)
- `engine/src/world.rs`: tiles, halos, seeds between ranks, and the cycle
- `engine/src/maths.rs`: which maths functions to use
- `engine/tests/world.rs`: the tests
- `python/src/lib.rs`: what Python sees
- `cli/src/main.rs`: the `worldscale` command, and talking over MPI
