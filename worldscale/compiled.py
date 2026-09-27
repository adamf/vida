"""This file is part of Vida.
    --------------------------
    Copyright 2026, Sean T. Hammond

    Vida is experimental in nature and is made available as a research courtesy "AS IS," but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.

    You should have received a copy of academic software agreement along with Vida. If not, see <https://github.com/seanth/Vida/blob/master/LICENSE.txt>.
"""

###The compiled engine, done by the Rust code in worldscale/rust (the
###worldscale_core module), used two ways:
###  -engine rust: the functions below, with the same arguments as
###      forest.py's, which world.py uses instead of forest.py's
###  -engine rust-world: CompiledWorld, one rank's whole share of the world
###      with every step of the cycle in Rust, used instead of world.py's
###      TiledWorld. It grows exactly the same forest as -engine rust.
###Both share each rank's work out between the computer's cores.
###
###Build it once, from worldscale/rust/python:  maturin develop --release
###
###The Rust code does the same sums in the same order. Its maths functions
###(log, pow, sin...) are its own, so the last digit of a result can differ
###from numpy's now and then; a run with it is its own reference, the same
###on any number of ranks, and close to the numpy engine's on average.

import numpy

import worldscale_core as core

from worldscale import forest as trees

ALL_BITS = (1 << 64) - 1


def addDeaths(deaths, counts):
    for place in range(len(trees.CAUSES)):
        deaths[trees.CAUSES[place]] += int(counts[place])


def germinate(forest, cycle, table, world, rngStart, random, deaths):
    dying, counts = core.germinate(forest, cycle, table, world, rngStart & ALL_BITS)
    addDeaths(deaths, counts)
    return dying


def grow(forest, plants, table, world, deaths):
    dying, mothers, counts, masses, deathCounts = core.grow(forest, plants.astype(numpy.int64), table, world)
    addDeaths(deaths, deathCounts)
    return dying, mothers, counts, masses


def disperse(forest, mothers, counts, masses, cycle, table, world, rngStart, random):
    columns = core.disperse(forest, mothers.astype(numpy.int64), counts.astype(numpy.int64),
                            masses.astype(numpy.float64), cycle, table, world, rngStart & ALL_BITS)
    seeds = trees.Forest(len(columns["x"]))
    seeds.species = columns["species"]
    seeds.isSeed[:] = True
    seeds.x = columns["x"]
    seeds.y = columns["y"]
    seeds.birthCycle[:] = cycle
    seeds.countToGerm = columns["countToGerm"]
    seeds.massSeed = columns["massSeed"]
    seeds.radiusSeed = columns["radiusSeed"]
    seeds.r = columns["radiusSeed"].copy()
    seeds.massTotal = columns["massSeed"].copy()
    return seeds, columns["motherId"], columns["seedNumber"]


def ownDeaths(forest, alreadyDying, cycle, table, world, rngStart, random, deaths):
    dying, counts = core.own_deaths(forest, numpy.ascontiguousarray(alreadyDying, dtype=bool), cycle, table, world,
                                    rngStart & ALL_BITS)
    addDeaths(deaths, counts)
    return dying


def findPairs(x, y, radius):
    return core.find_pairs(numpy.ascontiguousarray(x, dtype=numpy.float64),
                           numpy.ascontiguousarray(y, dtype=numpy.float64),
                           numpy.ascontiguousarray(radius, dtype=numpy.float64))


def overlapWinners(x, y, radius, massTotal, birthCycle, ids, ownedCount):
    return core.overlap_winners(numpy.ascontiguousarray(x, dtype=numpy.float64),
                                numpy.ascontiguousarray(y, dtype=numpy.float64),
                                numpy.ascontiguousarray(radius, dtype=numpy.float64),
                                numpy.ascontiguousarray(massTotal, dtype=numpy.float64),
                                numpy.ascontiguousarray(birthCycle, dtype=numpy.int32),
                                numpy.ascontiguousarray(ids, dtype=numpy.uint64), ownedCount)


def shade(ownedCount, x, y, r, isPlant, heightStem, ids, species, cycle, table, world, photonLimit, rngStart, random):
    return core.shade(ownedCount, numpy.ascontiguousarray(x, dtype=numpy.float64),
                      numpy.ascontiguousarray(y, dtype=numpy.float64),
                      numpy.ascontiguousarray(r, dtype=numpy.float64),
                      numpy.ascontiguousarray(isPlant, dtype=bool),
                      numpy.ascontiguousarray(heightStem, dtype=numpy.float64),
                      numpy.ascontiguousarray(ids, dtype=numpy.uint64),
                      numpy.ascontiguousarray(species, dtype=numpy.int32),
                      cycle, table, world, int(photonLimit), rngStart & ALL_BITS)


def photosynthesise(forest, table, deaths):
    dying, counts = core.photosynthesise(forest, table)
    addDeaths(deaths, counts)
    return dying


def compact(forest, keep):
    return core.compact(forest, numpy.ascontiguousarray(keep, dtype=bool))


def randomBlocks(rngStart, ids, cycle, purpose, index):
    ids = numpy.asarray(ids, dtype=numpy.uint64)
    index = numpy.asarray(index, dtype=numpy.int64)
    ids, index = numpy.broadcast_arrays(ids, index)
    return core.random_blocks(rngStart & ALL_BITS, numpy.ascontiguousarray(ids), cycle, purpose,
                              numpy.ascontiguousarray(index))


def setThreads(threads):
    ###how many cores the functions above use (only before they first run)
    core.set_threads(threads)


class CompiledWorld:
    ###One rank's share of the world, with every step of the cycle done by
    ###the Rust engine (worldscale_core.RankWorld). It works like
    ###world.TiledWorld, and talks to the other ranks through the same comm.
    def __init__(self, comm, settings, table, worldSettings):
        if settings.rng != "addressed" or settings.crush != "rounds":
            raise ValueError("the Rust world only has addressed random numbers and crushing in rounds")
        self.comm = comm
        self.settings = settings
        worldSettings.worldSize = settings.worldSize
        self.world = core.RankWorld(comm, table, worldSettings, settings.worldSize, settings.tileSize,
                                    settings.seedsPerHectare, settings.rngStart, settings.photonLimit,
                                    settings.partition, settings.shuffle, settings.threads)
        self.random = None
        self.cycle = 0
        self.timings = self.world.timings()
        self.traffic = self.world.traffic()

    def runCycle(self):
        result = self.world.run_cycle()
        self.cycle = self.cycle + 1
        self.timings = self.world.timings()
        self.traffic = self.world.traffic()
        return result

    def fingerprint(self):
        return self.world.fingerprint()

    def rowCount(self):
        return self.world.rows()
