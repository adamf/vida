"""Tests for Vida_Data/spatial_grid.py and its use in finding overlaps."""

import math
import random

import pytest

import geometry_utils
import list_utils
import spatial_grid
import vworldr


class Thing:
    def __init__(self, x, y, radius=0.1, isSeed=False):
        self.x = x
        self.y = y
        self.isSeed = isSeed
        self.radiusSeed = radius
        self.radiusStem = radius
        self.overlapList = []


def test_near_finds_things_within_the_distance_in_list_order():
    things = [Thing(5.0, 5.0), Thing(0.5, 0.0), Thing(-0.5, 0.2), Thing(0.0, 1.9)]
    grid = spatial_grid.SpatialGrid(things, 1.0)

    near = grid.near(0.0, 0.0, 2.0)

    assert near[0] is things[1]  # same order as the list,
    assert near[1] is things[2]  # not the order of the cells
    assert near[2] is things[3]
    assert things[0] not in near


def test_near_includes_things_exactly_at_the_distance():
    things = [Thing(3.0, 0.0), Thing(0.0, -3.0)]
    grid = spatial_grid.SpatialGrid(things, 1.0)
    assert grid.near(0.0, 0.0, 3.0) == things


def test_things_without_a_real_position_are_always_near():
    lost = Thing(math.nan, 0.0)
    grid = spatial_grid.SpatialGrid([Thing(50.0, 50.0), lost], 1.0)
    assert grid.near(0.0, 0.0, 1.0) == [lost]


def random_world(seed, count):
    rng = random.Random(seed)
    things = []
    for i in range(count):
        # mostly small seeds, some stems, a few big ones, packed closely
        radius = rng.choice([0.01, 0.02, 0.05, 0.2, 0.8])
        things.append(Thing(rng.uniform(-10, 10), rng.uniform(-10, 10), radius, isSeed=radius < 0.1))
    # a few exactly touching and exactly on top of each other
    things.append(Thing(0.0, 0.0, 0.5))
    things.append(Thing(1.0, 0.0, 0.5))
    things.append(Thing(1.0, 0.0, 0.2))
    return things


@pytest.mark.parametrize("seed", [1, 2, 3])
def test_the_grid_finds_exactly_the_same_overlaps(in_repo_root, seed):
    world = vworldr.garden()
    world.soil = random_world(seed, 2000)
    grid, largestRadius = world.makeOverlapGrid()

    for thing in world.soil:
        without_grid = world.checkForOverlap(thing)
        with_grid = world.checkForOverlap(thing, grid, largestRadius)
        assert with_grid == without_grid


class ShadeThing:
    """Just enough of a plant or seed for determineShade."""

    def __init__(self, rng, isSeed):
        self.x = rng.uniform(-10, 10)
        self.y = rng.uniform(-10, 10)
        self.isSeed = isSeed
        self.r = rng.uniform(0.01, 0.05) if isSeed else rng.uniform(0.1, 2.0)
        self.absHeightStem = 0.0 if isSeed else rng.uniform(0.5, 20.0)
        self.canopyTransmittance = rng.choice([0.0, 0.02, 0.3, 0.6])
        self.minimumLightForGermination = 0.0
        self.colourLeaf = [100.0, 1.0, 1.0]
        self.subregion = []
        self.overlapList = []
        self.areaCovered = 0.0
        self.name = "thing"


class ShadeWorld:
    def __init__(self, soil):
        self.soil = soil
        self.showProgressBar = False
        self.lightIntensity = 1.0


def expected_overlaps(soil):
    """determineShade's first step done the slow way: each plant against the
    first theIndex objects in the soil, where theIndex is plants done so far."""
    expected = {}
    theIndex = 0
    last_looked_at = None
    for plant in soil:
        if not plant.isSeed:
            found = []
            for j in range(theIndex):
                other = soil[j]
                last_looked_at = other
                if geometry_utils.checkOverlap(plant.x, plant.y, plant.r, other.x, other.y, other.r) > 0:
                    found.append(other)
            found = list_utils.sort_by_attr(found, "absHeightStem")
            found.reverse()
            expected[id(plant)] = found
            theIndex = theIndex + 1
    return expected, last_looked_at


@pytest.mark.parametrize("seed", [4, 5, 6])
def test_shading_finds_exactly_the_same_overlaps(seed):
    rng = random.Random(seed)
    soil = []
    for i in range(1500):
        soil.append(ShadeThing(rng, isSeed=rng.random() < 0.3))
    expected, last_looked_at = expected_overlaps(soil)

    random.seed(seed)
    vworldr.determineShade(ShadeWorld(soil))

    for plant in soil:
        if not plant.isSeed:
            assert plant.overlapList == expected[id(plant)]
            if len(plant.overlapList) == 1:
                # shaded by one other plant: uses the transmittance of the last
                # object the first step looked at (see determineShade)
                over = plant.overlapList[0]
                total = geometry_utils.areaCircle(plant.r)
                covered = geometry_utils.areaOverlappingCircles(plant.x, plant.y, plant.r, over.x, over.y, over.r)
                covered = covered - covered * last_looked_at.canopyTransmittance
                exposed = (total - covered) / total
                assert plant.areaCovered == total - total * exposed
