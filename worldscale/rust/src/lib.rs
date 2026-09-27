// worldscale_core: the compiled engine for Vida's world-scale prototype.
//
// It does one rank's work, on the same table of trees the Python prototype
// uses (worldscale/forest.py's Forest: one numpy array per column), working
// on the arrays in place. worldscale/compiled.py calls it, with the same
// functions and arguments as forest.py, so world.py can use either.
//
// The sums are forest.py's, in the same order. Its maths functions (log,
// pow, sin...) come from the libm crate, written in Rust, so the answers are
// the same on every computer, and results don't depend on how the world is
// split up or the order the trees are in, just as in the Python prototype.

// Vida uses 3.14 for pi and 0.7071067812 for one over the square root of
// two, so the engine does too, to give the same answers.
#![allow(clippy::approx_constant)]
// Loops over row numbers are easier to follow than iterator chains, and
// !(a > b) is written on purpose where a number might not be a number (NaN),
// as numpy's comparisons are.
#![allow(clippy::needless_range_loop, clippy::neg_cmp_op_on_partial_ord)]

use numpy::ndarray::Array2;
use numpy::{IntoPyArray, PyArray1, PyArray2, PyReadonlyArray1, PyReadwriteArray1, PyReadwriteArray2};
use pyo3::prelude::*;
use pyo3::types::PyDict;

mod growth;
mod maths;
mod pairs;
mod philox;
mod settings;
mod shading;

use growth::{Deaths, Trees};
use settings::{Species, World};

/// A Python int as the 64 bits of rngStart (negative numbers wrap round,
/// as they do in philox.py)
fn rng_bits(rng_start: &Bound<'_, PyAny>) -> PyResult<u64> {
    let masked = rng_start.call_method1("__and__", (0xFFFF_FFFF_FFFF_FFFFu64,))?;
    masked.extract()
}

/// The deaths counted, in the order of forest.CAUSES
fn death_counts(deaths: &Deaths) -> Vec<i64> {
    vec![
        deaths.failed_random,
        deaths.failed_immature,
        deaths.impossible_height,
        deaths.stem_off_world,
        deaths.random_death,
        deaths.growth_too_slow,
        deaths.buckled,
        0,
        deaths.lack_of_light,
        0,
    ]
}

/// Every column of a Forest, borrowed for writing
struct Columns<'py> {
    id: PyReadwriteArray1<'py, u64>,
    species: PyReadwriteArray1<'py, i32>,
    is_seed: PyReadwriteArray1<'py, bool>,
    x: PyReadwriteArray1<'py, f64>,
    y: PyReadwriteArray1<'py, f64>,
    birth_cycle: PyReadwriteArray1<'py, i32>,
    age: PyReadwriteArray1<'py, i32>,
    count_to_germ: PyReadwriteArray1<'py, i32>,
    mass_seed: PyReadwriteArray1<'py, f64>,
    radius_seed: PyReadwriteArray1<'py, f64>,
    mass_stem: PyReadwriteArray1<'py, f64>,
    mass_leaf: PyReadwriteArray1<'py, f64>,
    mass_fixed: PyReadwriteArray1<'py, f64>,
    mass_total: PyReadwriteArray1<'py, f64>,
    radius_stem: PyReadwriteArray1<'py, f64>,
    radius_leaf: PyReadwriteArray1<'py, f64>,
    r: PyReadwriteArray1<'py, f64>,
    height_stem: PyReadwriteArray1<'py, f64>,
    is_mature: PyReadwriteArray1<'py, bool>,
    area_covered: PyReadwriteArray1<'py, f64>,
    fixed_count: PyReadwriteArray1<'py, i32>,
    height_count: PyReadwriteArray1<'py, i32>,
    prev_height: PyReadwriteArray1<'py, f64>,
    avg_height_growth: PyReadwriteArray1<'py, f64>,
    max_avg_height_growth: PyReadwriteArray1<'py, f64>,
    attached_count: PyReadwriteArray1<'py, i32>,
    attached_mass: PyReadwriteArray1<'py, f64>,
    fixed_record: PyReadwriteArray2<'py, f64>,
    height_record: PyReadwriteArray2<'py, f64>,
}

impl<'py> Columns<'py> {
    fn read(forest: &Bound<'py, PyAny>) -> PyResult<Columns<'py>> {
        Ok(Columns {
            id: forest.getattr("id")?.extract()?,
            species: forest.getattr("species")?.extract()?,
            is_seed: forest.getattr("isSeed")?.extract()?,
            x: forest.getattr("x")?.extract()?,
            y: forest.getattr("y")?.extract()?,
            birth_cycle: forest.getattr("birthCycle")?.extract()?,
            age: forest.getattr("age")?.extract()?,
            count_to_germ: forest.getattr("countToGerm")?.extract()?,
            mass_seed: forest.getattr("massSeed")?.extract()?,
            radius_seed: forest.getattr("radiusSeed")?.extract()?,
            mass_stem: forest.getattr("massStem")?.extract()?,
            mass_leaf: forest.getattr("massLeaf")?.extract()?,
            mass_fixed: forest.getattr("massFixed")?.extract()?,
            mass_total: forest.getattr("massTotal")?.extract()?,
            radius_stem: forest.getattr("radiusStem")?.extract()?,
            radius_leaf: forest.getattr("radiusLeaf")?.extract()?,
            r: forest.getattr("r")?.extract()?,
            height_stem: forest.getattr("heightStem")?.extract()?,
            is_mature: forest.getattr("isMature")?.extract()?,
            area_covered: forest.getattr("areaCovered")?.extract()?,
            fixed_count: forest.getattr("fixedCount")?.extract()?,
            height_count: forest.getattr("heightCount")?.extract()?,
            prev_height: forest.getattr("prevHeight")?.extract()?,
            avg_height_growth: forest.getattr("avgHeightGrowth")?.extract()?,
            max_avg_height_growth: forest.getattr("maxAvgHeightGrowth")?.extract()?,
            attached_count: forest.getattr("attachedCount")?.extract()?,
            attached_mass: forest.getattr("attachedMass")?.extract()?,
            fixed_record: forest.getattr("fixedRecord")?.extract()?,
            height_record: forest.getattr("heightRecord")?.extract()?,
        })
    }

    fn trees(&mut self) -> PyResult<Trees<'_>> {
        Ok(Trees {
            id: self.id.as_slice_mut()?,
            species: self.species.as_slice_mut()?,
            is_seed: self.is_seed.as_slice_mut()?,
            x: self.x.as_slice_mut()?,
            y: self.y.as_slice_mut()?,
            birth_cycle: self.birth_cycle.as_slice_mut()?,
            age: self.age.as_slice_mut()?,
            count_to_germ: self.count_to_germ.as_slice_mut()?,
            mass_seed: self.mass_seed.as_slice_mut()?,
            radius_seed: self.radius_seed.as_slice_mut()?,
            mass_stem: self.mass_stem.as_slice_mut()?,
            mass_leaf: self.mass_leaf.as_slice_mut()?,
            mass_fixed: self.mass_fixed.as_slice_mut()?,
            mass_total: self.mass_total.as_slice_mut()?,
            radius_stem: self.radius_stem.as_slice_mut()?,
            radius_leaf: self.radius_leaf.as_slice_mut()?,
            r: self.r.as_slice_mut()?,
            height_stem: self.height_stem.as_slice_mut()?,
            is_mature: self.is_mature.as_slice_mut()?,
            area_covered: self.area_covered.as_slice_mut()?,
            fixed_count: self.fixed_count.as_slice_mut()?,
            height_count: self.height_count.as_slice_mut()?,
            prev_height: self.prev_height.as_slice_mut()?,
            avg_height_growth: self.avg_height_growth.as_slice_mut()?,
            max_avg_height_growth: self.max_avg_height_growth.as_slice_mut()?,
            attached_count: self.attached_count.as_slice_mut()?,
            attached_mass: self.attached_mass.as_slice_mut()?,
            fixed_record: self.fixed_record.as_slice_mut()?,
            height_record: self.height_record.as_slice_mut()?,
        })
    }
}

fn shape_problem(problem: numpy::ndarray::ShapeError) -> PyErr {
    pyo3::exceptions::PyValueError::new_err(problem.to_string())
}

/// philox.randomBlocks: four random numbers for each address
#[pyfunction]
fn random_blocks<'py>(py: Python<'py>, rng_start: &Bound<'py, PyAny>, ids: PyReadonlyArray1<'py, u64>,
                      cycle: u32, purpose: u32, index: PyReadonlyArray1<'py, i64>)
                      -> PyResult<Bound<'py, PyArray2<f64>>> {
    let start = rng_bits(rng_start)?;
    let ids = ids.as_slice()?;
    let index = index.as_slice()?;
    let mut numbers = Vec::with_capacity(ids.len() * 4);
    for place in 0..ids.len() {
        if index[place] < 0 || index[place] >= philox::INDEX_LIMIT as i64 {
            return Err(pyo3::exceptions::PyValueError::new_err("a random number's index must be from 0 to 16777215"));
        }
        let block = philox::random_block(start, ids[place], cycle, purpose, index[place] as u32);
        numbers.extend_from_slice(&block);
    }
    let array = Array2::from_shape_vec((ids.len(), 4), numbers)
        .map_err(shape_problem)?;
    Ok(array.into_pyarray(py))
}

/// forest.germinate: gives back which rows die, and the deaths counted
#[pyfunction]
fn germinate<'py>(py: Python<'py>, forest: &Bound<'py, PyAny>, cycle: u32, table: &Bound<'py, PyAny>,
                  world: &Bound<'py, PyAny>, rng_start: &Bound<'py, PyAny>)
                  -> PyResult<(Bound<'py, PyArray1<bool>>, Vec<i64>)> {
    let species = Species::read(table)?;
    let settings = World::read(world)?;
    let start = rng_bits(rng_start)?;
    let mut columns = Columns::read(forest)?;
    let mut trees = columns.trees()?;
    let mut dying = vec![false; trees.id.len()];
    let mut deaths = Deaths::default();
    growth::germinate(&mut trees, cycle, &species, &settings, start, &mut dying, &mut deaths);
    Ok((PyArray1::from_vec(py, dying), death_counts(&deaths)))
}

/// forest.grow: gives back which rows die, the dispersing plants' rows, how
/// many seeds each throws, the mass of each seed, and the deaths counted
#[pyfunction]
#[allow(clippy::type_complexity)]
fn grow<'py>(py: Python<'py>, forest: &Bound<'py, PyAny>, plants: PyReadonlyArray1<'py, i64>,
             table: &Bound<'py, PyAny>, world: &Bound<'py, PyAny>)
             -> PyResult<(Bound<'py, PyArray1<bool>>, Bound<'py, PyArray1<i64>>, Bound<'py, PyArray1<i64>>,
                          Bound<'py, PyArray1<f64>>, Vec<i64>)> {
    let species = Species::read(table)?;
    let settings = World::read(world)?;
    let mut columns = Columns::read(forest)?;
    let mut trees = columns.trees()?;
    let mut dying = vec![false; trees.id.len()];
    let mut deaths = Deaths::default();
    let dispersing = growth::grow(&mut trees, plants.as_slice()?, &species, &settings, &mut dying, &mut deaths);
    let mut rows = Vec::with_capacity(dispersing.len());
    let mut counts = Vec::with_capacity(dispersing.len());
    let mut masses = Vec::with_capacity(dispersing.len());
    for mother in &dispersing {
        rows.push(mother.row as i64);
        counts.push(mother.count);
        masses.push(mother.mass);
    }
    Ok((PyArray1::from_vec(py, dying), PyArray1::from_vec(py, rows), PyArray1::from_vec(py, counts),
        PyArray1::from_vec(py, masses), death_counts(&deaths)))
}

/// forest.disperse: where each dispersing seed lands. Gives back a dict of
/// the new seeds' columns, and their mothers' ids and positions and seed
/// numbers (for their ids).
#[pyfunction]
#[allow(clippy::too_many_arguments)]
fn disperse<'py>(py: Python<'py>, forest: &Bound<'py, PyAny>, mothers: PyReadonlyArray1<'py, i64>,
                 counts: PyReadonlyArray1<'py, i64>, masses: PyReadonlyArray1<'py, f64>, cycle: u32,
                 table: &Bound<'py, PyAny>, world: &Bound<'py, PyAny>, rng_start: &Bound<'py, PyAny>)
                 -> PyResult<Bound<'py, PyDict>> {
    let species = Species::read(table)?;
    let settings = World::read(world)?;
    let start = rng_bits(rng_start)?;
    let mut columns = Columns::read(forest)?;
    let trees = columns.trees()?;
    let mothers = mothers.as_slice()?;
    let counts = counts.as_slice()?;
    let masses = masses.as_slice()?;
    let mut dispersing = Vec::with_capacity(mothers.len());
    for place in 0..mothers.len() {
        dispersing.push(growth::Dispersing { row: mothers[place] as usize, count: counts[place], mass: masses[place] });
    }
    let seeds = growth::disperse(&trees, &dispersing, cycle, &species, &settings, start);
    let mut mother_id = Vec::with_capacity(seeds.len());
    let mut seed_number = Vec::with_capacity(seeds.len());
    let mut mother_x = Vec::with_capacity(seeds.len());
    let mut mother_y = Vec::with_capacity(seeds.len());
    let mut kind = Vec::with_capacity(seeds.len());
    let mut x = Vec::with_capacity(seeds.len());
    let mut y = Vec::with_capacity(seeds.len());
    let mut mass_seed = Vec::with_capacity(seeds.len());
    let mut radius_seed = Vec::with_capacity(seeds.len());
    let mut count_to_germ = Vec::with_capacity(seeds.len());
    for seed in &seeds {
        mother_id.push(seed.mother_id);
        seed_number.push(seed.seed_number);
        mother_x.push(seed.mother_x);
        mother_y.push(seed.mother_y);
        kind.push(seed.species);
        x.push(seed.x);
        y.push(seed.y);
        mass_seed.push(seed.mass_seed);
        radius_seed.push(seed.radius_seed);
        count_to_germ.push(seed.count_to_germ);
    }
    let result = PyDict::new(py);
    result.set_item("motherId", PyArray1::from_vec(py, mother_id))?;
    result.set_item("seedNumber", PyArray1::from_vec(py, seed_number))?;
    result.set_item("motherX", PyArray1::from_vec(py, mother_x))?;
    result.set_item("motherY", PyArray1::from_vec(py, mother_y))?;
    result.set_item("species", PyArray1::from_vec(py, kind))?;
    result.set_item("x", PyArray1::from_vec(py, x))?;
    result.set_item("y", PyArray1::from_vec(py, y))?;
    result.set_item("massSeed", PyArray1::from_vec(py, mass_seed))?;
    result.set_item("radiusSeed", PyArray1::from_vec(py, radius_seed))?;
    result.set_item("countToGerm", PyArray1::from_vec(py, count_to_germ))?;
    Ok(result)
}

/// forest.ownDeaths: gives back which rows die (with those already dying),
/// and the deaths counted
#[pyfunction]
fn own_deaths<'py>(py: Python<'py>, forest: &Bound<'py, PyAny>, already_dying: PyReadonlyArray1<'py, bool>,
                   cycle: u32, table: &Bound<'py, PyAny>, world: &Bound<'py, PyAny>, rng_start: &Bound<'py, PyAny>)
                   -> PyResult<(Bound<'py, PyArray1<bool>>, Vec<i64>)> {
    let species = Species::read(table)?;
    let settings = World::read(world)?;
    let start = rng_bits(rng_start)?;
    let mut columns = Columns::read(forest)?;
    let trees = columns.trees()?;
    let mut dying = already_dying.as_slice()?.to_vec();
    let mut deaths = Deaths::default();
    growth::own_deaths(&trees, cycle, &species, &settings, start, &mut dying, &mut deaths);
    Ok((PyArray1::from_vec(py, dying), death_counts(&deaths)))
}

/// forest.findPairs: every pair of overlapping circles, each once
#[pyfunction]
fn find_pairs<'py>(py: Python<'py>, x: PyReadonlyArray1<'py, f64>, y: PyReadonlyArray1<'py, f64>,
                   radius: PyReadonlyArray1<'py, f64>)
                   -> PyResult<(Bound<'py, PyArray1<i64>>, Bound<'py, PyArray1<i64>>)> {
    let (first, second) = pairs::find_pairs(x.as_slice()?, y.as_slice()?, radius.as_slice()?);
    Ok((PyArray1::from_vec(py, first), PyArray1::from_vec(py, second)))
}

/// forest.overlapWinners: overlapping pairs whose weaker one is ours, as
/// (the stronger's rows, the weaker's)
#[pyfunction]
#[allow(clippy::too_many_arguments)]
fn overlap_winners<'py>(py: Python<'py>, x: PyReadonlyArray1<'py, f64>, y: PyReadonlyArray1<'py, f64>,
                        radius: PyReadonlyArray1<'py, f64>, mass: PyReadonlyArray1<'py, f64>,
                        birth: PyReadonlyArray1<'py, i32>, ids: PyReadonlyArray1<'py, u64>, owned: usize)
                        -> PyResult<(Bound<'py, PyArray1<i64>>, Bound<'py, PyArray1<i64>>)> {
    let (winners, losers) = pairs::overlap_winners(x.as_slice()?, y.as_slice()?, radius.as_slice()?,
                                                   mass.as_slice()?, birth.as_slice()?, ids.as_slice()?, owned);
    Ok((PyArray1::from_vec(py, winners), PyArray1::from_vec(py, losers)))
}

/// forest.shade: each of the first `owned` rows' shaded area
#[pyfunction]
#[allow(clippy::too_many_arguments)]
fn shade<'py>(py: Python<'py>, owned: usize, x: PyReadonlyArray1<'py, f64>, y: PyReadonlyArray1<'py, f64>,
              r: PyReadonlyArray1<'py, f64>, is_plant: PyReadonlyArray1<'py, bool>,
              height: PyReadonlyArray1<'py, f64>, ids: PyReadonlyArray1<'py, u64>,
              species: PyReadonlyArray1<'py, i32>, cycle: u32, table: &Bound<'py, PyAny>,
              world: &Bound<'py, PyAny>, photon_limit: i64, rng_start: &Bound<'py, PyAny>)
              -> PyResult<Bound<'py, PyArray1<f64>>> {
    let table = Species::read(table)?;
    let settings = World::read(world)?;
    let start = rng_bits(rng_start)?;
    let covered = shading::shade(owned, x.as_slice()?, y.as_slice()?, r.as_slice()?, is_plant.as_slice()?,
                               height.as_slice()?, ids.as_slice()?, species.as_slice()?, cycle, &table,
                               &settings, photon_limit, start);
    Ok(PyArray1::from_vec(py, covered))
}

/// Move the rows to keep to the front of a column, in order. Gives back how
/// many were kept.
fn keep_rows<T: Copy>(column: &mut [T], keep: &[bool]) -> usize {
    let mut kept = 0;
    for row in 0..keep.len() {
        if keep[row] {
            column[kept] = column[row];
            kept += 1;
        }
    }
    kept
}

/// The same for a record column (MEMORY_SLOTS values per row)
fn keep_record_rows(record: &mut [f64], keep: &[bool]) -> usize {
    let slots = growth::MEMORY_SLOTS;
    let mut kept = 0;
    for row in 0..keep.len() {
        if keep[row] {
            for slot in 0..slots {
                record[kept * slots + slot] = record[row * slots + slot];
            }
            kept += 1;
        }
    }
    kept
}

/// Remove the rows not kept, in place: every column's kept rows are moved
/// to the front, in order. Gives back how many rows are kept; the table is
/// then the first that many rows of each column.
#[pyfunction]
fn compact<'py>(forest: &Bound<'py, PyAny>, keep: PyReadonlyArray1<'py, bool>) -> PyResult<usize> {
    let keep = keep.as_slice()?;
    let mut columns = Columns::read(forest)?;
    let trees = columns.trees()?;
    keep_rows(trees.id, keep);
    keep_rows(trees.species, keep);
    keep_rows(trees.is_seed, keep);
    keep_rows(trees.x, keep);
    keep_rows(trees.y, keep);
    keep_rows(trees.birth_cycle, keep);
    keep_rows(trees.age, keep);
    keep_rows(trees.count_to_germ, keep);
    keep_rows(trees.mass_seed, keep);
    keep_rows(trees.radius_seed, keep);
    keep_rows(trees.mass_stem, keep);
    keep_rows(trees.mass_leaf, keep);
    keep_rows(trees.mass_fixed, keep);
    keep_rows(trees.mass_total, keep);
    keep_rows(trees.radius_stem, keep);
    keep_rows(trees.radius_leaf, keep);
    keep_rows(trees.r, keep);
    keep_rows(trees.height_stem, keep);
    keep_rows(trees.is_mature, keep);
    keep_rows(trees.area_covered, keep);
    keep_rows(trees.fixed_count, keep);
    keep_rows(trees.height_count, keep);
    keep_rows(trees.prev_height, keep);
    keep_rows(trees.avg_height_growth, keep);
    keep_rows(trees.max_avg_height_growth, keep);
    keep_rows(trees.attached_count, keep);
    keep_rows(trees.attached_mass, keep);
    keep_record_rows(trees.fixed_record, keep);
    Ok(keep_record_rows(trees.height_record, keep))
}

/// forest.photosynthesise: gives back which rows die for lack of light,
/// and the deaths counted
#[pyfunction]
fn photosynthesise<'py>(py: Python<'py>, forest: &Bound<'py, PyAny>, table: &Bound<'py, PyAny>)
                        -> PyResult<(Bound<'py, PyArray1<bool>>, Vec<i64>)> {
    let species = Species::read(table)?;
    let mut columns = Columns::read(forest)?;
    let mut trees = columns.trees()?;
    let mut dying = vec![false; trees.id.len()];
    let mut deaths = Deaths::default();
    growth::photosynthesise(&mut trees, &species, &mut dying, &mut deaths);
    Ok((PyArray1::from_vec(py, dying), death_counts(&deaths)))
}

#[pymodule]
fn worldscale_core(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(random_blocks, module)?)?;
    module.add_function(wrap_pyfunction!(germinate, module)?)?;
    module.add_function(wrap_pyfunction!(grow, module)?)?;
    module.add_function(wrap_pyfunction!(disperse, module)?)?;
    module.add_function(wrap_pyfunction!(own_deaths, module)?)?;
    module.add_function(wrap_pyfunction!(find_pairs, module)?)?;
    module.add_function(wrap_pyfunction!(overlap_winners, module)?)?;
    module.add_function(wrap_pyfunction!(shade, module)?)?;
    module.add_function(wrap_pyfunction!(photosynthesise, module)?)?;
    module.add_function(wrap_pyfunction!(compact, module)?)?;
    Ok(())
}
