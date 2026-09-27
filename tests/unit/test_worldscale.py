"""Tests for the world-scale prototype (worldscale/).

The two things it's for:
- random numbers with addresses: the same numbers for a tree whatever
  order or batch it's dealt with in (philox.py);
- one world split over several ranks gives exactly the same forest however
  many ranks there are and however the tiles are shared out (world.py).
"""

import sys
from pathlib import Path

import numpy
import pytest

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO))

from worldscale import comm as comms  # noqa: E402
from worldscale import forest as trees  # noqa: E402
from worldscale import philox  # noqa: E402
from worldscale import run  # noqa: E402
from worldscale import species  # noqa: E402
from worldscale import world as worlds  # noqa: E402


# --------------------------------------------------------------------------
# Random numbers with addresses
# --------------------------------------------------------------------------

def test_philox_gives_random123s_known_answers():
    # Random123's known-answer tests for philox4x32-10: counter, key, answer
    known = [
        ((0, 0, 0, 0), (0, 0), (0x6627E8D5, 0xE169C58D, 0xBC57AC4C, 0x9B00DBD8)),
        ((0xFFFFFFFF,) * 4, (0xFFFFFFFF, 0xFFFFFFFF), (0x408F276D, 0x41C83B0E, 0xA20BC7C6, 0x6D5451FD)),
        ((0x243F6A88, 0x85A308D3, 0x13198A2E, 0x03707344), (0xA4093822, 0x299F31D0),
         (0xD16CFE09, 0x94FDCCEB, 0x5001E420, 0x24126EA1)),
    ]
    for counter, key, answer in known:
        words = philox.philox4x32(*counter, *key)
        assert [int(word) for word in words] == list(answer)


def test_a_trees_numbers_dont_depend_on_order_or_batch():
    ids = numpy.arange(1000, dtype=numpy.uint64) * numpy.uint64(7919)
    everything = philox.randomBlocks(42, ids, 7, philox.PHOTON, numpy.arange(1000) % 750)
    shuffled = numpy.random.default_rng(3).permutation(1000)
    again = philox.randomBlocks(42, ids[shuffled], 7, philox.PHOTON, (numpy.arange(1000) % 750)[shuffled])
    assert numpy.array_equal(everything[shuffled], again)
    one = philox.randomBlocks(42, ids[5:6], 7, philox.PHOTON, numpy.array([5]))
    assert numpy.array_equal(one[0], everything[5])


def test_different_addresses_give_different_numbers():
    ids = numpy.arange(20000, dtype=numpy.uint64)
    blocks = philox.randomBlocks(1, ids, 3, philox.RANDOM_DEATH, 0)
    assert 0.0 < blocks.min() and blocks.max() < 1.0
    assert abs(blocks.mean() - 0.5) < 0.01
    assert len(numpy.unique(blocks[:, 0])) == len(ids)
    for changed in (philox.randomBlocks(2, ids, 3, philox.RANDOM_DEATH, 0),
                    philox.randomBlocks(1, ids, 4, philox.RANDOM_DEATH, 0),
                    philox.randomBlocks(1, ids, 3, philox.SLOW_GROWTH, 0),
                    philox.randomBlocks(1, ids, 3, philox.RANDOM_DEATH, 1)):
        assert not numpy.any(changed[:, 0] == blocks[:, 0])


# --------------------------------------------------------------------------
# Finding overlapping circles
# --------------------------------------------------------------------------

def test_find_pairs_matches_checking_every_pair():
    rng = numpy.random.default_rng(8)
    for trial in range(5):
        count = 400
        x = rng.uniform(-20, 20, count)
        y = rng.uniform(-20, 20, count)
        radius = numpy.where(rng.random(count) < 0.7, rng.uniform(0.0, 0.05, count), rng.uniform(0.05, 6.0, count))
        first, second = trees.findPairs(x, y, radius)
        found = set()
        for a, b in zip(first.tolist(), second.tolist()):
            found.add((min(a, b), max(a, b)))
        assert len(found) == len(first)  # each pair once
        expected = set()
        for a in range(count):
            for b in range(a + 1, count):
                reach = radius[a] + radius[b]
                if (x[a] - x[b]) ** 2 + (y[a] - y[b]) ** 2 <= reach * reach:
                    expected.add((a, b))
        assert found == expected


# --------------------------------------------------------------------------
# Crushing: decided in rounds, the same on any number of ranks
# --------------------------------------------------------------------------

def crowded_stems(count, rng):
    stems = trees.Forest(count)
    stems.id = numpy.arange(count, dtype=numpy.uint64) + numpy.uint64(1)
    stems.x = rng.uniform(-9.9, 9.9, count)
    stems.y = rng.uniform(-9.9, 9.9, count)
    stems.radiusStem = rng.uniform(0.1, 0.6, count)
    stems.massTotal = numpy.round(rng.uniform(1, 30, count))  # some the same weight
    stems.birthCycle = rng.integers(0, 3, count).astype(numpy.int32)
    return stems


def strongest_first_one_at_a_time(stems):
    # the rule, done the slow obvious way: strongest first, each tree
    # surviving unless it overlaps a stronger one that survived
    order = numpy.lexsort((stems.id, stems.birthCycle, -stems.massTotal))
    alive = []
    crushed = set()
    for row in order.tolist():
        hit = False
        for other in alive:
            reach = stems.radiusStem[row] + stems.radiusStem[other]
            if (stems.x[row] - stems.x[other]) ** 2 + (stems.y[row] - stems.y[other]) ** 2 <= reach * reach:
                hit = True
                break
        if hit:
            crushed.add(int(stems.id[row]))
        else:
            alive.append(row)
    return crushed


def crush_on_ranks(comm, stems, partition, tileSize):
    table = species.SpeciesTable(species.speciesFilesIn(str(REPO / "Species")), str(REPO))
    settings = worlds.Settings(worldSize=20.0, tileSize=tileSize, seedsPerHectare=0, partition=partition)
    theWorld = worlds.TiledWorld(comm, settings, table, species.WorldSettings(str(REPO)))
    column, row = theWorld.tileOf(stems.x, stems.y)
    theWorld.forest = stems.take(theWorld.owner[column, row] == comm.rank)
    crushed, rounds = theWorld.crush()
    return set(theWorld.forest.id[crushed].tolist()), rounds


def test_crushing_in_rounds_is_strongest_first_on_any_number_of_ranks():
    rng = numpy.random.default_rng(4)
    for trial in range(3):
        stems = crowded_stems(600, rng)
        expected = strongest_first_one_at_a_time(stems)
        assert len(expected) > 50
        for ranks, partition in ((1, "strips"), (2, "strips"), (4, "scattered")):
            answers = comms.runAsThreads(ranks, crush_on_ranks, (stems, partition, 2.5))
            crushed = set()
            for rankCrushed, rounds in answers:
                crushed |= rankCrushed
            assert crushed == expected
            assert rounds > 1  # chains take more than one round


def test_the_old_one_at_a_time_rule_depends_on_the_order():
    # A (heaviest) overlaps B, B overlaps C, A doesn't overlap C.
    # Strongest first: A survives, B is crushed, so C survives.
    # Vida's list order C, B, A: C is crushed by B, then B by A.
    stems = trees.Forest(3)
    stems.id = numpy.array([1, 2, 3], dtype=numpy.uint64)
    stems.x = numpy.array([0.0, 0.5, 1.0])
    stems.radiusStem = numpy.array([0.3, 0.3, 0.3])
    stems.massTotal = numpy.array([30.0, 20.0, 10.0])
    assert strongest_first_one_at_a_time(stems) == {2}
    table = species.SpeciesTable(species.speciesFilesIn(str(REPO / "Species")), str(REPO))
    for crush, order, expected in (("rounds", [2, 1, 0], {2}), ("rounds", [0, 1, 2], {2}),
                                   ("sequential", [2, 1, 0], {2, 3}), ("sequential", [0, 1, 2], {2})):
        settings = worlds.Settings(worldSize=20.0, tileSize=5.0, seedsPerHectare=0, crush=crush)
        theWorld = worlds.TiledWorld(comms.SerialComm(), settings, table, species.WorldSettings(str(REPO)))
        theWorld.forest = stems.take(numpy.array(order))
        crushed, rounds = theWorld.crush()
        assert set(theWorld.forest.id[crushed].tolist()) == expected


# --------------------------------------------------------------------------
# Shading
# --------------------------------------------------------------------------

def test_photons_stop_at_the_tallest_canopy_they_land_in():
    # a small plant under two big canopies that each cover all of it: the
    # taller lets every photon through, the shorter none. Photons stop at
    # the first canopy they land in, tallest first, so all get through.
    table = species.SpeciesTable(species.speciesFilesIn(str(REPO / "Species"))[:2], str(REPO))
    table.canopyTransmittance = numpy.array([1.0, 0.0])
    world = species.WorldSettings(str(REPO))
    x = numpy.array([0.0, 0.1, -0.1])
    y = numpy.zeros(3)
    r = numpy.array([0.5, 3.0, 3.0])
    ids = numpy.array([7, 8, 9], dtype=numpy.uint64)
    isPlant = numpy.ones(3, dtype=bool)
    for tallRow in (1, 2):
        for tallSpecies, expectedCover in ((0, 0.0), (1, 1.0)):
            heights = numpy.array([1.0, 10.0, 10.0])
            heights[tallRow] = 20.0
            speciesOf = numpy.full(3, 1 - tallSpecies, dtype=numpy.int32)
            speciesOf[tallRow] = tallSpecies
            covered = trees.shade(1, x, y, r, isPlant, heights, ids, speciesOf, 0, table, world, 750, 1, philox.AddressedRandom())
            area = 3.14 * 0.5 * 0.5
            assert covered[0] == pytest.approx(area * expectedCover)


# --------------------------------------------------------------------------
# A whole world: the same on any number of ranks
# --------------------------------------------------------------------------

def run_small_world(ranks, extra):
    options = run.readOptions(["-w", "60", "-tile", "15", "-t", "22", "-photons", "200", "-quiet",
                               "-species", str(REPO / "Species")] + extra)
    return comms.runAsThreads(ranks, run.runWorld, (options,))[0]


def test_the_forest_is_the_same_on_any_number_of_ranks(in_repo_root):
    first = run_small_world(1, [])
    assert first["cycles"][-1]["plants"] > 100 and first["cycles"][-1]["born"] > 10
    for ranks, extra in ((2, []), (3, ["-partition", "curve"]), (4, ["-partition", "scattered", "-shuffle"])):
        assert run_small_world(ranks, extra)["fingerprint"] == first["fingerprint"]


def test_a_queue_of_random_numbers_depends_on_the_split(in_repo_root):
    one = run_small_world(1, ["-rng", "queue"])
    two = run_small_world(2, ["-rng", "queue"])
    assert one["fingerprint"] != two["fingerprint"]


def test_every_tree_has_its_own_id(in_repo_root):
    table = species.SpeciesTable(species.speciesFilesIn("Species"), ".")
    settings = worlds.Settings(worldSize=60.0, tileSize=15.0, photonLimit=100)
    theWorld = worlds.TiledWorld(comms.SerialComm(), settings, table, species.WorldSettings("."))
    seen = set(theWorld.forest.id.tolist())
    for cycle in range(20):
        before = set(theWorld.forest.id.tolist())
        theWorld.runCycle()
        new = set(theWorld.forest.id.tolist()) - before
        assert not (new & seen)
        seen |= new
    assert len(seen) > len(theWorld.forest)


# --------------------------------------------------------------------------
# The compiled engine (worldscale/rust, built with maturin)
# --------------------------------------------------------------------------

try:
    from worldscale import compiled as compiled_engine  # noqa: E402
except ImportError:
    compiled_engine = None
needs_rust = pytest.mark.skipif(compiled_engine is None, reason="the Rust engine isn't built (see worldscale/rust)")


@needs_rust
def test_the_compiled_engines_random_numbers_are_the_same():
    ids = numpy.arange(50000, dtype=numpy.uint64) * numpy.uint64(2654435761) + numpy.uint64(1 << 40)
    index = numpy.arange(50000) % 750
    for rngStart in (1, 42, -1, 2**63 + 5):
        expected = philox.randomBlocks(rngStart, ids, 9, philox.PHOTON, index)
        assert numpy.array_equal(compiled_engine.randomBlocks(rngStart, ids, 9, philox.PHOTON, index), expected)


@needs_rust
def test_the_compiled_engine_finds_the_same_pairs_and_winners():
    rng = numpy.random.default_rng(12)
    for trial in range(5):
        count = 3000
        x = rng.uniform(-40, 40, count)
        y = rng.uniform(-40, 40, count)
        radius = numpy.where(rng.random(count) < 0.8, rng.uniform(0.0, 0.1, count), rng.uniform(0.1, 6.0, count))
        radius[:20] = 0.05  # some the same size
        mass = numpy.round(rng.uniform(1, 20, count))
        birth = rng.integers(0, 3, count).astype(numpy.int32)
        ids = rng.permutation(count).astype(numpy.uint64)
        pairs = []
        for first, second in (trees.findPairs(x, y, radius), compiled_engine.findPairs(x, y, radius)):
            found = set()
            for a, b in zip(first.tolist(), second.tolist()):
                found.add((min(a, b), max(a, b)))
            assert len(found) == len(first)
            pairs.append(found)
        assert pairs[0] == pairs[1]
        winners = []
        for engine in (trees, compiled_engine):
            winner, loser = engine.overlapWinners(x, y, radius, mass, birth, ids, 2000)
            winners.append(set(zip(winner.tolist(), loser.tolist())))
        assert winners[0] == winners[1]


def small_world(engine, cycles):
    table = species.SpeciesTable(species.speciesFilesIn(str(REPO / "Species")), str(REPO))
    settings = worlds.Settings(worldSize=60.0, tileSize=15.0, photonLimit=200, engine=engine)
    theWorld = worlds.TiledWorld(comms.SerialComm(), settings, table, species.WorldSettings(str(REPO)))
    for cycle in range(cycles):
        theWorld.runCycle()
    return theWorld.forest


@needs_rust
def test_the_compiled_engine_grows_the_same_trees(in_repo_root):
    # the same births, deaths and crushes; the values can differ in the last
    # digits, as its maths functions are its own
    numpyForest = small_world("numpy", 22)
    rustForest = small_world("rust", 22)
    assert len(numpyForest) > 300
    numpyOrder = numpy.argsort(numpyForest.id)
    rustOrder = numpy.argsort(rustForest.id)
    assert numpy.array_equal(numpyForest.id[numpyOrder], rustForest.id[rustOrder])
    for name in ("x", "y", "massStem", "massLeaf", "massFixed", "heightStem", "r", "areaCovered", "attachedMass"):
        assert numpy.allclose(getattr(numpyForest, name)[numpyOrder], getattr(rustForest, name)[rustOrder],
                              rtol=1e-9, atol=1e-12), name
    for name in ("species", "isSeed", "age", "isMature", "attachedCount", "fixedCount"):
        assert numpy.array_equal(getattr(numpyForest, name)[numpyOrder], getattr(rustForest, name)[rustOrder]), name


@needs_rust
def test_the_compiled_engines_forest_is_the_same_on_any_number_of_ranks(in_repo_root):
    first = run_small_world(1, ["-engine", "rust"])
    for ranks, extra in ((2, []), (4, ["-partition", "scattered", "-shuffle"])):
        assert run_small_world(ranks, extra + ["-engine", "rust"])["fingerprint"] == first["fingerprint"]
