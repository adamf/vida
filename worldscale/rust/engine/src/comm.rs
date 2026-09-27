// How the ranks running one world talk to each other (worldscale/comm.py's
// three ways, for the Rust world). They only ever talk all together:
//     alltoall(letters): letters[r] goes to rank r; gives back what each
//         rank sent to this one, in rank order
//     sum(numbers): each number added up over all ranks (wrapping round at
//         2**64, so fingerprints can be added up too)
//     max(number): the largest over all ranks
//
// Here are two ways: SerialComm (one rank) and ThreadComm (several ranks as
// threads of one program, for the tests and the worldscale command's
// -ranks). The Python module has one that talks through a Python comm (so
// ThreadComm and mpi4py work), and the worldscale command one over MPI.

use std::sync::{Arc, Barrier, Mutex};

/// Something that went wrong, from any part of a run
pub type Problem = Box<dyn std::error::Error + Send + Sync>;

pub trait Comm {
    fn rank(&self) -> usize;
    fn size(&self) -> usize;
    fn alltoall(&mut self, letters: Vec<Vec<u8>>) -> Result<Vec<Vec<u8>>, Problem>;
    fn sum(&mut self, numbers: &[u64]) -> Result<Vec<u64>, Problem>;
    fn max(&mut self, number: f64) -> Result<f64, Problem>;
}

/// One rank on its own
pub struct SerialComm;

impl Comm for SerialComm {
    fn rank(&self) -> usize {
        0
    }
    fn size(&self) -> usize {
        1
    }
    fn alltoall(&mut self, letters: Vec<Vec<u8>>) -> Result<Vec<Vec<u8>>, Problem> {
        Ok(letters)
    }
    fn sum(&mut self, numbers: &[u64]) -> Result<Vec<u64>, Problem> {
        Ok(numbers.to_vec())
    }
    fn max(&mut self, number: f64) -> Result<f64, Problem> {
        Ok(number)
    }
}

/// What the ranks of a ThreadComm share: a table everyone writes their
/// part of, and a barrier to wait for each other at
struct Meeting {
    barrier: Barrier,
    letters: Mutex<Vec<Vec<Vec<u8>>>>,
    numbers: Mutex<Vec<Vec<u64>>>,
    maxima: Mutex<Vec<f64>>,
}

/// Several ranks as threads of one program
pub struct ThreadComm {
    rank: usize,
    size: usize,
    meeting: Arc<Meeting>,
}

impl ThreadComm {
    /// One comm for each of `size` ranks
    pub fn group(size: usize) -> Vec<ThreadComm> {
        let meeting = Arc::new(Meeting {
            barrier: Barrier::new(size),
            letters: Mutex::new(vec![Vec::new(); size]),
            numbers: Mutex::new(vec![Vec::new(); size]),
            maxima: Mutex::new(vec![0.0; size]),
        });
        (0..size).map(|rank| ThreadComm { rank, size, meeting: Arc::clone(&meeting) }).collect()
    }
}

impl Comm for ThreadComm {
    fn rank(&self) -> usize {
        self.rank
    }

    fn size(&self) -> usize {
        self.size
    }

    fn alltoall(&mut self, letters: Vec<Vec<u8>>) -> Result<Vec<Vec<u8>>, Problem> {
        // everyone puts their letters on the table (a row for each sender),
        // then everyone takes the letters addressed to them
        self.meeting.letters.lock().unwrap()[self.rank] = letters;
        self.meeting.barrier.wait();
        let mut received = Vec::with_capacity(self.size);
        {
            let mut table = self.meeting.letters.lock().unwrap();
            for sender in 0..self.size {
                received.push(std::mem::take(&mut table[sender][self.rank]));
            }
        }
        self.meeting.barrier.wait();
        Ok(received)
    }

    fn sum(&mut self, numbers: &[u64]) -> Result<Vec<u64>, Problem> {
        self.meeting.numbers.lock().unwrap()[self.rank] = numbers.to_vec();
        self.meeting.barrier.wait();
        let mut totals = vec![0u64; numbers.len()];
        for theirs in self.meeting.numbers.lock().unwrap().iter() {
            for place in 0..totals.len() {
                totals[place] = totals[place].wrapping_add(theirs[place]);
            }
        }
        self.meeting.barrier.wait();
        Ok(totals)
    }

    fn max(&mut self, number: f64) -> Result<f64, Problem> {
        self.meeting.maxima.lock().unwrap()[self.rank] = number;
        self.meeting.barrier.wait();
        let largest = self.meeting.maxima.lock().unwrap().iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
        self.meeting.barrier.wait();
        Ok(largest)
    }
}
