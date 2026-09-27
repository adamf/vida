// worldscale_core: the compiled engine for Vida's world-scale prototype, as
// a Python module. The model and the world are in the engine crate
// (../engine); this is what Python sees of them.
//
// Two ways to use it:
//  - the kernels (germinate, grow, shade...): the same functions and
//    arguments as worldscale/forest.py, working in place on the numpy
//    arrays of a Python Forest. worldscale/compiled.py calls them, so
//    world.py can use either (-engine rust).
//  - RankWorld: one rank's whole share of the world, every step of the
//    cycle in Rust (-engine rust-world). It talks to the other ranks through
//    a Python comm (worldscale/comm.py), so it runs on one process, as
//    threads, or over MPI with mpi4py, like world.py.
//
// Both share each rank's work out between the computer's cores (rayon).

// Loops over row numbers are easier to follow than iterator chains
#![allow(clippy::needless_range_loop)]

use numpy::ndarray::Array2;
use numpy::{IntoPyArray, PyArray1, PyArray2, PyReadonlyArray1, PyReadwriteArray1, PyReadwriteArray2};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList};

use worldscale_engine::comm::{Comm, Problem};
use worldscale_engine::forest::Trees;
use worldscale_engine::growth::{self, Deaths, CAUSES};
use worldscale_engine::settings::{Species, World};
use worldscale_engine::world::{self, Partition};
use worldscale_engine::{pairs, philox, shading};

/// A Python int as the 64 bits of rngStart (negative numbers wrap round,
/// as they do in philox.py)
fn rng_bits(rng_start: &Bound<'_, PyAny>) -> PyResult<u64> {
    let masked = rng_start.call_method1("__and__", (0xFFFF_FFFF_FFFF_FFFFu64,))?;
    masked.extract()
}

/// The deaths counted, in the order of forest.CAUSES
fn death_counts(deaths: &Deaths) -> Vec<u64> {
    deaths.counts.to_vec()
}

// ---------------------------------------------------------------------
// Settings, from the Python objects (species.SpeciesTable and WorldSettings)
// ---------------------------------------------------------------------

fn numbers(table: &Bound<'_, PyAny>, name: &str) -> PyResult<Vec<f64>> {
    let array: PyReadonlyArray1<f64> = table.getattr(name)?.extract()?;
    Ok(array.as_slice()?.to_vec())
}

fn whole_numbers(table: &Bound<'_, PyAny>, name: &str) -> PyResult<Vec<i64>> {
    let array: PyReadonlyArray1<i64> = table.getattr(name)?.extract()?;
    Ok(array.as_slice()?.to_vec())
}

fn true_false(table: &Bound<'_, PyAny>, name: &str) -> PyResult<Vec<bool>> {
    let array: PyReadonlyArray1<bool> = table.getattr(name)?.extract()?;
    Ok(array.as_slice()?.to_vec())
}

fn read_species(table: &Bound<'_, PyAny>) -> PyResult<Species> {
    Ok(Species {
        density_stem: numbers(table, "densityStem")?,
        density_leaf: numbers(table, "densityLeaf")?,
        density_seed: numbers(table, "densitySeed")?,
        canopy_transmittance: numbers(table, "canopyTransmittance")?,
        fraction_minimum_survival: numbers(table, "fractionMinimumSurvival")?,
        height_leaf_max: numbers(table, "heightLeafMax")?,
        height_stem_max: numbers(table, "heightStemMax")?,
        youngs_modulus_stem: numbers(table, "youngsModulusStem")?,
        fraction_selfishness: numbers(table, "fractionSelfishness")?,
        start_making_seeds_age: numbers(table, "startMakingSeedsAge")?,
        reproduction_constant: numbers(table, "reproductionConstant")?,
        reproduction_exponent: numbers(table, "reproductionExponent")?,
        mass_seed_max: numbers(table, "massSeedMax")?,
        delay_in_germination: numbers(table, "delayInGermination")?,
        random_slow_growth: numbers(table, "randomSlowGrowth")?,
        fraction_fail_germinate: numbers(table, "fractionFailGerminate")?,
        photo_constant: numbers(table, "photoConstant")?,
        photo_constant_shade: numbers(table, "photoConstantShade")?,
        photo_exponent: numbers(table, "photoExponent")?,
        fraction_carbon_to_seeds: numbers(table, "fractionCarbonToSeeds")?,
        fract_mass_seed_max_to_germ: numbers(table, "fractMassSeedMaxToGerm")?,
        fraction_seed_mass_to_plant: numbers(table, "fractionSeedMassToPlant")?,
        fraction_carbon_to_stem: numbers(table, "fractionCarbonToStem")?,
        constant1: numbers(table, "speciesConstant1")?,
        exponent1: numbers(table, "speciesExponent1")?,
        constant2: numbers(table, "speciesConstant2")?,
        exponent2: numbers(table, "speciesExponent2")?,
        constant3: numbers(table, "speciesConstant3")?,
        exponent3: numbers(table, "speciesExponent3")?,
        constant6: numbers(table, "speciesConstant6")?,
        constant7: numbers(table, "speciesConstant7")?,
        exponent7: numbers(table, "speciesExponent7")?,
        constant8: numbers(table, "speciesConstant8")?,
        constant20: numbers(table, "speciesConstant20")?,
        exponent20: numbers(table, "speciesExponent20")?,
        makes_seeds: true_false(table, "makeSeeds")?,
        leaf_is_hemisphere: true_false(table, "leafIsHemisphere")?,
        formation_max: numbers(table, "formationMax")?,
        formation_min: numbers(table, "formationMin")?,
        dispersal_method: whole_numbers(table, "dispersalMethod")?,
        dispersal1: numbers(table, "dispersal1")?,
        dispersal2: numbers(table, "dispersal2")?,
        memory: whole_numbers(table, "memory")?,
    })
}

fn read_world(world: &Bound<'_, PyAny>) -> PyResult<World> {
    Ok(World {
        world_size: world.getattr("worldSize")?.extract()?,
        gravity: world.getattr("gravity")?.extract()?,
        light_intensity: world.getattr("lightIntensity")?.extract()?,
        max_seeds_per_plant: world.getattr("maxSeedsPerPlant")?.extract()?,
        ignore_germ_death_at_start: world.getattr("ignoreGermDeathAtStart")?.extract()?,
        allow_random_death: world.getattr("allowRandomDeath")?.extract()?,
        random_death_plant: world.getattr("randomDeathPlant")?.extract()?,
        random_death_seed: world.getattr("randomDeathSeed")?.extract()?,
        allow_slow_growth_death: world.getattr("allowSlowGrowthDeath")?.extract()?,
        random_slow_growth: world.getattr("randomSlowGrowth")?.extract()?,
        allow_euler_greenhill_violations: world.getattr("allowEulerGreenhillViolations")?.extract()?,
        allow_overlaps: world.getattr("allowOverlaps")?.extract()?,
        allow_off_world: world.getattr("allowOffWorld")?.extract()?,
    })
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
                  -> PyResult<(Bound<'py, PyArray1<bool>>, Vec<u64>)> {
    let species = read_species(table)?;
    let settings = read_world(world)?;
    let start = rng_bits(rng_start)?;
    let mut columns = Columns::read(forest)?;
    let mut trees = columns.trees()?;
    let mut dying = vec![false; trees.id.len()];
    let deaths = growth::germinate(&mut trees, cycle, &species, &settings, start, &mut dying);
    Ok((PyArray1::from_vec(py, dying), death_counts(&deaths)))
}

/// forest.grow: gives back which rows die, the dispersing plants' rows, how
/// many seeds each throws, the mass of each seed, and the deaths counted
#[pyfunction]
#[allow(clippy::type_complexity)]
fn grow<'py>(py: Python<'py>, forest: &Bound<'py, PyAny>, plants: PyReadonlyArray1<'py, i64>,
             table: &Bound<'py, PyAny>, world: &Bound<'py, PyAny>)
             -> PyResult<(Bound<'py, PyArray1<bool>>, Bound<'py, PyArray1<i64>>, Bound<'py, PyArray1<i64>>,
                          Bound<'py, PyArray1<f64>>, Vec<u64>)> {
    let species = read_species(table)?;
    let settings = read_world(world)?;
    let mut columns = Columns::read(forest)?;
    let mut trees = columns.trees()?;
    let mut dying = vec![false; trees.id.len()];
    let mut was_plant = vec![false; trees.id.len()];
    for &row in plants.as_slice()? {
        was_plant[row as usize] = true;
    }
    let (deaths, dispersing) = growth::grow(&mut trees, &was_plant, &species, &settings, &mut dying);
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
    let species = read_species(table)?;
    let settings = read_world(world)?;
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
                   -> PyResult<(Bound<'py, PyArray1<bool>>, Vec<u64>)> {
    let species = read_species(table)?;
    let settings = read_world(world)?;
    let start = rng_bits(rng_start)?;
    let mut columns = Columns::read(forest)?;
    let trees = columns.trees()?;
    let mut dying = already_dying.as_slice()?.to_vec();
    let deaths = growth::own_deaths(&trees, cycle, &species, &settings, start, &mut dying);
    Ok((PyArray1::from_vec(py, dying), death_counts(&deaths)))
}

/// Two lists of row numbers, pair by pair
type RowPairs<'py> = (Bound<'py, PyArray1<i64>>, Bound<'py, PyArray1<i64>>);

/// forest.findPairs: every pair of overlapping circles, each once
#[pyfunction]
fn find_pairs<'py>(py: Python<'py>, x: PyReadonlyArray1<'py, f64>, y: PyReadonlyArray1<'py, f64>,
                   radius: PyReadonlyArray1<'py, f64>)
                   -> PyResult<RowPairs<'py>> {
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
                        -> PyResult<RowPairs<'py>> {
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
    let table = read_species(table)?;
    let settings = read_world(world)?;
    let start = rng_bits(rng_start)?;
    let covered = shading::shade(owned, x.as_slice()?, y.as_slice()?, r.as_slice()?, is_plant.as_slice()?,
                               height.as_slice()?, ids.as_slice()?, species.as_slice()?, cycle, &table,
                               &settings, photon_limit, start);
    Ok(PyArray1::from_vec(py, covered))
}

/// Remove the rows not kept, in place: every column's kept rows are moved
/// to the front, in order. Gives back how many rows are kept; the table is
/// then the first that many rows of each column.
#[pyfunction]
fn compact<'py>(forest: &Bound<'py, PyAny>, keep: PyReadonlyArray1<'py, bool>) -> PyResult<usize> {
    let keep = keep.as_slice()?;
    let mut columns = Columns::read(forest)?;
    let mut trees = columns.trees()?;
    Ok(trees.keep_where(keep, true))
}

/// forest.photosynthesise: gives back which rows die for lack of light,
/// and the deaths counted
#[pyfunction]
fn photosynthesise<'py>(py: Python<'py>, forest: &Bound<'py, PyAny>, table: &Bound<'py, PyAny>)
                        -> PyResult<(Bound<'py, PyArray1<bool>>, Vec<u64>)> {
    let species = read_species(table)?;
    let mut columns = Columns::read(forest)?;
    let mut trees = columns.trees()?;
    let mut dying = vec![false; trees.id.len()];
    let deaths = growth::photosynthesise(&mut trees, &species, &mut dying);
    Ok((PyArray1::from_vec(py, dying), death_counts(&deaths)))
}

// ---------------------------------------------------------------------
// The whole world in Rust
// ---------------------------------------------------------------------

/// A comm that talks through a Python comm object (worldscale/comm.py's
/// SerialComm, ThreadComm or MpiComm): letters go as bytes, sums as numpy
/// arrays of uint64 (so they wrap round at 2**64, like the Rust sums)
struct PyComm {
    comm: Py<PyAny>,
    rank: usize,
    size: usize,
}

impl PyComm {
    fn alltoall_python(&self, py: Python<'_>, letters: &[Vec<u8>]) -> PyResult<Vec<Vec<u8>>> {
        let mut parts = Vec::with_capacity(letters.len());
        for letter in letters {
            parts.push(PyBytes::new(py, letter));
        }
        let received = self.comm.bind(py).call_method1("alltoall", (PyList::new(py, parts)?,))?;
        let mut letters = Vec::with_capacity(self.size);
        for letter in received.try_iter()? {
            letters.push(letter?.cast::<PyBytes>()?.as_bytes().to_vec());
        }
        Ok(letters)
    }

    fn sum_python(&self, py: Python<'_>, numbers: &[u64]) -> PyResult<Vec<u64>> {
        let array = PyArray1::from_slice(py, numbers);
        let total = self.comm.bind(py).call_method1("allreduce", (array, "sum"))?;
        let total: PyReadonlyArray1<u64> = total.extract()?;
        Ok(total.as_slice()?.to_vec())
    }

    fn max_python(&self, py: Python<'_>, number: f64) -> PyResult<f64> {
        self.comm.bind(py).call_method1("allreduce", (number, "max"))?.extract()
    }
}

impl Comm for PyComm {
    fn rank(&self) -> usize {
        self.rank
    }

    fn size(&self) -> usize {
        self.size
    }

    fn alltoall(&mut self, letters: Vec<Vec<u8>>) -> Result<Vec<Vec<u8>>, Problem> {
        Python::attach(|py| self.alltoall_python(py, &letters)).map_err(|problem| Box::new(problem) as Problem)
    }

    fn sum(&mut self, numbers: &[u64]) -> Result<Vec<u64>, Problem> {
        Python::attach(|py| self.sum_python(py, numbers)).map_err(|problem| Box::new(problem) as Problem)
    }

    fn max(&mut self, number: f64) -> Result<f64, Problem> {
        Python::attach(|py| self.max_python(py, number)).map_err(|problem| Box::new(problem) as Problem)
    }
}

/// A problem from the engine, as a Python exception (the Python one itself,
/// if it came from the Python comm)
fn python_problem(problem: Problem) -> PyErr {
    match problem.downcast::<PyErr>() {
        Ok(python) => *python,
        Err(other) => PyRuntimeError::new_err(other.to_string()),
    }
}

/// One rank's share of the world, with every step of the cycle done in Rust:
/// world.py's TiledWorld, giving exactly the same forest.
///
/// RankWorld(comm, table, world, worldSize, tileSize, seedsPerHectare,
///           rngStart, photonLimit, partition, shuffle, threads=0)
///
/// comm is a comm from worldscale/comm.py; table and world are a
/// species.SpeciesTable and species.WorldSettings. threads is how many
/// cores this rank uses (0: rayon's own choice, usually all of them).
#[pyclass(module = "worldscale_core")]
struct RankWorld {
    world: world::RankWorld,
    comm: PyComm,
    pool: Option<rayon::ThreadPool>,
}

impl RankWorld {
    /// Run some work on this rank's cores, with Python let go while it runs
    /// (so other ranks' threads can run their Python)
    fn run<T, F>(&mut self, py: Python<'_>, work: F) -> PyResult<T>
    where
        T: Send,
        F: FnOnce(&mut world::RankWorld, &mut PyComm) -> Result<T, Problem> + Send,
    {
        let RankWorld { world, comm, pool } = self;
        let answer = py.detach(|| match pool {
            Some(pool) => pool.install(|| work(world, comm)),
            None => work(world, comm),
        });
        answer.map_err(python_problem)
    }
}

#[pymethods]
impl RankWorld {
    #[new]
    #[pyo3(signature = (comm, table, world, world_size, tile_size, seeds_per_hectare, rng_start, photon_limit,
                        partition, shuffle, threads=0))]
    #[allow(clippy::too_many_arguments)]
    fn new(py: Python<'_>, comm: &Bound<'_, PyAny>, table: &Bound<'_, PyAny>, world: &Bound<'_, PyAny>,
           world_size: f64, tile_size: f64, seeds_per_hectare: f64, rng_start: &Bound<'_, PyAny>,
           photon_limit: i64, partition: &str, shuffle: bool, threads: usize) -> PyResult<RankWorld> {
        let rank: usize = comm.getattr("rank")?.extract()?;
        let size: usize = comm.getattr("size")?.extract()?;
        let settings = world::Settings {
            world_size,
            tile_size,
            seeds_per_hectare,
            rng_start: rng_bits(rng_start)?,
            photon_limit,
            partition: Partition::from_name(partition).map_err(PyValueError::new_err)?,
            shuffle,
        };
        let species = read_species(table)?;
        let mut world_settings = read_world(world)?;
        world_settings.world_size = world_size;
        let pool = if threads > 0 {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .map_err(|problem| PyRuntimeError::new_err(problem.to_string()))?;
            Some(pool)
        } else {
            None
        };
        let make = || world::RankWorld::new(rank, size, settings, species, world_settings);
        let made = py.detach(|| match &pool {
            Some(pool) => pool.install(make),
            None => make(),
        });
        Ok(RankWorld {
            world: made.map_err(python_problem)?,
            comm: PyComm { comm: comm.clone().unbind(), rank, size },
            pool,
        })
    }

    /// Run one cycle. Gives back its counts over the whole world, as
    /// world.py's runCycle does.
    fn run_cycle<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let summary = self.run(py, |world, comm| world.run_cycle(comm))?;
        let result = PyDict::new(py);
        result.set_item("cycle", summary.cycle)?;
        result.set_item("plants", summary.plants)?;
        result.set_item("seeds", summary.seeds)?;
        result.set_item("born", summary.born)?;
        result.set_item("rounds", summary.rounds)?;
        let deaths = PyDict::new(py);
        for cause in 0..CAUSES.len() {
            deaths.set_item(CAUSES[cause], summary.deaths[cause])?;
        }
        result.set_item("deaths", deaths)?;
        Ok(result)
    }

    /// The fingerprint of the whole world (the same as world.py's)
    fn fingerprint(&mut self, py: Python<'_>) -> PyResult<String> {
        self.run(py, |world, comm| world.fingerprint(comm))
    }

    /// How many trees and seeds this rank has
    fn rows(&self) -> usize {
        self.world.forest.len()
    }

    /// This rank's seconds in each step of the cycle, so far
    fn timings<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let timings = PyDict::new(py);
        for step in 0..world::STEPS.len() {
            timings.set_item(world::STEPS[step], self.world.timings[step])?;
        }
        Ok(timings)
    }

    /// How much this rank has copied to others, so far
    fn traffic<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let traffic = PyDict::new(py);
        for kind in 0..world::TRAFFIC.len() {
            traffic.set_item(world::TRAFFIC[kind], self.world.traffic[kind])?;
        }
        Ok(traffic)
    }
}

/// How many cores the kernels (and RankWorlds made without their own
/// `threads`) use. Only works before they first run.
#[pyfunction]
fn set_threads(threads: usize) -> PyResult<()> {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build_global()
        .map_err(|problem| PyRuntimeError::new_err(problem.to_string()))
}

/// How many cores the kernels use
#[pyfunction]
fn threads() -> usize {
    rayon::current_num_threads()
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
    module.add_function(wrap_pyfunction!(set_threads, module)?)?;
    module.add_function(wrap_pyfunction!(threads, module)?)?;
    module.add_class::<RankWorld>()?;
    Ok(())
}
