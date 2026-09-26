"""This file is part of Vida.
    --------------------------
    Copyright 2026, Sean T. Hammond

    Vida is experimental in nature and is made available as a research courtesy "AS IS," but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.

    You should have received a copy of academic software agreement along with Vida. If not, see <https://github.com/seanth/Vida/blob/master/LICENSE.txt>.
"""

###The trees and seeds of one part of the world, as a table.
###
###Vida keeps each plant as a Python object. Here each setting of a plant is
###one column (a numpy array) and each plant or seed is one row, so the same
###sum is done for millions of plants at once. A tree takes about 200 bytes
###this way, against several kilobytes as a Python object.
###
###The sums are Vida's own (vplantr.py and vworldr.py): germination, growth
###(the allometry from calcMassStemFromMassNew to calcRadiusLeafFromMassLeaf),
###making and dispersing seeds, the kinds of death, shading and
###photosynthesis. What's different, and why, is in worldscale/README.md.
###
###Every random number comes from an address (see philox.py), so nothing
###here depends on the order the rows are in.

import math

import numpy

from worldscale import philox

###how many cycles of growth a plant remembers (numYearsGrowthMemory) at most
MEMORY_SLOTS = 4

###the columns, and their types
COLUMNS = {
    "id": numpy.uint64,            #a number no other tree or seed ever has
    "species": numpy.int32,        #row in the SpeciesTable
    "isSeed": numpy.bool_,
    "x": numpy.float64,
    "y": numpy.float64,
    "birthCycle": numpy.int32,     #the cycle it was dispersed (or placed) in
    "age": numpy.int32,
    "countToGerm": numpy.int32,
    "massSeed": numpy.float64,
    "radiusSeed": numpy.float64,
    "massStem": numpy.float64,
    "massLeaf": numpy.float64,
    "massFixed": numpy.float64,
    "massTotal": numpy.float64,
    "radiusStem": numpy.float64,
    "radiusLeaf": numpy.float64,
    "r": numpy.float64,            #the larger of the canopy and stem radius
    "heightStem": numpy.float64,
    "isMature": numpy.bool_,
    "areaCovered": numpy.float64,
    "fixedCount": numpy.int32,     #how many of fixedRecord are filled in
    "heightCount": numpy.int32,
    "prevHeight": numpy.float64,
    "avgHeightGrowth": numpy.float64,
    "maxAvgHeightGrowth": numpy.float64,
    "attachedCount": numpy.int32,  #seeds growing on the plant
    "attachedMass": numpy.float64, #the mass of each of them
}
###columns with one value for each of the last MEMORY_SLOTS cycles
RECORD_COLUMNS = ["fixedRecord", "heightRecord"]

###causes of death, counted each cycle
CAUSES = ["failed to germinate(random death)", "failed to germinate(immaturity)",
          "impossible height calculation", "stem off world", "random death",
          "growth too slow", "violated Euler-Greenhill", "crushed", "lack of light",
          "seed landed off world"]


class Forest:
    ###A table of trees and seeds: one numpy array per column.

    def __init__(self, count=0):
        for name in COLUMNS:
            setattr(self, name, numpy.zeros(count, dtype=COLUMNS[name]))
        for name in RECORD_COLUMNS:
            setattr(self, name, numpy.zeros((count, MEMORY_SLOTS)))

    def __len__(self):
        return len(self.id)

    def columnNames(self):
        return list(COLUMNS) + RECORD_COLUMNS

    def take(self, rows):
        ###a new Forest with just these rows (a list of row numbers, or a
        ###True/False mask)
        chosen = Forest(0)
        for name in self.columnNames():
            setattr(chosen, name, getattr(self, name)[rows])
        return chosen

    def asDict(self):
        ###the columns, to send to another processor
        columns = {}
        for name in self.columnNames():
            columns[name] = getattr(self, name)
        return columns


def forestFromDict(columns):
    theForest = Forest(0)
    for name in theForest.columnNames():
        setattr(theForest, name, columns[name])
    return theForest


def joinForests(forests):
    ###one Forest with all the rows of these, in order
    joined = Forest(0)
    for name in joined.columnNames():
        parts = []
        for aForest in forests:
            parts.append(getattr(aForest, name))
        if parts:
            setattr(joined, name, numpy.concatenate(parts))
    return joined


###---------------------------------------------------------------------
###Sizes from masses (Vida's allometry)
###---------------------------------------------------------------------

def seedRadius(massSeed, densitySeed):
    ###growSeedOnPlant: the radius of a ball of the seed's volume
    volume = massSeed / densitySeed
    return (volume / 1.3333 / 3.14) ** 0.3333


def stemRadius(massStem, species, table):
    ###calcRadiusStemFromMassStem
    diameter = table.speciesConstant20[species] * (massStem ** table.speciesExponent20[species])
    return diameter / 2.0


def stemHeight(radiusStem, isMature, species, table):
    ###calcHeightStemFromRadiusStem: the height for a stem this thick, and
    ###whether the plant is now mature (the young formula has caught up with
    ###the grown one)
    diameter = radiusStem * 2.0
    with numpy.errstate(divide="ignore", invalid="ignore"):
        grown = table.speciesConstant8[species] * numpy.log(diameter) + table.heightStemMax[species]
    young = table.speciesConstant7[species] * (diameter ** table.speciesExponent7[species]) - table.speciesConstant6[species]
    nowMature = isMature | (grown >= young)
    height = numpy.where(nowMature, grown, young)
    return height, nowMature


def leafMass(massStem, age, isMature, species, table):
    ###calcMassLeafFromMassStem
    older = (age >= table.startMakingSeedsAge[species]) | isMature
    return numpy.where(older,
                       table.speciesConstant3[species] * (massStem ** table.speciesExponent3[species]),
                       table.speciesConstant2[species] * (massStem ** table.speciesExponent2[species]))


def leafRadius(massLeaf, species, table):
    ###calcRadiusLeafFromMassLeaf: leaves as a disc heightLeafMax thick
    volume = massLeaf / table.densityLeaf[species]
    area = volume / table.heightLeafMax[species]
    radius = numpy.sqrt(area / 3.14)
    return numpy.where(table.leafIsHemisphere[species], radius * 0.7071067812, radius)


def recentAverage(record, count, memory):
    ###the average of the last min(count, memory) values in a record (the
    ###newest value is in the last column)
    slots = record.shape[1]
    used = numpy.minimum(count, memory)
    total = numpy.zeros(len(count))
    for slot in range(slots):
        ###slot slots-1 is the newest, slots-2 the one before, and so on
        age = slots - 1 - slot
        total = total + numpy.where(age < used, record[:, slot], 0.0)
    with numpy.errstate(invalid="ignore", divide="ignore"):
        return numpy.where(used > 0, total / numpy.maximum(used, 1), 0.0)


def pushRecord(record, count, values, rows):
    ###add this cycle's value to the end of each row's record
    record[rows, :-1] = record[rows, 1:]
    record[rows, -1] = values
    count[rows] = numpy.minimum(count[rows] + 1, MEMORY_SLOTS)


###---------------------------------------------------------------------
###Starting seeds
###---------------------------------------------------------------------

def startingSeeds(tileNumbers, tileCorners, tileSize, seedsPerTile, worldSize, table, rngStart, random):
    ###Seeds scattered at random over these tiles, each tile's seeds worked
    ###out from its own addresses (so a tile's seeds are the same whichever
    ###processor makes them). Like Vida's "random" seed placement, with a
    ###random species for each seed.
    count = len(tileNumbers) * seedsPerTile
    seeds = Forest(count)
    if count == 0:
        return seeds
    tile = numpy.repeat(numpy.asarray(tileNumbers, dtype=numpy.uint64), seedsPerTile)
    number = numpy.tile(numpy.arange(seedsPerTile), len(tileNumbers))
    corners = numpy.repeat(numpy.asarray(tileCorners, dtype=numpy.float64).reshape(-1, 2), seedsPerTile, axis=0)
    blocks = random.randomBlocks(rngStart, tile, 0, philox.PLACE_START, number)
    seeds.x = corners[:, 0] + blocks[:, 0] * tileSize
    seeds.y = corners[:, 1] + blocks[:, 1] * tileSize
    seeds.species = numpy.minimum((blocks[:, 2] * table.count).astype(numpy.int32), table.count - 1)
    seeds.id = (tile << numpy.uint64(40)) | number.astype(numpy.uint64)
    seeds.isSeed[:] = True
    seeds.massSeed = table.massSeedMax[seeds.species].copy()
    seeds.radiusSeed = seedRadius(seeds.massSeed, table.densitySeed[seeds.species])
    seeds.r = seeds.radiusSeed.copy()
    seeds.massTotal = seeds.massSeed.copy()
    ###a tile only keeps seeds inside the world (tiles at the edge can stick out)
    half = worldSize / 2.0
    inside = (seeds.x >= -half) & (seeds.x < half) & (seeds.y >= -half) & (seeds.y < half)
    return seeds.take(inside)


###---------------------------------------------------------------------
###Germinating and growing (each row only needs its own values)
###---------------------------------------------------------------------

def germinate(forest, cycle, table, world, rngStart, random, deaths):
    ###Vida's germinate(), for every seed at once. Gives back the rows that die.
    seeds = numpy.nonzero(forest.isSeed)[0]
    dying = numpy.zeros(len(forest), dtype=bool)
    if len(seeds) == 0:
        return dying
    species = forest.species[seeds]
    ready = forest.countToGerm[seeds] < 1
    tooBad = random.randomBlocks(rngStart, forest.id[seeds], cycle, philox.GERMINATE, 0)[:, 0]
    if cycle == 0 and world.ignoreGermDeathAtStart:
        tooBad[:] = 1.0
    failed = ready & (tooBad < table.fractionFailGerminate[species])
    massForGrowth = forest.massSeed[seeds] * table.fractionSeedMassToPlant[species]
    tooSmall = (massForGrowth <= table.massSeedMax[species] * table.fractMassSeedMaxToGerm[species] * table.fractionSeedMassToPlant[species]) | (massForGrowth <= 0.0)
    immature = ready & ~failed & tooSmall
    grows = ready & ~failed & ~tooSmall
    deaths[CAUSES[0]] += int(failed.sum())
    deaths[CAUSES[1]] += int(immature.sum())
    dying[seeds[failed | immature]] = True
    ###seeds not ready yet count down
    waiting = seeds[~ready]
    forest.countToGerm[waiting] = forest.countToGerm[waiting] - 1
    ###the rest become plants
    rows = seeds[grows]
    species = forest.species[rows]
    mass = massForGrowth[grows]
    forest.isSeed[rows] = False
    forest.age[rows] = 1
    forest.massStem[rows] = mass * table.fractionCarbonToStem[species]
    forest.massLeaf[rows] = mass - forest.massStem[rows]
    forest.radiusStem[rows] = stemRadius(forest.massStem[rows], species, table)
    height, mature = stemHeight(forest.radiusStem[rows], forest.isMature[rows], species, table)
    forest.heightStem[rows] = height
    forest.isMature[rows] = mature
    forest.radiusLeaf[rows] = leafRadius(forest.massLeaf[rows], species, table)
    forest.r[rows] = numpy.maximum(forest.radiusLeaf[rows], forest.radiusStem[rows])
    forest.massTotal[rows] = forest.massStem[rows] + forest.massLeaf[rows]
    ###Vida's zeroSeedValues sets massFixed to -0.0; it's filled in by
    ###photosynthesis at the end of this cycle
    forest.massFixed[rows] = 0.0
    impossible = height < 0.0
    deaths[CAUSES[2]] += int(impossible.sum())
    dying[rows[impossible]] = True
    return dying


def grow(forest, plants, table, world, deaths):
    ###Vida's growPlant() for these rows (the plants; seeds that germinated
    ###this cycle wait until the next): feed the seeds growing on the plant,
    ###start new ones, then grow the stem and leaves.
    ###Gives back (rows that die, dispersing plants' rows, how many seeds each
    ###disperses, the mass of each of those seeds).
    dying = numpy.zeros(len(forest), dtype=bool)
    if len(plants) == 0:
        return dying, plants, plants, numpy.zeros(0)
    species = forest.species[plants]
    makes = table.makeSeeds[species]
    fixed = forest.massFixed[plants].copy()

    ###1. seeds on the plant get carbon. (Vida gives each seed its share plus
    ###or minus a random amount; here the seeds on a plant grow together, each
    ###with its share, as one cohort.)
    count = forest.attachedCount[plants]
    hasSeeds = makes & (count > 0)
    massSeedMax = table.massSeedMax[species]
    with numpy.errstate(divide="ignore", invalid="ignore"):
        share = numpy.where(hasSeeds, fixed * table.fractionCarbonToSeeds[species] / numpy.maximum(count, 1), 0.0)
    share = numpy.minimum(share, massSeedMax)
    share = numpy.where(fixed > 0.0, share, 0.0)
    total = share * count
    tooMuch = total > fixed
    share = numpy.where(tooMuch & (count > 0), fixed / numpy.maximum(count, 1), share)
    fixed = fixed - share * count
    attachedMass = forest.attachedMass[plants] + share
    full = hasSeeds & (attachedMass >= massSeedMax)
    dispersing = plants[full]
    dispersingCount = count[full]
    dispersingMass = attachedMass[full]
    count = numpy.where(full, 0, count)
    attachedMass = numpy.where(full, 0.0, attachedMass)

    ###2. mature plants start new seeds (makeSomeSeeds). Here only when none
    ###are growing on the plant already, so they stay one cohort.
    record = forest.fixedRecord[plants]
    averageFixed = recentAverage(record, forest.fixedCount[plants], table.memory[species])
    maxKgSeeds = table.reproductionConstant[species] * (numpy.maximum(fixed, 0.0) ** table.reproductionExponent[species])
    adjusted = maxKgSeeds * table.fractionCarbonToSeeds[species]
    with numpy.errstate(divide="ignore", invalid="ignore"):
        stressed = (fixed < averageFixed) & (fixed / numpy.where(averageFixed > 0, averageFixed, 1.0) < table.fractionSelfishness[species]) & (table.fractionCarbonToSeeds[species] < 1.0)
    adjusted = numpy.where(stressed, maxKgSeeds, adjusted)
    newSeeds = (adjusted / massSeedMax).astype(numpy.int64) - count
    newSeeds = numpy.minimum(newSeeds, world.maxSeedsPerPlant)
    starts = makes & forest.isMature[plants] & (count == 0) & (newSeeds > 0)
    count = numpy.where(starts, newSeeds, count)
    attachedMass = numpy.where(starts, 0.0, attachedMass)
    forest.attachedCount[plants] = count
    forest.attachedMass[plants] = attachedMass
    forest.massFixed[plants] = fixed

    ###3. the stem, then the leaves (calcMassStemFromMassNew onwards)
    massStem = forest.massStem[plants] + table.speciesConstant1[species] * (numpy.maximum(fixed, 0.0) ** table.speciesExponent1[species])
    radiusStem = stemRadius(massStem, species, table)
    height, mature = stemHeight(radiusStem, forest.isMature[plants], species, table)
    massLeaf = leafMass(massStem, forest.age[plants], mature, species, table)
    radiusLeaf = leafRadius(massLeaf, species, table)
    forest.massStem[plants] = massStem
    forest.radiusStem[plants] = radiusStem
    forest.heightStem[plants] = height
    forest.isMature[plants] = mature
    forest.massLeaf[plants] = massLeaf
    forest.radiusLeaf[plants] = radiusLeaf
    forest.massTotal[plants] = massLeaf + massStem + attachedMass * count
    forest.r[plants] = numpy.maximum(radiusLeaf, radiusStem)
    impossible = (radiusStem <= 0.0) | ~(height >= 0.0)
    deaths[CAUSES[2]] += int(impossible.sum())
    dying[plants[impossible]] = True

    ###4. remember how much the stem grew
    pushRecord(forest.heightRecord, forest.heightCount, height - forest.prevHeight[plants], plants)
    forest.prevHeight[plants] = height
    average = recentAverage(forest.heightRecord[plants], forest.heightCount[plants], table.memory[species])
    forest.avgHeightGrowth[plants] = average
    forest.maxAvgHeightGrowth[plants] = numpy.maximum(forest.maxAvgHeightGrowth[plants], average)
    forest.age[plants] = forest.age[plants] + 1
    return dying, dispersing, dispersingCount, dispersingMass


def disperse(forest, mothers, counts, masses, cycle, table, world, rngStart, random):
    ###Where each dispersing seed lands (Vida's makeSeed for where on the
    ###canopy it formed, then disperseSeed). Gives back a Forest of the new
    ###seeds, without ids yet, and the mothers' ids and seed numbers (to
    ###give them ids).
    total = int(numpy.sum(counts))
    seeds = Forest(total)
    if total == 0:
        return seeds, numpy.zeros(0, dtype=numpy.uint64), numpy.zeros(0, dtype=numpy.int64)
    mother = numpy.repeat(mothers, counts)
    firstOfEach = numpy.cumsum(counts) - counts
    seedNumber = numpy.arange(total) - numpy.repeat(firstOfEach, counts)
    species = forest.species[mother]
    motherId = forest.id[mother]
    motherX = forest.x[mother]
    motherY = forest.y[mother]

    ###where on the canopy it formed (makeSeed)
    form = random.randomBlocks(rngStart, motherId, cycle, philox.FORM_SEED, seedNumber)
    canopy = forest.radiusLeaf[mother]
    outer = canopy * table.formationMax[species]
    inner = canopy * table.formationMin[species]
    distance = form[:, 0] * (outer - inner) + inner
    angle = form[:, 1] * (3.14 * 2)
    formedX = distance * numpy.cos(angle) + motherX
    formedY = distance * numpy.sin(angle) + motherY

    ###how it's dispersed (disperseSeed)
    throw = random.randomBlocks(rngStart, motherId, cycle, philox.DISPERSE, seedNumber)
    method = table.dispersalMethod[species]
    newX = formedX.copy()
    newY = formedY.copy()
    ###0: anywhere in the world
    anywhere = method == 0
    newX[anywhere] = (throw[anywhere, 0] - 0.5) * world.worldSize
    newY[anywhere] = (throw[anywhere, 1] - 0.5) * world.worldSize
    ###1: straight down (newX, newY stay where it formed)
    ###2: in a circle round where it formed
    circle = method == 2
    reach = throw[circle, 0] * table.dispersal1[species[circle]]
    turn = throw[circle, 1] * (3.14 * 2)
    newX[circle] = formedX[circle] + reach * numpy.cos(turn)
    newY[circle] = formedY[circle] + reach * numpy.sin(turn)
    ###3 and 4: thrown outwards from the middle of the plant
    thrown = (method == 3) | (method == 4)
    if numpy.any(thrown):
        how = throw[thrown]
        kinds = method[thrown]
        s = species[thrown]
        ###3: up to a set distance
        throwDistance = how[:, 0] * table.dispersal1[s]
        ###4: ballistic, from the launch angle and speed, each varied by up to half
        launch = table.dispersal1[s]
        change = how[:, 0] * (launch * 0.5)
        change = numpy.where(how[:, 1] > 0.5, -change, change)
        launch = numpy.radians(launch + change)
        speed = table.dispersal2[s]
        change = how[:, 2] * (speed * 0.5)
        change = numpy.where(how[:, 3] > 0.5, -change, change)
        speed = speed + change
        height = forest.heightStem[mother[thrown]] + table.heightLeafMax[s]
        g = world.gravity
        across = (speed * numpy.cos(launch)) / g
        up = speed * numpy.sin(launch)
        ballistic = across * (up + (up * up + 2.0 * g * height) ** 0.5)
        throwDistance = numpy.where(kinds == 4, ballistic, throwDistance)
        run = formedX[thrown] - motherX[thrown]
        rise = formedY[thrown] - motherY[thrown]
        hypot = numpy.hypot(run, rise)
        with numpy.errstate(invalid="ignore", divide="ignore"):
            direction = numpy.where(hypot > 0.0, numpy.arcsin(numpy.clip(rise / numpy.where(hypot > 0.0, hypot, 1.0), -1.0, 1.0)), 0.0)
        alongX = numpy.cos(direction) * throwDistance
        alongX = numpy.where(run < 0.0, -alongX, alongX)
        newX[thrown] = alongX + formedX[thrown]
        newY[thrown] = numpy.sin(direction) * throwDistance + formedY[thrown]

    seeds.species = species.astype(numpy.int32)
    seeds.isSeed[:] = True
    seeds.x = newX
    seeds.y = newY
    seeds.birthCycle[:] = cycle
    seeds.countToGerm = table.delayInGermination[species].astype(numpy.int32)
    seeds.massSeed = numpy.repeat(masses, counts)
    seeds.radiusSeed = seedRadius(seeds.massSeed, table.densitySeed[species])
    seeds.r = seeds.radiusSeed.copy()
    seeds.massTotal = seeds.massSeed.copy()
    return seeds, motherId, seedNumber


###---------------------------------------------------------------------
###Deaths that each row decides for itself
###---------------------------------------------------------------------

def ownDeaths(forest, cycle, table, world, rngStart, random, deaths):
    ###Stems off the world, random death, growing too slowly and buckling
    ###(Euler-Greenhill), in the order Vida checks them. Gives back the rows
    ###that die.
    dying = numpy.zeros(len(forest), dtype=bool)
    if len(forest) == 0:
        return dying
    plant = ~forest.isSeed
    species = forest.species
    if not world.allowOffWorld:
        radius = numpy.where(plant, forest.radiusStem, forest.radiusSeed)
        half = world.worldSize / 2.0
        off = (forest.x + radius > half) | (forest.x - radius < -half) | (forest.y + radius > half) | (forest.y - radius < -half)
        deaths[CAUSES[3]] += int(off.sum())
        dying |= off
    if world.allowRandomDeath:
        tooBad = random.randomBlocks(rngStart, forest.id, cycle, philox.RANDOM_DEATH, 0)[:, 0]
        chance = numpy.where(plant, world.randomDeathPlant, world.randomDeathSeed)
        unlucky = ~dying & (tooBad < chance)
        deaths[CAUSES[4]] += int(unlucky.sum())
        dying |= unlucky
    if world.allowSlowGrowthDeath:
        with numpy.errstate(invalid="ignore", divide="ignore"):
            fraction = forest.avgHeightGrowth / numpy.where(forest.maxAvgHeightGrowth > 0.0, forest.maxAvgHeightGrowth, 1.0)
        slow = plant & (forest.maxAvgHeightGrowth > 0.0) & ((table.randomSlowGrowth[species] > fraction) | (world.randomSlowGrowth > fraction))
        tooBad = random.randomBlocks(rngStart, forest.id, cycle, philox.SLOW_GROWTH, 0)[:, 0]
        tooSlow = ~dying & slow & (tooBad <= world.randomDeathPlant)
        deaths[CAUSES[5]] += int(tooSlow.sum())
        dying |= tooSlow
    if not world.allowEulerGreenhillViolations:
        youngs = table.youngsModulusStem[species] * 1000000000
        critical = 0.79 * ((youngs / (world.gravity * table.densityStem[species])) ** 0.3333) * ((forest.radiusStem * 2.0) ** 0.6667)
        buckled = ~dying & plant & (forest.heightStem >= critical)
        deaths[CAUSES[6]] += int(buckled.sum())
        dying |= buckled
    return dying


###---------------------------------------------------------------------
###Finding neighbours
###---------------------------------------------------------------------

def findPairs(x, y, radius):
    ###Every pair of circles that overlap (or touch), each pair once, as two
    ###arrays of row numbers.
    ###
    ###Circles come in all sizes, from seeds a few centimetres across to
    ###canopies ten metres across, so each pair is found by the larger circle
    ###of the two. Circles are put in size classes (each class twice the
    ###radius of the one before), and each class looks for overlaps among
    ###circles no bigger than itself, on a grid with squares twice its largest
    ###radius: then everything that can overlap it is in the 3 x 3 squares
    ###around it.
    count = len(x)
    if count < 2:
        return numpy.zeros(0, dtype=numpy.int64), numpy.zeros(0, dtype=numpy.int64)
    smallest = 0.001
    sizeClass = numpy.ceil(numpy.log2(numpy.maximum(radius, smallest) / smallest)).astype(numpy.int64)
    firstParts = []
    secondParts = []
    for theClass in numpy.unique(sizeClass):
        queries = numpy.nonzero(sizeClass == theClass)[0]
        targets = numpy.nonzero(sizeClass <= theClass)[0]
        square = max(2.0 * float(radius[queries].max()), smallest)
        column = numpy.floor(x[targets] / square).astype(numpy.int64)
        row = numpy.floor(y[targets] / square).astype(numpy.int64)
        lowestColumn = column.min() - 2
        lowestRow = row.min() - 2
        rows = int(row.max() - lowestRow + 3)
        key = (column - lowestColumn) * rows + (row - lowestRow)
        byKey = numpy.argsort(key, kind="stable")
        sortedKeys = key[byKey]
        queryColumn = numpy.floor(x[queries] / square).astype(numpy.int64)
        queryRow = numpy.floor(y[queries] / square).astype(numpy.int64)
        ###look them up in square order: searchsorted is much quicker when
        ###what it's looking for comes in order
        inOrder = numpy.argsort((queryColumn - lowestColumn) * rows + (queryRow - lowestRow), kind="stable")
        queries = queries[inOrder]
        queryColumn = queryColumn[inOrder]
        queryRow = queryRow[inOrder]
        for across in (-1, 0, 1):
            for up in (-1, 0, 1):
                wanted = (queryColumn + across - lowestColumn) * rows + (queryRow + up - lowestRow)
                start = numpy.searchsorted(sortedKeys, wanted, side="left")
                end = numpy.searchsorted(sortedKeys, wanted, side="right")
                counts = end - start
                total = int(counts.sum())
                if total == 0:
                    continue
                first = numpy.repeat(queries, counts)
                offsets = numpy.arange(total) - numpy.repeat(numpy.cumsum(counts) - counts, counts)
                second = targets[byKey[numpy.repeat(start, counts) + offsets]]
                ###each pair once: within the same class, only the lower row number looks
                keep = (first != second) & ((sizeClass[second] < theClass) | (second > first))
                first = first[keep]
                second = second[keep]
                dx = x[first] - x[second]
                dy = y[first] - y[second]
                reach = radius[first] + radius[second]
                close = dx * dx + dy * dy <= reach * reach
                firstParts.append(first[close])
                secondParts.append(second[close])
    if not firstParts:
        return numpy.zeros(0, dtype=numpy.int64), numpy.zeros(0, dtype=numpy.int64)
    return numpy.concatenate(firstParts), numpy.concatenate(secondParts)


def strongerFirst(massTotal, birthCycle, ids):
    ###A number for each row: lower means it wins an overlap. Vida's
    ###removeOverlaps: the heavier wins; if they weigh the same, the one
    ###planted first. Here the tree id settles anything left, so every pair
    ###has a winner, the same one on every processor.
    order = numpy.lexsort((ids, birthCycle, -massTotal))
    place = numpy.empty(len(order), dtype=numpy.int64)
    place[order] = numpy.arange(len(order))
    return place


def tallerFirst(heightStem, ids):
    ###A number for each row: lower means taller, for shading. (Vida sorts by
    ###height; the id settles ties.)
    order = numpy.lexsort((ids, -heightStem))
    place = numpy.empty(len(order), dtype=numpy.int64)
    place[order] = numpy.arange(len(order))
    return place


###---------------------------------------------------------------------
###Shading and photosynthesis
###---------------------------------------------------------------------

def lensArea(x, y, r, xx, yy, rr):
    ###geometry_utils.areaOverlappingCircles, for arrays
    distance = numpy.hypot(x - xx, y - yy)
    area = numpy.zeros(len(x))
    inside = distance < numpy.abs(r - rr)
    area[inside] = 3.14 * r[inside] * r[inside]
    partly = ~inside & (distance <= r + rr) & (distance > 0.0)
    d = distance[partly]
    r0 = r[partly]
    r1 = rr[partly]
    d0 = (r0 * r0 - r1 * r1 + d * d) / (2 * d)
    d1 = (r1 * r1 - r0 * r0 + d * d) / (2 * d)
    halfLine = numpy.sqrt(numpy.maximum(r0 * r0 - d0 * d0, 0.0))
    angle0 = numpy.arccos(numpy.clip(d0 / r0, -1.0, 1.0)) * 2.0
    angle1 = numpy.arccos(numpy.clip(d1 / r1, -1.0, 1.0)) * 2.0
    sector0 = 0.5 * r0 * r0 * angle0
    sector1 = 0.5 * r1 * r1 * angle1
    triangle0 = 2.0 * 0.5 * d0 * halfLine
    triangle1 = 2.0 * 0.5 * d1 * halfLine
    area[partly] = (sector0 - triangle0) + (sector1 - triangle1)
    return area


def shade(ownedCount, x, y, r, isPlant, heightStem, ids, species, cycle, table, world, photonLimit, rngStart, random):
    ###Vida's classic shading (determineShade), for the plants in the first
    ###ownedCount places of these arrays. The rest are the neighbours'
    ###plants near enough to shade them. Gives back each one's areaCovered.
    ###
    ###A plant is shaded by the taller canopies it overlaps. No canopies: all
    ###its area gets light. One: the overlapping area, less what gets through.
    ###Two or more: photons are dropped at random on the plant, and each
    ###stops at the first (tallest) canopy it lands in, getting through that
    ###one with the canopy's transmittance. Each photon's random numbers have
    ###their own address (the plant, the cycle and the photon's number).
    plants = numpy.nonzero(isPlant)[0]
    first, second = findPairs(x[plants], y[plants], r[plants])
    first = plants[first]
    second = plants[second]
    ###who covers whom: the taller one covers the other
    place = tallerFirst(heightStem, ids)
    firstTaller = place[first] < place[second]
    shaded = numpy.where(firstTaller, second, first)
    cover = numpy.where(firstTaller, first, second)
    mine = shaded < ownedCount
    shaded = shaded[mine]
    cover = cover[mine]
    ###each plant's covers, tallest first
    order = numpy.lexsort((place[cover], shaded))
    shaded = shaded[order]
    cover = cover[order]
    coverCount = numpy.bincount(shaded, minlength=ownedCount)
    firstCover = numpy.cumsum(coverCount) - coverCount
    radius = r[:ownedCount]
    areaTotal = 3.14 * radius * radius
    light = world.lightIntensity
    exposed = numpy.full(ownedCount, light)

    ###one cover: the overlapping area, worked out exactly
    one = numpy.nonzero(isPlant[:ownedCount] & (coverCount == 1))[0]
    if len(one):
        other = cover[firstCover[one]]
        area = lensArea(x[one], y[one], radius[one], x[other], y[other], r[other])
        area = area - area * table.canopyTransmittance[species[other]]
        with numpy.errstate(invalid="ignore", divide="ignore"):
            exposed[one] = numpy.where(areaTotal[one] > 0.0, (areaTotal[one] - area) / numpy.where(areaTotal[one] > 0.0, areaTotal[one], 1.0), 1.0) * light

    ###two or more: photons
    many = numpy.nonzero(isPlant[:ownedCount] & (coverCount >= 2))[0]
    if len(many):
        photons = numpy.maximum(areaTotal[many].astype(numpy.int64), 1) * 100
        photons = numpy.minimum(photons, photonLimit)
        hits = countPhotons(many, photons, x, y, r, ids, species, cover, firstCover, coverCount, cycle, table, rngStart, random)
        exposed[many] = hits / photons * light
    return areaTotal - areaTotal * exposed


def countPhotons(plants, photons, x, y, r, ids, species, cover, firstCover, coverCount, cycle, table, rngStart, random):
    ###How many of each plant's photons get through, a batch of plants at a
    ###time so the arrays stay a manageable size.
    hits = numpy.zeros(len(plants))
    batchLimit = 2000000
    runningTotal = numpy.cumsum(photons)
    ###the plants where each batch starts
    cuts = numpy.searchsorted(runningTotal, numpy.arange(batchLimit, int(runningTotal[-1]), batchLimit), side="left")
    starts = numpy.unique(numpy.concatenate(([0], cuts + 1)))
    starts = starts[starts < len(plants)]
    ends = numpy.append(starts[1:], len(plants))
    for start, end in zip(starts.tolist(), ends.tolist()):
        batch = numpy.arange(start, end)
        perPlant = photons[batch]
        total = int(perPlant.sum())
        owner = numpy.repeat(batch, perPlant)
        number = numpy.arange(total) - numpy.repeat(numpy.cumsum(perPlant) - perPlant, perPlant)
        row = plants[owner]
        blocks = random.randomBlocks(rngStart, ids[row], cycle, philox.PHOTON, number)
        ###a point spread evenly over the plant's circle
        distance = r[row] * numpy.sqrt(blocks[:, 0])
        angle = blocks[:, 1] * (2.0 * math.pi)
        photonX = x[row] + distance * numpy.cos(angle)
        photonY = y[row] + distance * numpy.sin(angle)
        getsThrough = blocks[:, 2]
        ###go down each photon's covers, tallest first, until one catches it
        blocked = numpy.zeros(total, dtype=bool)
        flying = numpy.ones(total, dtype=bool)
        which = 0
        while True:
            still = numpy.nonzero(flying & (which < coverCount[row]))[0]
            if len(still) == 0:
                break
            canopy = cover[firstCover[row[still]] + which]
            inside = numpy.hypot(x[canopy] - photonX[still], y[canopy] - photonY[still]) <= r[canopy]
            landed = still[inside]
            blocked[landed] = getsThrough[landed] > table.canopyTransmittance[species[canopy[inside]]]
            flying[landed] = False
            which = which + 1
        hits[batch] = numpy.bincount(owner - start, weights=(~blocked).astype(numpy.float64), minlength=len(batch))
    return hits


def photosynthesise(forest, table, deaths):
    ###calcNewMassFromLeaf for every plant. Gives back the rows that die for
    ###lack of light.
    plants = numpy.nonzero(~forest.isSeed)[0]
    dying = numpy.zeros(len(forest), dtype=bool)
    if len(plants) == 0:
        return dying
    species = forest.species[plants]
    areaPhotosynthesis = 3.14 * forest.radiusLeaf[plants] * forest.radiusLeaf[plants]
    available = areaPhotosynthesis - forest.areaCovered[plants]
    with numpy.errstate(invalid="ignore", divide="ignore"):
        fraction = numpy.where(areaPhotosynthesis > 0.0, available / numpy.where(areaPhotosynthesis > 0.0, areaPhotosynthesis, 1.0), 0.0)
        perLeaf = forest.massLeaf[plants] ** table.photoExponent[species]
    enough = fraction > table.fractionMinimumSurvival[species]
    rate = table.photoConstant[species] * fraction + table.photoConstantShade[species] * (1.0 - fraction)
    newMass = rate * available * perLeaf
    lacking = plants[~enough]
    deaths[CAUSES[8]] += len(lacking)
    dying[lacking] = True
    growing = plants[enough]
    forest.massFixed[growing] = newMass[enough]
    pushRecord(forest.fixedRecord, forest.fixedCount, newMass[enough], growing)
    return dying
