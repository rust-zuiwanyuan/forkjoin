// Copyright (c) 2015-2016 Linus Färnstrand.
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or http://opensource.org/licenses/MIT>,
// at your option. All files in the project carrying such
// notice may not be copied, modified, or distributed except
// according to those terms.

//! # ForkJoin
//! A work stealing fork-join parallelism library.
//!
//! [![Build Status](https://api.travis-ci.org/faern/forkjoin.svg?branch=master)](https://travis-ci.org/faern/forkjoin)
//!
//! Inspired by the blog post [Data Parallelism in Rust](http://smallcultfollowing.com/babysteps/blog/2013/06/11/data-parallelism-in-rust/)
//! and implemented as part of a master's thesis. Repository hosted at [github.com/faern/forkjoin](https://github.com/faern/forkjoin)
//!
//! Library documentation hosted [here](https://faern.github.io/rust-docs/forkjoin/forkjoin/)
//!
//! This library has been developed to accommodate the needs of three types of
//! algorithms that all fit very well for fork-join parallelism.
//!
//! # Reduce style
//!
//! Reduce style is where the algorithm receive an argument, recursively compute a value
//! from this argument and return one answer. Examples of this style include recursively
//! finding the n:th Fibonacci number and summing of tree structures.
//! Characteristics of this style is that the algorithm does not need to mutate its
//! argument and the resulting value is only available after every subtask has been
//! fully computed.
//!
//! In reduce style algorithms the return values of each subtask is passed to a special
//! join function that is executed when all subtasks have completed.
//! To this join function an extra argument can be sent directly from the task if the algorithm
//! has `ReduceStyle::Arg`. This can be seen in the examples here.
//!
//! ## Example of reduce style (`ReduceStyle::NoArg`)
//!
//! ```rust
//! use forkjoin::{TaskResult,ForkPool,AlgoStyle,ReduceStyle,Algorithm};
//!
//! fn fib_30_with_4_threads() {
//!     let forkpool = ForkPool::with_threads(4);
//!     let fibpool = forkpool.init_algorithm(Algorithm {
//!         fun: fib_task,
//!         style: AlgoStyle::Reduce(ReduceStyle::NoArg(fib_join)),
//!     });
//!
//!     let job = fibpool.schedule(30);
//!     let result: usize = job.recv().unwrap();
//!     assert_eq!(1346269, result);
//! }
//!
//! fn fib_task(n: usize) -> TaskResult<usize, usize> {
//!     if n < 2 {
//!         TaskResult::Done(1)
//!     } else {
//!         TaskResult::Fork(vec![n-1,n-2], None)
//!     }
//! }
//!
//! fn fib_join(values: &[usize]) -> usize {
//!     values.iter().fold(0, |acc, &v| acc + v)
//! }
//! ```
//!
//! ## Example of reduce style (`ReduceStyle::Arg`)
//!
//! ```rust
//! use forkjoin::{TaskResult,ForkPool,AlgoStyle,ReduceStyle,Algorithm};
//!
//! struct Tree {
//!     value: usize,
//!     children: Vec<Tree>,
//! }
//!
//! fn sum_tree(t: &Tree) -> usize {
//!     let forkpool = ForkPool::new();
//!     let sumpool = forkpool.init_algorithm(Algorithm {
//!         fun: sum_tree_task,
//!         style: AlgoStyle::Reduce(ReduceStyle::Arg(sum_tree_join)),
//!     });
//!     let job = sumpool.schedule(t);
//!     job.recv().unwrap()
//! }
//!
//! fn sum_tree_task(t: &Tree) -> TaskResult<&Tree, usize> {
//!     if t.children.is_empty() {
//!         TaskResult::Done(t.value)
//!     } else {
//!         let mut fork_args: Vec<&Tree> = vec![];
//!         for c in t.children.iter() {
//!             fork_args.push(c);
//!         }
//!         TaskResult::Fork(fork_args, Some(t.value)) // Pass current nodes value to join
//!     }
//! }
//!
//! fn sum_tree_seq(t: &Tree) -> usize {
//!     t.value + t.children.iter().fold(0, |acc, t2| acc + sum_tree_seq(t2))
//! }
//!
//! fn sum_tree_join(value: &usize, values: &[usize]) -> usize {
//!     *value + values.iter().fold(0, |acc, &v| acc + v)
//! }
//! ```
//!
//! # Search style
//!
//! Search style return results continuously and can sometimes start without any
//! argument, or start with some initial state. The algorithm produce one or multiple
//! output values during the execution, possibly aborting anywhere in the middle.
//! Algorithms where leafs in the problem tree represent a complete solution to the
//! problem (unless the leaf represent a dead end that is not a solution and does
//! not spawn any subtasks), for example nqueens and sudoku solvers, have this style.
//! Characteristics of the search style is that they can produce multiple results
//! and can abort before all tasks in the tree have been computed.
//!
//! ## Example of search style
//!
//! ```rust
//! use forkjoin::{ForkPool,TaskResult,AlgoStyle,Algorithm};
//!
//! type Queen = usize;
//! type Board = Vec<Queen>;
//! type Solutions = Vec<Board>;
//!
//! fn search_nqueens() {
//!     let n: usize = 8;
//!     let empty = vec![];
//!
//!     let forkpool = ForkPool::with_threads(4);
//!     let queenpool = forkpool.init_algorithm(Algorithm {
//!         fun: nqueens_task,
//!         style: AlgoStyle::Search,
//!     });
//!
//!     let job = queenpool.schedule((empty, n));
//!
//!     let mut solutions: Vec<Board> = vec![];
//!     loop {
//!         match job.recv() {
//!             Err(..) => break, // Job has completed
//!             Ok(board) => solutions.push(board),
//!         };
//!     }
//!     let num_solutions = solutions.len();
//!     println!("Found {} solutions to nqueens({}x{})", num_solutions, n, n);
//! }
//!
//! fn nqueens_task((q, n): (Board, usize)) -> TaskResult<(Board,usize), Board> {
//!     if q.len() == n {
//!         TaskResult::Done(q)
//!     } else {
//!         let mut fork_args: Vec<(Board, usize)> = vec![];
//!         for i in 0..n {
//!             let mut q2 = q.clone();
//!             q2.push(i);
//!
//!             if ok(&q2[..]) {
//!                 fork_args.push((q2, n));
//!             }
//!         }
//!         TaskResult::Fork(fork_args, None)
//!     }
//! }
//!
//! fn ok(q: &[usize]) -> bool {
//!     for (x1, &y1) in q.iter().enumerate() {
//!         for (x2, &y2) in q.iter().enumerate() {
//!             if x2 > x1 {
//!                 let xd = x2-x1;
//!                 if y1 == y2 || y1 == y2 + xd || (y2 >= xd && y1 == y2 - xd) {
//!                     return false;
//!                 }
//!             }
//!         }
//!     }
//!     true
//! }
//! ```
//!
//! # In-place mutation style
//!
//! NOTE: This style works in the current lib version, but it requires very ugly
//! unsafe code!
//!
//! In-place mutation style receive a mutable argument, recursively modifies this value
//! and the result is the argument itself. Sorting algorithms that sort their input
//! arrays are cases of this style. Characteristics of this style is that they mutate
//! their input argument instead of producing any output.
//!
//! Examples of this will come when they can be nicely implemented.
//!
//! # Tasks
//!
//! The small units that are executed and can choose to fork or to return a value is the
//! `TaskFun`. A TaskFun can NEVER block, because that would block the kernel thread
//! it's being executed on. Instead it should decide if it's done calculating or need
//! to fork. This decision is taken in the return value to indicate to the user
//! that a TaskFun need to return before anything can happen.
//!
//! A TaskFun return a `TaskResult`. It can be `TaskResult::Done(value)` if it's done
//! calculating. It can be `TaskResult::Fork(args)` if it needs to fork.
//!
//! # TODO
//!
//! - [ ] Make mutation style algorithms work without giving join function
//! - [ ] Implement a sorting algorithm. Quicksort?
//! - [ ] Remove need to return None on fork with NoArg
//! - [ ] Make it possible to use algorithms with different Arg & Ret on same pool.
//! - [ ] Make ForkJoin work in stable Rust.
//! - [ ] Remove mutex around channel in search style.
//!
//! # License
//!
//! Licensed under either of
//!  * Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
//!  * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)
//! at your option.
//!
//! ## Contribution
//!
//! Unless you explicitly state otherwise, any contribution intentionally submitted
//! for inclusion in the work by you shall be dual licensed as above, without any
//! additional terms or conditions.
//!


#![feature(unique)]


extern crate deque;
extern crate rand;
extern crate num_cpus;
extern crate thread_scoped;
extern crate libc;

use std::ptr::Unique;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc,Mutex};
use std::sync::mpsc::{channel,Sender,Receiver,TryRecvError};
use std::fmt;

mod workerthread;
mod poolsupervisor;

use ::poolsupervisor::{PoolSupervisorThread,SupervisorMsg};

/// Type definition of the main function in a task.
/// Your task functions must have this signature
pub type TaskFun<Arg, Ret> = extern "Rust" fn(Arg) -> TaskResult<Arg, Ret>;

/// Type definition of functions joining together forked results.
/// Only used in `AlgoStyle::Reduce` algorithms with `ReduceStyle::NoArg`.
pub type TaskJoin<Ret> = extern "Rust" fn(&[Ret]) -> Ret;

/// Similar to `TaskJoin` but takes an extra argument sent directly
/// from the task in algorithms with `ReduceStyle::Arg`.
pub type TaskJoinArg<Ret> = extern "Rust" fn(&Ret, &[Ret]) -> Ret;

/// Internal representation of a task.
pub struct Task<Arg: Send, Ret: Send + Sync> {
    pub algo: Algorithm<Arg, Ret>,
    pub arg: Arg,
    pub join: ResultReceiver<Ret>,
}

/// Return values from tasks. Represent a computed value or a fork of the algorithm.
pub enum TaskResult<Arg, Ret> {
    /// Return this from `TaskFun` to indicate a computed value. Represents a leaf in the
    /// problem tree of the computation.
    ///
    /// If the algorithm style is `AlgoStyle::Search` the value in `Done` will be sent
    /// directly to the `Job` held by the submitter of the computation.
    /// If the algorithm style is `AlgoStyle::Reduce` the value in `Done` will be inserted
    /// into a join barrier and later passed to the algorithms join function when all
    /// subtasks have completed execution.
    Done(Ret),

    /// Return this from `TaskFun` to indicate that the algorithm wants to fork.
    /// Takes a list of arguments to fork on. One subtask will be created for each argument.
    /// The second value is only used by `ReduceStyle::Arg`-algorithms to send a value directly
    /// to the `TaskJoinArg`, passing None in such algorithms will yield a panic.
    Fork(Vec<Arg>, Option<Ret>),
}

/// Enum representing the style of the executed algorithm.
pub enum AlgoStyle<Ret> {
    /// A `Reduce` style algorithm join together the results of the individual nodes
    /// in the problem tree to finally form one result for the entire computation.
    ///
    /// Examples of this style include recursively computing fibbonacci numbers
    /// and summing binary trees.
    ///
    /// Takes a `ReduceStyle` to indicate what type of join function to use.
    Reduce(ReduceStyle<Ret>),

    /// A `Search` style algoritm return their results to the listener directly upon a
    /// `TaskResult::Done`.
    ///
    /// Examples of this style include sudoku solvers and nqueens where a node can
    /// represent a complete solution.
    Search,
}
impl<Ret> Copy for AlgoStyle<Ret> {}
impl<Ret> Clone for AlgoStyle<Ret> { fn clone(&self) -> AlgoStyle<Ret> { *self } }

/// Enum indicating what type of join function an `Algorithm` will use.
pub enum ReduceStyle<Ret> {
    /// No extra argument is passed to the join function, only the resulting values of all subtasks
    NoArg(TaskJoin<Ret>),

    /// Together with the result values of the subtasks, the join function will also
    /// be passed an argument sent directly from the `TaskFun`.
    Arg(TaskJoinArg<Ret>),
}
impl<Ret> Copy for ReduceStyle<Ret> {}
impl<Ret> Clone for ReduceStyle<Ret> { fn clone(&self) -> ReduceStyle<Ret> { *self } }

/// The representation of a specific algorithm to use the ForkJoin library.
///
/// Create one instance of this struct for each algorithm to be executed in ForkJoin.
pub struct Algorithm<Arg: Send, Ret: Send + Sync> {
    /// A function pointer. The function that will be executed by all the tasks and subtasks.
    pub fun: TaskFun<Arg, Ret>,

    /// Enum showing the type of algorithm, indicate what should be done with results from
    /// subtasks created by forks of this algorithm.
    pub style: AlgoStyle<Ret>,
}
impl<Arg: Send, Ret: Send + Sync> Copy for Algorithm<Arg,Ret> {}
impl<Arg: Send, Ret: Send + Sync> Clone for Algorithm<Arg,Ret> {
    fn clone(&self) -> Algorithm<Arg,Ret> { *self }
}

/// Internal struct for receiving results from multiple subtasks in parallel
pub struct JoinBarrier<Ret: Send + Sync> {
    /// Atomic counter counting missing arguments before this join can be executed.
    pub ret_counter: AtomicUsize,
    /// Function to execute when all arguments have arrived.
    pub joinfun: ReduceStyle<Ret>,
    /// Extra argument to pass to `joinfun` only used when `joinfun` is `ReduceStyle::Arg`.
    pub joinarg: Option<Ret>,
    /// Vector holding the results of all subtasks. Initialized unsafely so can't be used
    /// for anything until all the values have been put in place.
    pub joinfunarg: Vec<Ret>,
    /// Where to send the result of the execution of `joinfun`
    pub parent: ResultReceiver<Ret>,
}

/// Enum describing what to do with results of `Task`s and `JoinBarrier`s.
pub enum ResultReceiver<Ret: Send + Sync> {
    /// Algorithm has Reduce style and the value should be inserted into a `JoinBarrier`
    Join(Unique<Ret>, Box<JoinBarrier<Ret>>),
    /// Algorithm has Search style and results should be sent directly to the owner.
    Channel(Arc<Mutex<Sender<Ret>>>),
}

impl<Ret: Send + Sync> Clone for ResultReceiver<Ret> {
    fn clone(&self) -> Self {
        match *self {
            ResultReceiver::Join(..) => panic!("Unable to clone ResultReceiver::Join"),
            ResultReceiver::Channel(ref c) => ResultReceiver::Channel(c.clone()),
        }
    }
}

/// Enum indicating there was a problem fetching a result from a job.
#[derive(Debug)]
pub enum ResultError {
    /// Returned from `try_recv` when no results are available at the time of the call
    NoResult,
    /// Returned from `try_recv` and `recv` when there are no more results to fetch
    Completed,
}
impl fmt::Display for ResultError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let msg = match *self {
            ResultError::Completed => "The job has finished executing, no results left to read",
            ResultError::NoResult => "No results available",
        };
        write!(f, "{}", msg)
    }
}

/// The handle for a computation. Can be used to fetch results of the computation.
/// Upon drop it will wait for the entire computation to complete if it's still executing.
/// Algorithm termination is detected by the `try_recv` and `recv` methods returning a `ResultError`
pub struct Job<Ret> {
    port: Receiver<Ret>,
}

impl<Ret> Job<Ret> {
    /// Attempt to get a result from this `Job` without blocking.
    /// Will return a `ResultError` if no result is available at the time of call.
    pub fn try_recv(&self) -> Result<Ret, ResultError> {
        match self.port.try_recv() {
            Ok(res) => Ok(res),
            Err(e) => match e {
                TryRecvError::Empty => Err(ResultError::NoResult),
                TryRecvError::Disconnected => Err(ResultError::Completed),
            }
        }
    }

    /// Block and wait for a result. If a result is available it will return immediately.
    pub fn recv(&self) -> Result<Ret, ResultError> {
        match self.port.recv() {
            Ok(res) => Ok(res),
            Err(_) => Err(ResultError::Completed),
        }
    }
}

impl<Ret> Drop for Job<Ret> {
    /// Don't allow a job to be dropped while it's still computing.
    /// Block and fetch all results.
    fn drop(&mut self) {
        while let Ok(_) = self.port.recv() {}
    }
}

/// A handle for a specific `Algorithm` running on a `ForkPool`.
/// Acquired from `ForkPool::init_algorithm`.
pub struct AlgoOnPool<'a, Arg: 'a + Send, Ret: 'a + Send + Sync> {
    forkpool: &'a ForkPool<'a, Arg, Ret>,
    algo: Algorithm<Arg, Ret>,
}

impl<'a, Arg: Send, Ret: Send + Sync> AlgoOnPool<'a, Arg, Ret> {
    /// Schedule a new computation. Returns instantly with a handle to the computation.
    ///
    /// Return value(s) can be read from the returned `Job`.
    /// `AlgoStyle::Reduce` will only return one message on this channel.
    ///
    /// `AlgoStyle::Search` algorithm might return arbitrary number of messages.
    pub fn schedule(&self, arg: Arg) -> Job<Ret> {
        let (chan, port) = channel();

        let task = Task {
            algo: self.algo,
            arg: arg,
            join: ResultReceiver::Channel(Arc::new(Mutex::new(chan))),
        };
        self.forkpool.schedule(task);

        Job { port: port }
    }
}

/// Main struct of the ForkJoin library.
/// Represents a pool of threads implementing a work stealing algorithm.
pub struct ForkPool<'a, Arg: Send, Ret: Send + Sync> {
    #[allow(dead_code)]
    joinguard: thread_scoped::JoinGuard<'a, ()>,
    channel: Sender<SupervisorMsg<Arg, Ret>>,
}

impl<'a, Arg: Send + 'a, Ret: Send + Sync + 'a> ForkPool<'a, Arg, Ret> {
    /// Create a new `ForkPool` using num_cpus to determine pool size
    pub fn new() -> ForkPool<'a, Arg, Ret> {
        let nthreads = num_cpus::get();
        ForkPool::with_threads(nthreads)
    }

    /// Create a new `ForkPool` with `nthreads` `WorkerThread`s at its disposal.
    pub fn with_threads(nthreads: usize) -> ForkPool<'a, Arg, Ret> {
        assert!(nthreads > 0);
        let (channel, joinguard) = PoolSupervisorThread::spawn(nthreads);

        ForkPool {
            joinguard: joinguard,
            channel: channel,
        }
    }

    /// Apply a specified algorithm to this `ForkPool` and get a handle for it where jobs
    /// can be scheduled.
    pub fn init_algorithm(&self, algorithm: Algorithm<Arg, Ret>) -> AlgoOnPool<Arg, Ret> {
        AlgoOnPool {
            forkpool: self,
            algo: algorithm,
        }
    }

    fn schedule(&self, task: Task<Arg, Ret>) {
        self.channel.send(SupervisorMsg::Schedule(task)).unwrap();
    }
}

impl<'a, Arg: Send, Ret: Send + Sync> Drop for ForkPool<'a, Arg, Ret> {
    fn drop(&mut self) {
        match self.channel.send(SupervisorMsg::Shutdown) {
            Ok(_) => (),
            Err(e) => panic!("Unable to send Shutdown to supervisor: {}", e),
        }
    }
}
