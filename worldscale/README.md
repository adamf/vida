# Vida at world scale: a prototype

Vida simulates every tree and seed in a patch of forest, one at a time, on
one processor. To simulate a forest the size of the Amazon (about 400
billion trees), the world has to be split over thousands of processors,
*and the answer mustn't depend on how it's split*. This folder is a working
prototype of the two ideas that make that possible:

1. **Random numbers with addresses** (`philox.py`). Vida takes its random
   numbers from one queue, so a plant's numbers depend on how many were
   taken before it. Here each number is worked out from its address (run,
   tree, cycle, purpose, index) with the Philox4x32-10 generator, so a
   tree's numbers are the same on any processor, in any order.
2. **Everyone decides, then everything happens** (`world.py`). Vida deals
   with plants one after another, and some steps see what earlier plants
   just did. Here each step reads the world as it was at the start of the
   step. The one step where trees really do affect each other, overlapping
   stems crushing each other, is decided by a rule that doesn't depend on
   order (strongest first). It is worked out in rounds, which neighbouring
   processors can do together.

The world is cut into fixed square tiles. Each processor (MPI rank) looks
after some of the tiles, and between steps it swaps copies of the trees
near its tiles' edges with its neighbours (the "halo").

The result: **the same forest, bit for bit, on 1, 2, 3 or 4 ranks, with the
tiles shared out in strips, along a space-filling curve or at random, and
with each rank's trees shuffled every cycle.** The `fingerprint` printed at
the end of a run checks every value of every tree and seed.

## Running it

From the folder with `Vida.py` in it:

    python -m worldscale.run -w 200 -t 40
    mpiexec -n 4 python -m worldscale.run -mpi -w 800 -t 40

Options: `-w` world width (m), `-tile` tile width (m), `-s` starting seeds
per hectare, `-t` cycles, `-rngstart`, `-photons` (most photons per plant),
`-partition strips|curve|scattered`, `-shuffle`, `-engine numpy|rust|rust-world`,
`-threads` (cores per rank, for the Rust engines), and for comparison the
old ways, `-rng queue` and `-crush sequential`. MPI needs `mpi4py` and an MPI
library (`pip install mpi4py mpich` works on Linux).

The tests (`tests/unit/test_worldscale.py`) run several ranks as threads,
so they don't need MPI.

## The compiled engine

`rust/` has the same model and the same world written in Rust (see
`rust/README.md` for building it and why Rust). There are three ways to use
it, and they all grow exactly the same forest, bit for bit, on any number of
ranks and cores:

- `-engine rust`: `world.py` runs the cycle and the Rust engine does the
  sums.
- `-engine rust-world`: every step of the cycle is in Rust, and Python only
  starts it and passes letters between ranks.
- the `worldscale` command: no Python at all, on one computer (with
  `-ranks` to run several ranks as threads) or over MPI.

Inside each rank, the Rust engine shares the work out between the
computer's cores, so a run can use MPI between computers and threads inside
each one. Its values are the same as the numpy engine's to within one part in
a billion (its maths functions are its own), with the same births, deaths
and crushes.

On the 800 m world (40 cycles, ending with about 253,000 trees and seeds), on
a computer with 4 cores:

| | 1 core | 4 cores, as threads | 4 MPI ranks |
|---|---|---|---|
| numpy | 20.4 s | | 5.8 s |
| `-engine rust` | 5.9 s | 3.1 s | 2.1 s |
| `-engine rust-world` | 4.8 s | 2.3 s | 1.6 s |
| `worldscale` command | 5.3 s | 2.3 s | 1.4 s |

A tree costs about 1.8 µs a cycle on one core (numpy: 10 µs), most of it
Vida's own maths (about seven `pow` and `log` a plant a cycle, and hundreds
of photons for each shaded plant). With 4 million trees on one core it's
2.2 µs, as more of the trees' neighbours are out of the processor's cache.
Ranks scale better than threads (3.7 times on 4 cores, against 2.3),
because some of each step (filing trees into the grid, taking dead ones
out) is still done on one core.

Bigger worlds, with the `worldscale` command on the same 4 cores:

| World | Trees and seeds after 40 cycles | 1 core | 4 threads | 4 MPI ranks | Memory |
|---|---|---|---|---|---|
| 3.2 km (1,024 ha) | 4.0 million | 91 s | 39 s | 26 s | 2.3 GB |
| 6.4 km (4,096 ha) | 16.2 million | | 4.1 min | | 8.7 GB |

That's about 540 bytes per tree or seed at the busiest moment of a cycle.

## The model

It reads Vida's own species files and `Vida World Preferences.yml`, and
uses Vida's sums for germination, growth (the allometry), making and
throwing seeds, random death, slow growth, buckling (Euler-Greenhill),
stems off the world, crushing, classic shading (one canopy exactly, two or
more with photons) and photosynthesis. With the same settings (a 100 m world,
400 seeds, 40 cycles, three runs each) its plant counts overlap the spread
of Vida's own runs for all 40 cycles and end 5% lower; its seed counts
overlap until cycle 35 and end 18% lower.

What's different, and why:

- **The random numbers** are different ones (from addresses), with the same
  distributions.
- **Crushing** is strongest first: a tree survives unless it overlaps a
  stronger one (heavier, or planted first) that survives. Vida goes down its
  list, so a tree can be crushed by one that is itself crushed a moment
  later, and the result depends on the list's order.
- **Photons land evenly on the plant's circle** (radius x sqrt(random)).
  Vida's `countPhotonsGettingThrough` uses sqrt(random x radius), which is
  only even when the radius is 1 m: on bigger plants the photons all land
  near the middle, and on smaller ones some land outside the plant. That
  looks like a bug in Vida.
- **Seeds on a plant grow together**, each with its share of the carbon
  (Vida gives each its share plus or minus a random amount), and a plant
  starts new seeds only when it has none growing.
- **Not included yet:** terrain and water, regions and events,
  Janzen-Connell mortality, seeds dropping when their mother dies, the
  sunmap shading, and Vida's output files and graphics.

## Files

- `philox.py`: the addressed random numbers
- `species.py`: species and world settings from Vida's files, as arrays
- `forest.py`: the trees as a table (one numpy array per setting), and
  Vida's sums done on whole columns at once
- `comm.py`: how ranks talk (one process, threads for the tests, or MPI)
- `world.py`: tiles, halos, seeds moving between ranks, and the cycle
- `compiled.py`: the same functions as `forest.py`, done by the Rust engine,
  and `CompiledWorld`, a rank's whole world in Rust
- `rust/`: the Rust engine, the Python module and the `worldscale` command
- `run.py`: the command line
