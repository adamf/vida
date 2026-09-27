// worldscale: Vida's world-scale prototype, all in Rust.
//
// The same run as `python -m worldscale.run -engine rust-world`, without
// Python: it reads Vida's own species files and world preferences, and
// prints the same counts and the same fingerprint.
//
//     worldscale -w 800 -t 40                      one process, all its cores
//     worldscale -w 800 -t 40 -ranks 4            4 ranks, as threads
//     mpiexec -n 4 worldscale -w 800 -t 40        4 ranks over MPI (built
//                                                 with --features mpi)
//
// Run it from the folder with Vida.py in it (or give -vida).

use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::Instant;

use worldscale_engine::comm::{Comm, Problem, SerialComm, ThreadComm};
use worldscale_engine::growth::CAUSES;
use worldscale_engine::settings::{self, Species, World};
use worldscale_engine::world::{self, Partition, RankWorld, Summary};

#[derive(Clone)]
struct Options {
    world_size: f64,
    tile_size: f64,
    seeds_per_hectare: f64,
    cycles: u32,
    rng_start: u64,
    photon_limit: i64,
    partition: Partition,
    shuffle: bool,
    threads: usize,
    ranks: usize,
    vida: PathBuf,
    species: PathBuf,
    json: Option<PathBuf>,
    quiet: bool,
}

const HELP: &str = "worldscale: Vida's world-scale prototype, all in Rust

options:
  -w METRES          width of the (square) world (200)
  -tile METRES       width of a tile (50)
  -s NUMBER          starting seeds per hectare (400)
  -t NUMBER          how many cycles to run (30)
  -rngstart NUMBER   starting value for the random numbers (1)
  -photons NUMBER    most photons per plant (750, as Vida)
  -partition HOW     how tiles are shared out: strips, curve or scattered (strips)
  -shuffle           shuffle each rank's trees every cycle (it shouldn't matter)
  -threads NUMBER    cores each process uses (0: all of them)
  -ranks NUMBER      run this many ranks as threads of one process (1)
  -vida FOLDER       the folder with Vida.py in it (.)
  -species FOLDER    folder of species files (Species, in the Vida folder)
  -json FILE         also write the results to this file
  -quiet             only print the end";

fn parse<T: std::str::FromStr>(name: &str, value: &str) -> Result<T, String> {
    value.parse().map_err(|_| format!("{} can't be {}", name, value))
}

fn read_options() -> Result<Options, String> {
    let mut options = Options {
        world_size: 200.0,
        tile_size: 50.0,
        seeds_per_hectare: 400.0,
        cycles: 30,
        rng_start: 1,
        photon_limit: 750,
        partition: Partition::Strips,
        shuffle: false,
        threads: 0,
        ranks: 1,
        vida: PathBuf::from("."),
        species: PathBuf::from("Species"),
        json: None,
        quiet: false,
    };
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let mut place = 0;
    while place < arguments.len() {
        let name = arguments[place].as_str();
        if name == "-shuffle" || name == "-quiet" || name == "-h" || name == "-help" || name == "--help" {
            match name {
                "-shuffle" => options.shuffle = true,
                "-quiet" => options.quiet = true,
                _ => {
                    println!("{}", HELP);
                    std::process::exit(0);
                }
            }
            place += 1;
            continue;
        }
        let value = arguments.get(place + 1).ok_or(format!("{} needs a value", name))?.clone();
        match name {
            "-w" => options.world_size = parse(name, &value)?,
            "-tile" => options.tile_size = parse(name, &value)?,
            "-s" => options.seeds_per_hectare = parse(name, &value)?,
            "-t" => options.cycles = parse(name, &value)?,
            // (negative numbers wrap round, as they do in philox.py)
            "-rngstart" => options.rng_start = parse::<i128>(name, &value)? as u64,
            "-photons" => options.photon_limit = parse(name, &value)?,
            "-partition" => options.partition = Partition::from_name(&value)?,
            "-threads" => options.threads = parse(name, &value)?,
            "-ranks" => options.ranks = parse(name, &value)?,
            "-vida" => options.vida = PathBuf::from(value),
            "-species" => options.species = PathBuf::from(value),
            "-json" => options.json = Some(PathBuf::from(value)),
            _ => return Err(format!("no option called {} (-help lists them)", name)),
        }
        place += 2;
    }
    if options.ranks < 1 {
        return Err("-ranks must be at least 1".to_string());
    }
    Ok(options)
}

/// What a run gives back (the same as run.py's runWorld)
struct Results {
    cycles: Vec<(Summary, f64)>,
    fingerprint: String,
    seconds: f64,
    timings: Vec<f64>,
    largest_rank: u64,
    traffic: Vec<u64>,
}

/// Run one world on this rank
fn run_rank(comm: &mut dyn Comm, options: &Options, species: Species, world: World) -> Result<Results, Problem> {
    let settings = world::Settings {
        world_size: options.world_size,
        tile_size: options.tile_size,
        seeds_per_hectare: options.seeds_per_hectare,
        rng_start: options.rng_start,
        photon_limit: options.photon_limit,
        partition: options.partition,
        shuffle: options.shuffle,
    };
    let mut the_world = RankWorld::new(comm.rank(), comm.size(), settings, species, world)?;
    let mut cycles = Vec::new();
    let started = Instant::now();
    for _ in 0..options.cycles {
        let cycle_started = Instant::now();
        let summary = the_world.run_cycle(comm)?;
        let seconds = comm.max(cycle_started.elapsed().as_secs_f64())?;
        if comm.rank() == 0 && !options.quiet {
            println!("cycle {:3}  plants {:9}  seeds {:9}  born {:8}  crushed {:7}  rounds {:2}  {:6.2} s",
                     summary.cycle, summary.plants, summary.seeds, summary.born, summary.deaths[7], summary.rounds,
                     seconds);
        }
        cycles.push((summary, seconds));
    }
    let seconds = comm.max(started.elapsed().as_secs_f64())?;
    let mut timings = Vec::new();
    for step in 0..world::STEPS.len() {
        timings.push(comm.max(the_world.timings[step])?);
    }
    let largest_rank = comm.max(the_world.forest.len() as f64)? as u64;
    let traffic = comm.sum(&the_world.traffic)?;
    let fingerprint = the_world.fingerprint(comm)?;
    Ok(Results { cycles, fingerprint, seconds, timings, largest_rank, traffic })
}

/// The results as JSON, like run.py's -json
fn as_json(results: &Results, options: &Options, ranks: usize) -> String {
    let mut text = String::from("{\n \"cycles\": [\n");
    for (place, (summary, seconds)) in results.cycles.iter().enumerate() {
        let mut deaths = Vec::new();
        for (cause, count) in CAUSES.iter().zip(summary.deaths) {
            deaths.push(format!("\"{}\": {}", cause, count));
        }
        let _ = writeln!(text, "  {{\"cycle\": {}, \"plants\": {}, \"seeds\": {}, \"born\": {}, \"rounds\": {}, \
                              \"deaths\": {{{}}}, \"seconds\": {}}}{}",
                       summary.cycle, summary.plants, summary.seeds, summary.born, summary.rounds, deaths.join(", "),
                       seconds, if place + 1 < results.cycles.len() { "," } else { "" });
    }
    let mut timings = Vec::new();
    for step in 0..world::STEPS.len() {
        timings.push(format!("\"{}\": {}", world::STEPS[step], results.timings[step]));
    }
    let mut traffic = Vec::new();
    for kind in 0..world::TRAFFIC.len() {
        traffic.push(format!("\"{}\": {}", world::TRAFFIC[kind], results.traffic[kind]));
    }
    let _ = write!(text, " ],\n \"fingerprint\": \"{}\",\n \"seconds\": {},\n \"timings\": {{{}}},\n \"ranks\": {},\n \
                          \"threads\": {},\n \"largestRank\": {},\n \"traffic\": {{{}}},\n \"options\": {{\"worldSize\": {}, \
                          \"tileSize\": {}, \"seedsPerHectare\": {}, \"cycles\": {}, \"rngStart\": {}, \"photonLimit\": {}, \
                          \"partition\": \"{}\", \"shuffle\": {}, \"engine\": \"worldscale\"}}\n}}\n",
                   results.fingerprint, results.seconds, timings.join(", "), ranks, rayon::current_num_threads(),
                   results.largest_rank, traffic.join(", "), options.world_size, options.tile_size,
                   options.seeds_per_hectare, options.cycles, options.rng_start, options.photon_limit,
                   options.partition.name(), options.shuffle);
    text
}

fn report(results: &Results, options: &Options, ranks: usize, how: &str) -> Result<(), Problem> {
    println!("ranks {} ({}), {} threads each, {} tiles: {:.1} s", ranks, how, rayon::current_num_threads(),
             options.partition.name(), results.seconds);
    let mut steps = Vec::new();
    for step in 0..world::STEPS.len() {
        steps.push(format!("{} {:.1} s", world::STEPS[step], results.timings[step]));
    }
    println!("  slowest rank's time in each step: {}", steps.join(", "));
    let mut copies = Vec::new();
    for kind in 0..world::TRAFFIC.len() {
        copies.push(format!("{} {}", world::TRAFFIC[kind], results.traffic[kind]));
    }
    println!("  over all the cycles: {}", copies.join(", "));
    println!("fingerprint {}", results.fingerprint);
    if let Some(file) = &options.json {
        std::fs::write(file, as_json(results, options, ranks))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------
// MPI
// ---------------------------------------------------------------------

#[cfg(feature = "mpi")]
mod over_mpi {
    use mpi::collective::SystemOperation;
    use mpi::datatype::{Partition, PartitionMut};
    use mpi::topology::SimpleCommunicator;
    use mpi::traits::*;

    use worldscale_engine::comm::{Comm, Problem};

    /// Ranks as separate processes, over MPI. Only the main thread talks to
    /// MPI; the other threads only do sums.
    pub struct MpiComm {
        pub world: SimpleCommunicator,
    }

    /// Where each part starts, when parts of these sizes are put end to end
    fn starts(counts: &[i32]) -> Vec<i32> {
        let mut starts = Vec::with_capacity(counts.len());
        let mut total = 0;
        for &count in counts {
            starts.push(total);
            total += count;
        }
        starts
    }

    impl Comm for MpiComm {
        fn rank(&self) -> usize {
            self.world.rank() as usize
        }

        fn size(&self) -> usize {
            self.world.size() as usize
        }

        fn alltoall(&mut self, letters: Vec<Vec<u8>>) -> Result<Vec<Vec<u8>>, Problem> {
            let mut sizes = Vec::with_capacity(letters.len());
            for letter in &letters {
                sizes.push(i32::try_from(letter.len()).map_err(|_| "a letter is over 2 GB")?);
            }
            // first how big each letter is, then the letters
            let mut coming = vec![0i32; letters.len()];
            self.world.all_to_all_into(&sizes[..], &mut coming[..]);
            let sending: Vec<u8> = letters.concat();
            let mut arriving = vec![0u8; coming.iter().map(|&size| size as usize).sum()];
            {
                let send_starts = starts(&sizes);
                let receive_starts = starts(&coming);
                let out = Partition::new(&sending[..], &sizes[..], &send_starts[..]);
                let mut into = PartitionMut::new(&mut arriving[..], &coming[..], &receive_starts[..]);
                self.world.all_to_all_varcount_into(&out, &mut into);
            }
            let mut received = Vec::with_capacity(coming.len());
            let mut at = 0;
            for &size in &coming {
                received.push(arriving[at..at + size as usize].to_vec());
                at += size as usize;
            }
            Ok(received)
        }

        fn sum(&mut self, numbers: &[u64]) -> Result<Vec<u64>, Problem> {
            let mut totals = vec![0u64; numbers.len()];
            self.world.all_reduce_into(numbers, &mut totals[..], SystemOperation::sum());
            Ok(totals)
        }

        fn max(&mut self, number: f64) -> Result<f64, Problem> {
            let mut largest = 0.0f64;
            self.world.all_reduce_into(&number, &mut largest, SystemOperation::max());
            Ok(largest)
        }
    }
}

fn run() -> Result<(), Problem> {
    let options = read_options()?;
    if options.threads > 0 {
        rayon::ThreadPoolBuilder::new().num_threads(options.threads).build_global()?;
    }
    let species_folder = if options.species.is_absolute() { options.species.clone() } else { options.vida.join(&options.species) };
    let species = Species::from_files(&settings::species_files_in(&species_folder)?, &options.vida)?;
    let world = World::from_file(&options.vida, options.world_size)?;

    #[cfg(feature = "mpi")]
    {
        let (universe, _) = mpi::initialize_with_threading(mpi::Threading::Funneled).ok_or("MPI didn't start")?;
        let mpi_world = mpi::traits::Communicator::duplicate(&universe.world());
        let size = mpi::traits::Communicator::size(&mpi_world) as usize;
        if size > 1 {
            if options.ranks > 1 {
                return Err("use -ranks or MPI, not both".into());
            }
            let mut comm = over_mpi::MpiComm { world: mpi_world };
            let results = run_rank(&mut comm, &options, species, world)?;
            if comm.rank() == 0 {
                report(&results, &options, size, "MPI")?;
            }
            return Ok(());
        }
    }

    if options.ranks == 1 {
        let results = run_rank(&mut SerialComm, &options, species, world)?;
        return report(&results, &options, 1, "one process");
    }
    // several ranks, as threads, sharing the cores
    let mut threads = Vec::new();
    for mut comm in ThreadComm::group(options.ranks) {
        let (options, species, world) = (options.clone(), species.clone(), world.clone());
        threads.push(std::thread::spawn(move || {
            let results = run_rank(&mut comm, &options, species, world);
            (comm.rank(), results.map_err(|problem| problem.to_string()))
        }));
    }
    for thread in threads {
        let (rank, results) = thread.join().map_err(|_| "a rank's thread failed")?;
        if rank == 0 {
            report(&results?, &options, options.ranks, "threads")?;
        }
    }
    Ok(())
}

fn main() {
    if let Err(problem) = run() {
        eprintln!("worldscale: {}", problem);
        std::process::exit(1);
    }
}
