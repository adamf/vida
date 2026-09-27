"""This file is part of Vida.
    --------------------------
    Copyright 2026, Sean T. Hammond

    Vida is experimental in nature and is made available as a research courtesy "AS IS," but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.

    You should have received a copy of academic software agreement along with Vida. If not, see <https://github.com/seanth/Vida/blob/master/LICENSE.txt>.
"""

###Run the world-scale prototype. From the folder with Vida.py in it:
###
###    python -m worldscale.run -w 500 -t 30
###    mpiexec -n 4 python -m worldscale.run -w 500 -t 30 -mpi
###
###Every rank prints nothing but rank 0, which prints each cycle's counts,
###the time each step took (the slowest rank's), and at the end the world's
###fingerprint. Two runs with the same settings give the same fingerprint,
###however many ranks they're on and however the tiles are shared out.

import argparse
import json
import os
import sys
import time

from worldscale import comm as comms
from worldscale import species
from worldscale import world as worlds


def readOptions(arguments=None):
    parser = argparse.ArgumentParser(description="Vida's world-scale prototype")
    parser.add_argument("-w", type=float, default=200.0, dest="worldSize", help="width of the (square) world, in metres")
    parser.add_argument("-tile", type=float, default=50.0, dest="tileSize", help="width of a tile, in metres")
    parser.add_argument("-s", type=float, default=400.0, dest="seedsPerHectare", help="starting seeds per hectare")
    parser.add_argument("-t", type=int, default=30, dest="cycles", help="how many cycles to run")
    parser.add_argument("-rngstart", type=int, default=1, dest="rngStart", help="starting value for the random numbers")
    parser.add_argument("-photons", type=int, default=750, dest="photonLimit", help="most photons per plant (Vida uses 750)")
    parser.add_argument("-partition", default="strips", choices=["strips", "curve", "scattered"], help="how tiles are shared out")
    parser.add_argument("-rng", default="addressed", choices=["addressed", "queue"], help="addressed random numbers, or the old queue")
    parser.add_argument("-crush", default="rounds", choices=["rounds", "sequential"], help="decide overlaps in rounds, or the old way")
    parser.add_argument("-shuffle", action="store_true", help="shuffle each rank's trees every cycle (it shouldn't matter)")
    parser.add_argument("-engine", default="numpy", choices=["numpy", "rust"], help="numpy (forest.py) or the compiled Rust engine")
    parser.add_argument("-species", default="Species", help="folder of species files")
    parser.add_argument("-mpi", action="store_true", help="run on MPI (use with mpiexec)")
    parser.add_argument("-json", default=None, help="also write the results to this file")
    parser.add_argument("-quiet", action="store_true", help="only print the end")
    return parser.parse_args(arguments)


def runWorld(comm, options):
    ###Run one world on this rank. Gives back the per-cycle summaries, the
    ###fingerprint, and timings.
    vidaFolder = os.getcwd()
    table = species.SpeciesTable(species.speciesFilesIn(options.species), vidaFolder)
    settings = worlds.Settings(worldSize=options.worldSize, tileSize=options.tileSize,
                               seedsPerHectare=options.seedsPerHectare, rngStart=options.rngStart,
                               photonLimit=options.photonLimit, partition=options.partition,
                               rng=options.rng, crush=options.crush, shuffle=options.shuffle,
                               engine=options.engine)
    theWorld = worlds.TiledWorld(comm, settings, table, species.WorldSettings(vidaFolder))
    cycles = []
    started = time.perf_counter()
    for cycle in range(options.cycles):
        cycleStarted = time.perf_counter()
        result = theWorld.runCycle()
        result["seconds"] = comm.allreduce(time.perf_counter() - cycleStarted, "max")
        cycles.append(result)
        if comm.rank == 0 and not options.quiet:
            print("cycle %3d  plants %9d  seeds %9d  born %8d  crushed %7d  rounds %2d  %6.2f s"
                  % (result["cycle"], result["plants"], result["seeds"], result["born"],
                     result["deaths"]["crushed"], result["rounds"], result["seconds"]), flush=True)
    seconds = comm.allreduce(time.perf_counter() - started, "max")
    timings = {}
    for name in theWorld.timings:
        timings[name] = comm.allreduce(theWorld.timings[name], "max")
    rows = comm.allreduce(len(theWorld.forest), "max")
    traffic = {}
    for name in theWorld.traffic:
        traffic[name] = comm.allreduce(theWorld.traffic[name], "sum")
    blocks = comm.allreduce(getattr(theWorld.random, "blocksMade", 0), "sum")
    return {"cycles": cycles, "fingerprint": theWorld.fingerprint(), "seconds": seconds,
            "timings": timings, "ranks": comm.size, "largestRank": rows, "traffic": traffic,
            "randomBlocks": blocks}


def main():
    options = readOptions()
    if options.mpi:
        comm = comms.MpiComm()
    else:
        comm = comms.SerialComm()
    result = runWorld(comm, options)
    if comm.rank == 0:
        print("ranks %d, %s tiles, rng %s, crush %s, %s engine: %.1f s" % (comm.size, options.partition, options.rng, options.crush, options.engine, result["seconds"]))
        steps = []
        for name in result["timings"]:
            steps.append("%s %.1f s" % (name, result["timings"][name]))
        print("  slowest rank's time in each step: " + ", ".join(steps))
        copies = []
        for name in result["traffic"]:
            copies.append("%s %d" % (name, result["traffic"][name]))
        print("  over all the cycles: " + ", ".join(copies))
        print("  random number blocks made: %d" % result["randomBlocks"])
        print("fingerprint " + result["fingerprint"])
        if options.json:
            result["options"] = vars(options)
            with open(options.json, "w") as theFile:
                json.dump(result, theFile, indent=1)
    sys.stdout.flush()


if __name__ == "__main__":
    main()
