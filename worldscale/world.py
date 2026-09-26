"""This file is part of Vida.
    --------------------------
    Copyright 2026, Sean T. Hammond

    Vida is experimental in nature and is made available as a research courtesy "AS IS," but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.

    You should have received a copy of academic software agreement along with Vida. If not, see <https://github.com/seanth/Vida/blob/master/LICENSE.txt>.
"""

###One world, split over many processors.
###
###The world is cut into square tiles (50 m, say). The tiles are fixed: they
###are part of the model, like its grid. Which processor (rank) looks after
###which tiles is separate, and can be anything: strips, blocks along a
###space-filling curve, or scattered at random. A tree belongs to the rank
###that has the tile it stands in.
###
###Each cycle is done in steps that every rank does together:
###  1. germinate and grow: each tree on its own, from its own values
###  2. seeds that were thrown go to the rank of the tile they land in
###  3. deaths each tree decides for itself (random death, buckling...)
###  4. overlapping stems and seeds: the heavier crushes the lighter,
###     decided in rounds, swapping decisions with neighbouring ranks
###  5. shading, from a copy of the neighbours' plants near the tile edges
###  6. photosynthesis: each plant on its own
###Between the steps, ranks swap copies of the trees near their tiles'
###edges (the "halo"), so each rank sees everything that can shade or crush
###its own trees.
###
###Nothing depends on which rank does what, or in what order: every random
###number has an address (philox.py), every decision about an overlap is
###made the same way on every rank, and trees get ids from their mother's
###tile. So 1, 2, 4 or 10,000 ranks, and any way of sharing out the tiles,
###give exactly the same forest. fingerprint() checks that.

import math
import time

import numpy

from worldscale import forest as trees
from worldscale import philox

TILE_BITS = 40   #a tree id is (tile number << 40) | (number born in that tile)


class Settings:
    ###what a run is: the world, the random numbers and how it's split up
    def __init__(self, worldSize=200.0, tileSize=50.0, seedsPerHectare=400.0, rngStart=1,
                 photonLimit=750, partition="strips", rng="addressed", crush="rounds", shuffle=False):
        self.worldSize = worldSize
        self.tileSize = tileSize
        self.seedsPerHectare = seedsPerHectare
        self.rngStart = rngStart
        self.photonLimit = photonLimit
        self.partition = partition   #strips, curve or scattered
        self.rng = rng               #addressed, or queue (the old way, for comparison)
        self.crush = crush           #rounds, or sequential (the old way, for comparison)
        self.shuffle = shuffle       #shuffle each rank's rows every cycle (it mustn't matter)


def interleave(values):
    ###spread a number's bits out, one every other place (for the Z-order curve)
    values = values.astype(numpy.uint64)
    result = numpy.zeros(len(values), dtype=numpy.uint64)
    for bit in range(32):
        result |= ((values >> numpy.uint64(bit)) & numpy.uint64(1)) << numpy.uint64(2 * bit)
    return result


def tileOwners(tilesAcross, ranks, partition):
    ###Which rank looks after each tile (an array, tilesAcross x tilesAcross)
    column, row = numpy.meshgrid(numpy.arange(tilesAcross), numpy.arange(tilesAcross), indexing="ij")
    column = column.ravel()
    row = row.ravel()
    count = len(column)
    if partition == "strips":
        owner = column * ranks // tilesAcross
    elif partition == "curve":
        ###along a Z-order (Morton) curve, cut into equal lengths: nearby
        ###tiles mostly go to the same rank
        order = numpy.argsort(interleave(column) | (interleave(row) << numpy.uint64(1)), kind="stable")
        owner = numpy.empty(count, dtype=numpy.int64)
        owner[order] = numpy.arange(count) * ranks // count
    elif partition == "scattered":
        ###each tile to a rank at random: the worst case for talking to neighbours
        mixed = philox.philox4x32(column, row, numpy.zeros(count), numpy.zeros(count), 12345, 678)[0]
        owner = mixed.astype(numpy.int64) % ranks
    else:
        raise ValueError("partition must be strips, curve or scattered")
    return owner.reshape(tilesAcross, tilesAcross)


class TiledWorld:
    def __init__(self, comm, settings, table, worldSettings):
        self.comm = comm
        self.settings = settings
        self.table = table
        self.world = worldSettings
        self.world.worldSize = settings.worldSize
        self.rngStart = settings.rngStart
        if settings.rng == "addressed":
            self.random = philox.AddressedRandom()
        else:
            self.random = philox.QueueRandom(settings.rngStart)
        self.tileSize = settings.tileSize
        self.tilesAcross = int(math.ceil(settings.worldSize / settings.tileSize))
        self.owner = tileOwners(self.tilesAcross, comm.size, settings.partition)
        self.half = settings.worldSize / 2.0
        myColumns, myRows = numpy.nonzero(self.owner == comm.rank)
        self.myTiles = myColumns * self.tilesAcross + myRows
        corners = numpy.stack((myColumns * self.tileSize - self.half, myRows * self.tileSize - self.half), axis=1)
        seedsPerTile = int(round(settings.seedsPerHectare * self.tileSize * self.tileSize / 10000.0))
        self.forest = trees.startingSeeds(self.myTiles, corners, self.tileSize, seedsPerTile, settings.worldSize,
                                          table, self.rngStart, self.random)
        ###each tile numbers the seeds born there, carrying on from its starting seeds
        self.bornInTile = numpy.zeros(self.tilesAcross * self.tilesAcross, dtype=numpy.int64)
        self.bornInTile[self.myTiles] = seedsPerTile
        self.cycle = 0
        self.timings = {}
        ###how much each rank copies to others, for the run's summary
        self.traffic = {"trees looked after": 0, "halo copies for crushing": 0,
                        "halo copies for shading": 0, "seeds sent to another rank": 0}
        self.shuffler = numpy.random.default_rng(1000 + comm.rank)

    ###-----------------------------------------------------------------
    ###Tiles
    ###-----------------------------------------------------------------

    def tileOf(self, x, y):
        column = numpy.clip(numpy.floor((x + self.half) / self.tileSize).astype(numpy.int64), 0, self.tilesAcross - 1)
        row = numpy.clip(numpy.floor((y + self.half) / self.tileSize).astype(numpy.int64), 0, self.tilesAcross - 1)
        return column, row

    def haloFor(self, rows, width):
        ###For each rank, which of these rows it needs a copy of: those within
        ###`width` of a tile it looks after. width must be no more than a
        ###tile, so only the 8 tiles round a tree's own can need it.
        if width > self.tileSize:
            raise ValueError("something reaches %.1f m, more than a tile (%.1f m): use bigger tiles (-tile)" % (width, self.tileSize))
        wanted = [numpy.zeros(len(rows), dtype=bool) for rank in range(self.comm.size)]
        if len(rows) == 0:
            return [rows[wanted[rank]] for rank in range(self.comm.size)]
        x = self.forest.x[rows]
        y = self.forest.y[rows]
        column, row = self.tileOf(x, y)
        insideX = x + self.half - column * self.tileSize
        insideY = y + self.half - row * self.tileSize
        for across in (-1, 0, 1):
            for up in (-1, 0, 1):
                if across == 0 and up == 0:
                    continue
                otherColumn = column + across
                otherRow = row + up
                exists = (otherColumn >= 0) & (otherColumn < self.tilesAcross) & (otherRow >= 0) & (otherRow < self.tilesAcross)
                near = exists.copy()
                if across == -1:
                    near &= insideX < width
                if across == 1:
                    near &= insideX > self.tileSize - width
                if up == -1:
                    near &= insideY < width
                if up == 1:
                    near &= insideY > self.tileSize - width
                who = self.owner[numpy.where(exists, otherColumn, 0), numpy.where(exists, otherRow, 0)]
                for rank in range(self.comm.size):
                    if rank != self.comm.rank:
                        wanted[rank] |= near & (who == rank)
        return [rows[wanted[rank]] for rank in range(self.comm.size)]

    def swapHalo(self, sends, columns):
        ###Send each rank copies of the rows it needs (just these columns).
        ###Gives back the columns of everything received, joined up in rank
        ###order.
        letters = []
        for rank in range(self.comm.size):
            letter = {}
            for name in columns:
                letter[name] = getattr(self.forest, name)[sends[rank]]
            letters.append(letter)
        received = self.comm.alltoall(letters)
        joined = {}
        for name in columns:
            parts = []
            for letter in received:
                parts.append(letter[name])
            joined[name] = numpy.concatenate(parts)
        return joined

    def swapValues(self, sends, values):
        ###Send each rank one value for each row it has a copy of (in the same
        ###order as swapHalo), and get back the values for our copies.
        letters = []
        for rank in range(self.comm.size):
            letters.append(values[sends[rank]])
        return numpy.concatenate(self.comm.alltoall(letters))

    ###-----------------------------------------------------------------
    ###One cycle
    ###-----------------------------------------------------------------

    def timeStep(self, name, started):
        self.timings[name] = self.timings.get(name, 0.0) + time.perf_counter() - started
        return time.perf_counter()

    def runCycle(self):
        cycle = self.cycle
        table = self.table
        deaths = {}
        for cause in trees.CAUSES:
            deaths[cause] = 0
        started = time.perf_counter()

        ###1. germinate and grow, each tree on its own
        wasPlant = numpy.nonzero(~self.forest.isSeed)[0]
        dying = trees.germinate(self.forest, cycle, table, self.world, self.rngStart, self.random, deaths)
        grown, mothers, counts, masses = trees.grow(self.forest, wasPlant, table, self.world, deaths)
        dying |= grown
        seeds, motherIds, seedNumbers = trees.disperse(self.forest, mothers, counts, masses, cycle, table, self.world,
                                                       self.rngStart, self.random)
        seeds.id = self.newIds(self.forest.x[numpy.repeat(mothers, counts)], self.forest.y[numpy.repeat(mothers, counts)],
                               motherIds, seedNumbers)
        onWorld = (seeds.x >= -self.half) & (seeds.x < self.half) & (seeds.y >= -self.half) & (seeds.y < self.half)
        deaths[trees.CAUSES[9]] += int((~onWorld).sum())
        seeds = seeds.take(onWorld)
        self.forest = self.forest.take(~dying)
        started = self.timeStep("grow", started)

        ###2. seeds go to the rank of the tile they land in
        column, row = self.tileOf(seeds.x, seeds.y)
        destination = self.owner[column, row]
        self.traffic["seeds sent to another rank"] += int(numpy.sum(destination != self.comm.rank))
        letters = []
        for rank in range(self.comm.size):
            letters.append(seeds.take(destination == rank).asDict())
        arrived = []
        for letter in self.comm.alltoall(letters):
            arrived.append(trees.forestFromDict(letter))
        self.forest = trees.joinForests([self.forest] + arrived)
        born = len(seeds)
        started = self.timeStep("send seeds", started)

        ###3. deaths each tree decides for itself
        dying = trees.ownDeaths(self.forest, cycle, table, self.world, self.rngStart, self.random, deaths)
        self.forest = self.forest.take(~dying)
        started = self.timeStep("own deaths", started)

        ###4. overlapping stems and seeds
        if not self.world.allowOverlaps:
            crushed, rounds = self.crush()
            deaths[trees.CAUSES[7]] += int(crushed.sum())
            self.forest = self.forest.take(~crushed)
        else:
            rounds = 0
        started = self.timeStep("crush", started)

        ###5. shading
        self.forest.areaCovered = self.shade()
        started = self.timeStep("shade", started)

        ###6. photosynthesis
        dying = trees.photosynthesise(self.forest, table, deaths)
        self.forest = self.forest.take(~dying)
        started = self.timeStep("photosynthesis", started)

        if self.settings.shuffle:
            self.forest = self.forest.take(self.shuffler.permutation(len(self.forest)))
        self.cycle = cycle + 1
        return self.summary(deaths, born, rounds)

    def newIds(self, motherX, motherY, motherIds, seedNumbers):
        ###Ids for seeds born this cycle: each mother's tile numbers its new
        ###seeds in order of (mother's id, seed number), carrying on from the
        ###last number it gave out. The same ids whichever rank has the tile.
        if len(motherIds) == 0:
            return numpy.zeros(0, dtype=numpy.uint64)
        column, row = self.tileOf(motherX, motherY)
        tile = column * self.tilesAcross + row
        order = numpy.lexsort((seedNumbers, motherIds, tile))
        sortedTiles = tile[order]
        firstInTile = numpy.searchsorted(sortedTiles, sortedTiles, side="left")
        numberInTile = numpy.arange(len(order)) - firstInTile + self.bornInTile[sortedTiles]
        ids = numpy.empty(len(order), dtype=numpy.uint64)
        ids[order] = (sortedTiles.astype(numpy.uint64) << numpy.uint64(TILE_BITS)) | numberInTile.astype(numpy.uint64)
        tiles, howMany = numpy.unique(sortedTiles, return_counts=True)
        self.bornInTile[tiles] += howMany
        return ids

    ###-----------------------------------------------------------------
    ###4. Overlapping stems and seeds
    ###-----------------------------------------------------------------

    def crush(self):
        ###Vida's removeOverlaps: when two stems (or seeds) overlap, the
        ###heavier one crushes the lighter. Vida goes down its list one at a
        ###time, and what's already been crushed can't crush anything else,
        ###so the answer depends on the order of the list.
        ###
        ###Here the rule is the same everywhere: a tree survives unless it
        ###overlaps a stronger tree (heavier, or planted first) that survives.
        ###That's decided in rounds. In each round a tree whose stronger
        ###neighbours have all been decided is decided: crushed if any of them
        ###survived, surviving if none did. Ranks swap the decisions about the
        ###trees near their edges after each round, until nothing is left
        ###undecided. It's the same answer as going down the list strongest
        ###first, one at a time, but many trees are decided at once.
        count = len(self.forest)
        radius = numpy.where(self.forest.isSeed, self.forest.radiusSeed, self.forest.radiusStem)
        largest = self.comm.allreduce(float(radius.max()) if count else 0.0, "max")
        sends = self.haloFor(numpy.arange(count), 2.0 * largest * 1.000001 + 0.000001)
        columns = ["id", "x", "y", "isSeed", "radiusSeed", "radiusStem", "massTotal", "birthCycle"]
        halo = self.swapHalo(sends, columns)
        self.traffic["trees looked after"] += count
        self.traffic["halo copies for crushing"] += len(halo["id"])
        x = numpy.concatenate((self.forest.x, halo["x"]))
        y = numpy.concatenate((self.forest.y, halo["y"]))
        haloRadius = numpy.where(halo["isSeed"], halo["radiusSeed"], halo["radiusStem"])
        allRadius = numpy.concatenate((radius, haloRadius))
        first, second = trees.findPairs(x, y, allRadius)
        place = trees.strongerFirst(numpy.concatenate((self.forest.massTotal, halo["massTotal"])),
                                    numpy.concatenate((self.forest.birthCycle, halo["birthCycle"])),
                                    numpy.concatenate((self.forest.id, halo["id"])))
        firstWins = place[first] < place[second]
        winner = numpy.where(firstWins, first, second)
        loser = numpy.where(firstWins, second, first)
        ###only our own trees' fates are ours to decide
        mine = loser < count
        winner = winner[mine]
        loser = loser[mine]
        if self.settings.crush == "sequential":
            return self.crushOneAtATime(winner, loser, count), 1
        UNDECIDED = 0
        ALIVE = 1
        CRUSHED = 2
        status = numpy.zeros(len(x), dtype=numpy.int8)
        rounds = 0
        while True:
            rounds = rounds + 1
            killedBy = numpy.bincount(loser[status[winner] == ALIVE], minlength=count) > 0
            waitingFor = numpy.bincount(loser[status[winner] == UNDECIDED], minlength=count) > 0
            undecided = status[:count] == UNDECIDED
            newStatus = status[:count].copy()
            newStatus[undecided & killedBy] = CRUSHED
            newStatus[undecided & ~killedBy & ~waitingFor] = ALIVE
            status[:count] = newStatus
            status[count:] = self.swapValues(sends, status[:count])
            left = self.comm.allreduce(int(numpy.sum(status[:count] == UNDECIDED)), "sum")
            if left == 0:
                break
        return status[:count] == CRUSHED, rounds

    def crushOneAtATime(self, winner, loser, count):
        ###The old way, for comparison: down this rank's own list in order,
        ###each tree crushing (or being crushed by) the first overlapping tree
        ###still standing, as Vida's removeOverlaps does. Each rank only knows
        ###about its own trees being crushed, so the answer depends on the
        ###order of the list and on how the world is split up.
        crushed = numpy.zeros(count, dtype=bool)
        haloGone = set()
        partners = {}
        for w, l in zip(winner.tolist(), loser.tolist()):
            partners.setdefault(l, []).append((w, True))
            if w < count:
                partners.setdefault(w, []).append((l, False))
        for row in range(count):
            if crushed[row]:
                continue
            for other, otherWins in partners.get(row, []):
                if (other < count and crushed[other]) or other in haloGone:
                    continue
                if otherWins:
                    crushed[row] = True
                elif other < count:
                    crushed[other] = True
                else:
                    haloGone.add(other)
                break
        return crushed

    ###-----------------------------------------------------------------
    ###5. Shading
    ###-----------------------------------------------------------------

    def shade(self):
        count = len(self.forest)
        plants = numpy.nonzero(~self.forest.isSeed)[0]
        largest = self.comm.allreduce(float(self.forest.r[plants].max()) if len(plants) else 0.0, "max")
        sends = self.haloFor(plants, 2.0 * largest * 1.000001 + 0.000001)
        halo = self.swapHalo(sends, ["id", "x", "y", "r", "heightStem", "species"])
        haloCount = len(halo["id"])
        self.traffic["halo copies for shading"] += haloCount
        isPlant = numpy.concatenate((~self.forest.isSeed, numpy.ones(haloCount, dtype=bool)))
        return trees.shade(count,
                           numpy.concatenate((self.forest.x, halo["x"])),
                           numpy.concatenate((self.forest.y, halo["y"])),
                           numpy.concatenate((self.forest.r, halo["r"])),
                           isPlant,
                           numpy.concatenate((self.forest.heightStem, halo["heightStem"])),
                           numpy.concatenate((self.forest.id, halo["id"])),
                           numpy.concatenate((self.forest.species, halo["species"])),
                           self.cycle, self.table, self.world, self.settings.photonLimit, self.rngStart, self.random)

    ###-----------------------------------------------------------------
    ###Counting and checking
    ###-----------------------------------------------------------------

    def summary(self, deaths, born, rounds):
        seeds = int(numpy.sum(self.forest.isSeed))
        plants = len(self.forest) - seeds
        counts = numpy.array([plants, seeds, born] + [deaths[cause] for cause in trees.CAUSES], dtype=numpy.int64)
        totals = self.comm.allreduce(counts, "sum")
        result = {"cycle": self.cycle - 1, "plants": int(totals[0]), "seeds": int(totals[1]), "born": int(totals[2]),
                  "rounds": rounds, "deaths": {}}
        for place in range(len(trees.CAUSES)):
            result["deaths"][trees.CAUSES[place]] = int(totals[3 + place])
        return result

    def fingerprint(self):
        ###A 128-bit number summing up every tree and seed in the world:
        ###each row's values are scrambled into two 64-bit numbers, which are
        ###added up over all the rows (and all the ranks). Adding doesn't care
        ###about order, so it's the same however the rows are shared out, and
        ###any difference anywhere changes it.
        f = self.forest
        fields = [f.id, f.species, f.isSeed, f.x, f.y, f.birthCycle, f.age, f.countToGerm, f.massSeed,
                  f.massStem, f.massLeaf, f.massFixed, f.heightStem, f.r, f.areaCovered, f.isMature,
                  f.attachedCount, f.attachedMass]
        words = [numpy.zeros(len(f), dtype=numpy.uint32) for place in range(4)]
        for field in fields:
            if field.dtype == numpy.float64:
                bits = field.view(numpy.uint64)
            else:
                bits = field.astype(numpy.uint64)
            low = (bits & numpy.uint64(0xFFFFFFFF)).astype(numpy.uint32)
            high = (bits >> numpy.uint64(32)).astype(numpy.uint32)
            words = list(philox.philox4x32(words[0] ^ low, words[1] ^ high, words[2], words[3], 0x9E3779B9, 0x7F4A7C15))
        firstHalf = int(numpy.sum(words[0].astype(numpy.uint64) | (words[1].astype(numpy.uint64) << numpy.uint64(32)), dtype=numpy.uint64))
        secondHalf = int(numpy.sum(words[2].astype(numpy.uint64) | (words[3].astype(numpy.uint64) << numpy.uint64(32)), dtype=numpy.uint64))
        firstHalf = self.comm.allreduce(firstHalf, "sum") % (1 << 64)
        secondHalf = self.comm.allreduce(secondHalf, "sum") % (1 << 64)
        count = self.comm.allreduce(len(f), "sum")
        return "%d:%016x%016x" % (count, firstHalf, secondHalf)
