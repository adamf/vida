"""Tests for Vida_Data/spatial_grid.py and its use in finding overlaps."""

import math
import random

import pytest

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
