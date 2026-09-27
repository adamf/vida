// The species and world settings, read from the Python SpeciesTable and
// WorldSettings (worldscale/species.py): one value per species for each
// setting.

use numpy::PyReadonlyArray1;
use pyo3::prelude::*;

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

pub struct Species {
    pub density_stem: Vec<f64>,
    pub density_leaf: Vec<f64>,
    pub density_seed: Vec<f64>,
    pub canopy_transmittance: Vec<f64>,
    pub fraction_minimum_survival: Vec<f64>,
    pub height_leaf_max: Vec<f64>,
    pub height_stem_max: Vec<f64>,
    pub youngs_modulus_stem: Vec<f64>,
    pub fraction_selfishness: Vec<f64>,
    pub start_making_seeds_age: Vec<f64>,
    pub reproduction_constant: Vec<f64>,
    pub reproduction_exponent: Vec<f64>,
    pub mass_seed_max: Vec<f64>,
    pub delay_in_germination: Vec<f64>,
    pub random_slow_growth: Vec<f64>,
    pub fraction_fail_germinate: Vec<f64>,
    pub photo_constant: Vec<f64>,
    pub photo_constant_shade: Vec<f64>,
    pub photo_exponent: Vec<f64>,
    pub fraction_carbon_to_seeds: Vec<f64>,
    pub fract_mass_seed_max_to_germ: Vec<f64>,
    pub fraction_seed_mass_to_plant: Vec<f64>,
    pub fraction_carbon_to_stem: Vec<f64>,
    pub constant1: Vec<f64>,
    pub exponent1: Vec<f64>,
    pub constant2: Vec<f64>,
    pub exponent2: Vec<f64>,
    pub constant3: Vec<f64>,
    pub exponent3: Vec<f64>,
    pub constant6: Vec<f64>,
    pub constant7: Vec<f64>,
    pub exponent7: Vec<f64>,
    pub constant8: Vec<f64>,
    pub constant20: Vec<f64>,
    pub exponent20: Vec<f64>,
    pub makes_seeds: Vec<bool>,
    pub leaf_is_hemisphere: Vec<bool>,
    pub formation_max: Vec<f64>,
    pub formation_min: Vec<f64>,
    pub dispersal_method: Vec<i64>,
    pub dispersal1: Vec<f64>,
    pub dispersal2: Vec<f64>,
    pub memory: Vec<i64>,
}

impl Species {
    pub fn read(table: &Bound<'_, PyAny>) -> PyResult<Species> {
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
}

pub struct World {
    pub world_size: f64,
    pub gravity: f64,
    pub light_intensity: f64,
    pub max_seeds_per_plant: i64,
    pub ignore_germ_death_at_start: bool,
    pub allow_random_death: bool,
    pub random_death_plant: f64,
    pub random_death_seed: f64,
    pub allow_slow_growth_death: bool,
    pub random_slow_growth: f64,
    pub allow_euler_greenhill_violations: bool,
    pub allow_off_world: bool,
}

impl World {
    pub fn read(world: &Bound<'_, PyAny>) -> PyResult<World> {
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
            allow_off_world: world.getattr("allowOffWorld")?.extract()?,
        })
    }
}
