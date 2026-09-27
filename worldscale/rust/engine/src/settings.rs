// The species and world settings: one value per species for each species
// setting (as worldscale/species.py's SpeciesTable has them), and the few
// world settings the model uses (species.py's WorldSettings).
//
// The Python module fills these in from the Python objects; the worldscale
// command reads them from Vida's own files here, the same way species.py
// does: Vida_Data/Default_species.yml, overridden by each species file, and
// "Vida World Preferences.yml".

use std::fs;
use std::path::Path;

use serde_yaml::Value;

#[derive(Clone)]
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

#[derive(Clone)]
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
    pub allow_overlaps: bool,
    pub allow_off_world: bool,
}

// ---------------------------------------------------------------------
// Reading Vida's files
// ---------------------------------------------------------------------

fn read_yaml(file: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(file).map_err(|problem| format!("can't read {}: {}", file.display(), problem))?;
    serde_yaml::from_str(&text).map_err(|problem| format!("can't read {}: {}", file.display(), problem))
}

/// A setting from the first of these files that has it (a species file,
/// then the default species)
fn setting<'a>(files: &[&'a Value], name: &str) -> Result<&'a Value, String> {
    for file in files {
        if let Some(value) = file.get(name) {
            return Ok(value);
        }
    }
    Err(format!("no setting called {}", name))
}

/// A number, as Python's float() makes one
fn number(value: &Value, name: &str) -> Result<f64, String> {
    match value {
        Value::Number(number) => number.as_f64().ok_or_else(|| format!("{} isn't a number", name)),
        Value::Bool(true_or_false) => Ok(if *true_or_false { 1.0 } else { 0.0 }),
        Value::String(text) => text.trim().parse().map_err(|_| format!("{} isn't a number: {}", name, text)),
        _ => Err(format!("{} isn't a number", name)),
    }
}

/// True or false, as Python's bool() makes one
fn true_false(value: &Value, name: &str) -> Result<bool, String> {
    match value {
        Value::Bool(true_or_false) => Ok(*true_or_false),
        Value::Number(_) => Ok(number(value, name)? != 0.0),
        Value::String(text) => Ok(!text.is_empty()),
        Value::Null => Ok(false),
        _ => Err(format!("{} isn't true or false", name)),
    }
}

/// A list setting's values, as numbers
fn numbers(value: &Value, name: &str) -> Result<Vec<f64>, String> {
    match value {
        Value::Sequence(items) => items.iter().map(|item| number(item, name)).collect(),
        _ => Err(format!("{} isn't a list", name)),
    }
}

/// The .yml species files directly in a folder, in sorted order
pub fn species_files_in(folder: &Path) -> Result<Vec<std::path::PathBuf>, String> {
    let mut files = Vec::new();
    let listing = fs::read_dir(folder).map_err(|problem| format!("can't read {}: {}", folder.display(), problem))?;
    for entry in listing {
        let path = entry.map_err(|problem| problem.to_string())?.path();
        if path.extension().is_some_and(|extension| extension == "yml") && path.is_file() {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

impl Species {
    /// The species in these files, over Vida_Data/Default_species.yml in
    /// the Vida folder (as species.SpeciesTable reads them)
    pub fn from_files(files: &[std::path::PathBuf], vida_folder: &Path) -> Result<Species, String> {
        let defaults = read_yaml(&vida_folder.join("Vida_Data").join("Default_species.yml"))?;
        let mut species = Species::empty();
        for file in files {
            let own = read_yaml(file)?;
            let both = [&own, &defaults];
            species.add(&both).map_err(|problem| format!("{}: {}", file.display(), problem))?;
        }
        if species.density_stem.is_empty() {
            return Err("no species files".to_string());
        }
        Ok(species)
    }

    fn empty() -> Species {
        Species {
            density_stem: Vec::new(),
            density_leaf: Vec::new(),
            density_seed: Vec::new(),
            canopy_transmittance: Vec::new(),
            fraction_minimum_survival: Vec::new(),
            height_leaf_max: Vec::new(),
            height_stem_max: Vec::new(),
            youngs_modulus_stem: Vec::new(),
            fraction_selfishness: Vec::new(),
            start_making_seeds_age: Vec::new(),
            reproduction_constant: Vec::new(),
            reproduction_exponent: Vec::new(),
            mass_seed_max: Vec::new(),
            delay_in_germination: Vec::new(),
            random_slow_growth: Vec::new(),
            fraction_fail_germinate: Vec::new(),
            photo_constant: Vec::new(),
            photo_constant_shade: Vec::new(),
            photo_exponent: Vec::new(),
            fraction_carbon_to_seeds: Vec::new(),
            fract_mass_seed_max_to_germ: Vec::new(),
            fraction_seed_mass_to_plant: Vec::new(),
            fraction_carbon_to_stem: Vec::new(),
            constant1: Vec::new(),
            exponent1: Vec::new(),
            constant2: Vec::new(),
            exponent2: Vec::new(),
            constant3: Vec::new(),
            exponent3: Vec::new(),
            constant6: Vec::new(),
            constant7: Vec::new(),
            exponent7: Vec::new(),
            constant8: Vec::new(),
            constant20: Vec::new(),
            exponent20: Vec::new(),
            makes_seeds: Vec::new(),
            leaf_is_hemisphere: Vec::new(),
            formation_max: Vec::new(),
            formation_min: Vec::new(),
            dispersal_method: Vec::new(),
            dispersal1: Vec::new(),
            dispersal2: Vec::new(),
            memory: Vec::new(),
        }
    }

    /// Add one species, from its settings
    fn add(&mut self, files: &[&Value]) -> Result<(), String> {
        let get = |name: &str| -> Result<f64, String> { number(setting(files, name)?, name) };
        self.density_stem.push(get("densityStem")?);
        self.density_leaf.push(get("densityLeaf")?);
        self.density_seed.push(get("densitySeed")?);
        self.canopy_transmittance.push(get("canopyTransmittance")?);
        self.fraction_minimum_survival.push(get("fractionMinimumSurvival")?);
        self.height_leaf_max.push(get("heightLeafMax")?);
        self.height_stem_max.push(get("heightStemMax")?);
        self.youngs_modulus_stem.push(get("youngsModulusStem")?);
        self.fraction_selfishness.push(get("fractionSelfishness")?);
        self.start_making_seeds_age.push(get("startMakingSeedsAge")?);
        self.reproduction_constant.push(get("reproductionConstant")?);
        self.reproduction_exponent.push(get("reproductionExponent")?);
        self.mass_seed_max.push(get("massSeedMax")?);
        self.delay_in_germination.push(get("delayInGermination")?);
        self.random_slow_growth.push(get("randomSlowGrowth")?);
        self.fraction_fail_germinate.push(get("fractionFailGerminate")?);
        let photo_constant = get("photoConstant")?;
        self.photo_constant.push(photo_constant);
        // Vida sets photoConstantShade to photoConstant for every plant
        self.photo_constant_shade.push(photo_constant);
        self.photo_exponent.push(get("photoExponent")?);
        self.fraction_carbon_to_seeds.push(get("fractionCarbonToSeeds")?);
        self.fract_mass_seed_max_to_germ.push(get("fractMassSeedMaxToGerm")?);
        self.fraction_seed_mass_to_plant.push(get("fractionSeedMassToPlant")?);
        self.fraction_carbon_to_stem.push(get("fractionCarbonToStem")?);
        self.constant1.push(get("speciesConstant1")?);
        self.exponent1.push(get("speciesExponent1")?);
        self.constant2.push(get("speciesConstant2")?);
        self.exponent2.push(get("speciesExponent2")?);
        self.constant3.push(get("speciesConstant3")?);
        self.exponent3.push(get("speciesExponent3")?);
        self.constant6.push(get("speciesConstant6")?);
        self.constant7.push(get("speciesConstant7")?);
        self.exponent7.push(get("speciesExponent7")?);
        self.constant8.push(get("speciesConstant8")?);
        self.constant20.push(get("speciesConstant20")?);
        self.exponent20.push(get("speciesExponent20")?);
        self.makes_seeds.push(true_false(setting(files, "makeSeeds")?, "makeSeeds")?);
        self.leaf_is_hemisphere.push(true_false(setting(files, "leafIsHemisphere")?, "leafIsHemisphere")?);
        // where on the canopy seeds form, and how they're dispersed
        let formation = numbers(setting(files, "locSeedFormation")?, "locSeedFormation")?;
        if formation.len() < 2 {
            return Err("locSeedFormation needs two numbers".to_string());
        }
        self.formation_max.push(formation[0].clamp(0.0, 1.0));
        self.formation_min.push(formation[1].clamp(0.0, 1.0));
        let mut dispersal = numbers(setting(files, "seedDispersalMethod")?, "seedDispersalMethod")?;
        dispersal.extend_from_slice(&[0.0, 0.0]);
        self.dispersal_method.push(dispersal[0] as i64);
        self.dispersal1.push(dispersal[1]);
        self.dispersal2.push(dispersal[2]);
        self.memory.push((get("numYearsGrowthMemory")? as i64).max(1));
        Ok(())
    }

    pub fn count(&self) -> usize {
        self.density_stem.len()
    }
}

impl World {
    /// "Vida World Preferences.yml" in the Vida folder, for a world this wide
    pub fn from_file(vida_folder: &Path, world_size: f64) -> Result<World, String> {
        let preferences = read_yaml(&vida_folder.join("Vida World Preferences.yml"))?;
        let files = [&preferences];
        let get = |name: &str| -> Result<f64, String> { number(setting(&files, name)?, name) };
        let yes = |name: &str| -> Result<bool, String> { true_false(setting(&files, name)?, name) };
        Ok(World {
            world_size,
            gravity: get("gravity")?,
            light_intensity: get("lightIntensity")?,
            max_seeds_per_plant: get("maxSeedsPerPlant")? as i64,
            ignore_germ_death_at_start: yes("ignoreGermDeathAtStart")?,
            allow_random_death: yes("allowRandomDeath")?,
            random_death_plant: get("randomDeathPlant")?,
            random_death_seed: get("randomDeathSeed")?,
            allow_slow_growth_death: yes("allowSlowGrowthDeath")?,
            random_slow_growth: get("randomSlowGrowth")?,
            allow_euler_greenhill_violations: yes("allowEulerGreenhillViolations")?,
            allow_overlaps: yes("allowOverlaps")?,
            allow_off_world: yes("allowOffWorld")?,
        })
    }
}
