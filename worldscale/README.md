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
`-partition strips|curve|scattered`, `-shuffle`, `-engine numpy|rust`, and for comparison the old
ways, `-rng queue` and `-crush sequential`. MPI needs `mpi4py` and an MPI
library (`pip install mpi4py mpich` works on Linux).

The tests (`tests/unit/test_worldscale.py`) run several ranks as threads,
so they don't need MPI.

## The compiled engine

`-engine rust` does each rank's sums with the Rust code in `rust/` (see
`rust/README.md` for building it and why Rust). It grows exactly the same
trees as the numpy engine, with values the same to within one part in a
billion, and like it gives the same forest on any number of ranks.

On the 800 m world (40 cycles, ending with about 253,000 trees and seeds):

| | numpy | Rust |
|---|---|---|
| 1 process | 20.4 s | 6.7 s |
| 4 processes | 6.0 s | 2.6 s |
| per tree per cycle, grown forest, one core | 9.4 µs | 2.2 µs |

What's left is mostly Vida's own maths (about seven `pow` and `log` a plant
a cycle, and hundreds of photons for each shaded plant), plus the Python
that moves trees between steps and the pickled messages between ranks.

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
- `compiled.py`: the same functions as `forest.py`, done by the Rust engine
- `rust/`: the Rust engine
- `run.py`: the command line
