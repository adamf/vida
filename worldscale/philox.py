"""This file is part of Vida.
    --------------------------
    Copyright 2026, Sean T. Hammond

    Vida is experimental in nature and is made available as a research courtesy "AS IS," but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.

    You should have received a copy of academic software agreement along with Vida. If not, see <https://github.com/seanth/Vida/blob/master/LICENSE.txt>.
"""

###Random numbers that don't come in a queue.
###
###Vida today takes its random numbers from Python's `random`: one long
###stream, handed out one after another. Which number a plant gets depends
###on how many were handed out before it, so the plants have to be dealt with
###one at a time, always in the same order. That is what stops one
###simulation being split across processors.
###
###Here every random number has an address instead: which run (rngStart),
###which tree (its id), which cycle, what it's for (purpose) and which one
###(index). The number is worked out from the address alone, like looking a
###word up in a book by page and line. So a tree's numbers are the same
###whichever processor deals with it, in whatever order, and however the
###world is split up.
###
###The function that turns an address into random bits is Philox4x32-10
###(Salmon et al. 2011, "Parallel random numbers: as easy as 1, 2, 3"), the
###generator in the Random123 library that GPU and molecular dynamics codes
###use. It scrambles a 128-bit counter with a 64-bit key in ten rounds of
###multiplying and mixing, and passes the BigCrush tests of randomness. Each
###address gives four 32-bit numbers (a "block").
###
###An address is packed into Philox's counter and key like this:
###    key     = rngStart (64 bits)
###    counter = tree id (64 bits), cycle (32 bits), purpose (8 bits) and
###              index (24 bits, so up to 16,777,216 per purpose)

import numpy

###Philox4x32's constants (from Random123)
MULTIPLIER_0 = numpy.uint64(0xD2511F53)
MULTIPLIER_1 = numpy.uint64(0xCD9E8D57)
KEY_STEP_0 = numpy.uint32(0x9E3779B9)
KEY_STEP_1 = numpy.uint32(0xBB67AE85)
LOW_32_BITS = numpy.uint64(0xFFFFFFFF)
SHIFT_32 = numpy.uint64(32)
ROUNDS = 10

###What each random number is for. Two different uses of randomness never
###share an address, so they can't accidentally get the same numbers.
PLACE_START = 1    #where a tile's starting seeds land (id = the tile's number)
GERMINATE = 2      #does a seed fail to germinate?
FORM_SEED = 3      #where on its mother's canopy a seed forms
DISPERSE = 4       #how far and which way a seed is thrown
RANDOM_DEATH = 5   #random death
SLOW_GROWTH = 6    #death from growing too slowly
PHOTON = 7         #one photon of the shading Monte Carlo
SPECIES = 8        #which species a starting seed is

INDEX_LIMIT = 1 << 24


def philox4x32(counter0, counter1, counter2, counter3, key0, key1):
    ###Philox4x32-10 on arrays: each position is one counter, all with the
    ###same key. Gives back the four 32-bit words of each block.
    c0 = numpy.asarray(counter0, dtype=numpy.uint32)
    c1 = numpy.asarray(counter1, dtype=numpy.uint32)
    c2 = numpy.asarray(counter2, dtype=numpy.uint32)
    c3 = numpy.asarray(counter3, dtype=numpy.uint32)
    k0 = numpy.uint32(key0)
    k1 = numpy.uint32(key1)
    for roundNumber in range(ROUNDS):
        if roundNumber > 0:
            ###the key moves on between rounds (wrapping round at 2**32)
            k0 = numpy.uint32((int(k0) + int(KEY_STEP_0)) & 0xFFFFFFFF)
            k1 = numpy.uint32((int(k1) + int(KEY_STEP_1)) & 0xFFFFFFFF)
        ###two 32 x 32 -> 64 bit multiplications, split into high and low halves
        product0 = c0.astype(numpy.uint64) * MULTIPLIER_0
        product1 = c2.astype(numpy.uint64) * MULTIPLIER_1
        high0 = (product0 >> SHIFT_32).astype(numpy.uint32)
        low0 = (product0 & LOW_32_BITS).astype(numpy.uint32)
        high1 = (product1 >> SHIFT_32).astype(numpy.uint32)
        low1 = (product1 & LOW_32_BITS).astype(numpy.uint32)
        c0, c1, c2, c3 = high1 ^ c1 ^ k0, low1, high0 ^ c3 ^ k1, low0
    return c0, c1, c2, c3


def randomBlocks(rngStart, ids, cycle, purpose, index):
    ###Four random numbers between 0 and 1 (never exactly 0 or 1) for each
    ###address. ids and index can be arrays (of the same length, or one of
    ###them a single number); cycle and purpose are single numbers.
    ###Gives back an array with one row of four numbers per address.
    ids = numpy.asarray(ids, dtype=numpy.uint64)
    index = numpy.asarray(index, dtype=numpy.int64)
    if index.size and (index.min() < 0 or index.max() >= INDEX_LIMIT):
        raise ValueError("a random number's index must be from 0 to %d" % (INDEX_LIMIT - 1))
    ids, index = numpy.broadcast_arrays(ids, index)
    idLow = (ids & LOW_32_BITS).astype(numpy.uint32)
    idHigh = (ids >> SHIFT_32).astype(numpy.uint32)
    cycleWord = numpy.full(ids.shape, cycle & 0xFFFFFFFF, dtype=numpy.uint32)
    lastWord = ((purpose << 24) | index).astype(numpy.uint32)
    words = philox4x32(idLow, idHigh, cycleWord, lastWord,
                       rngStart & 0xFFFFFFFF, (rngStart >> 32) & 0xFFFFFFFF)
    blocks = numpy.empty(ids.shape + (4,))
    for column in range(4):
        ###(w + 0.5) / 2**32: evenly spread over (0, 1), with no 0 or 1
        blocks[..., column] = (words[column].astype(numpy.float64) + 0.5) * (1.0 / 4294967296.0)
    return blocks


class QueueRandom:
    ###The old way, for comparison: numbers handed out one after another
    ###from a single stream, whatever address is asked for. Results then
    ###depend on the order things are dealt with (see the prototype's
    ###-rng queue option).
    def __init__(self, rngStart):
        self.generator = numpy.random.Generator(numpy.random.MT19937(rngStart))

    def randomBlocks(self, rngStart, ids, cycle, purpose, index):
        ids = numpy.asarray(ids, dtype=numpy.uint64)
        index = numpy.asarray(index, dtype=numpy.int64)
        ids, index = numpy.broadcast_arrays(ids, index)
        return self.generator.random(ids.shape + (4,))


class AddressedRandom:
    ###The same interface as QueueRandom, using addresses (the default).
    ###It counts the blocks it makes, for the run's summary.
    def __init__(self):
        self.blocksMade = 0

    def randomBlocks(self, rngStart, ids, cycle, purpose, index):
        blocks = randomBlocks(rngStart, ids, cycle, purpose, index)
        self.blocksMade = self.blocksMade + len(blocks)
        return blocks
