// Copyright 2015 Linus Färnstrand
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.


use std::sync::atomic::{AtomicUsize,Ordering};
use std::sync::Arc;
use std::ptr::{Unique,write};
use std::sync::mpsc::{Receiver,Sender};
use std::thread;
use libc::funcs::posix88::unistd::usleep;
use std::cmp::min;

use deque::{BufferPool,Worker,Stealer,Stolen};
use rand::{Rng,XorShiftRng,weak_rng};

use ::{Task,JoinBarrier,TaskResult,ResultReceiver,AlgoStyle,SummaStyle,Algorithm};
use ::poolsupervisor::SupervisorMsg;

static STEAL_TRIES_BETWEEN_SYNC: u32 = 50;
static STEAL_TRIES_UNTIL_BACKOFF: u32 = 25;
static BACKOFF_INC_US: u32 = 5;
static BACKOFF_MAX_US: u32 = 250;

/// Messages from the `PoolSupervisor` to `WorkerThread`s
pub enum WorkerMsg<Arg: Send, Ret: Send + Sync> {
    /// A new `Task` to be scheduled for execution by the `WorkerThread`
    Schedule(Task<Arg,Ret>),
    /// Tell the `WorkerThread` to simply try to steal from the other `WorkerThread`s
    Steal,
}

pub struct WorkerThread<Arg: Send, Ret: Send + Sync> {
    id: usize,
    started: bool,
    supervisor_port: Receiver<WorkerMsg<Arg, Ret>>,
    supervisor_channel: Sender<SupervisorMsg<Arg, Ret>>,
    deque: Worker<Task<Arg, Ret>>,
    stealer: Stealer<Task<Arg, Ret>>,
    other_stealers: Vec<Stealer<Task<Arg, Ret>>>,
    rng: XorShiftRng,
    steal_counter: Arc<AtomicUsize>,
    threadcount: usize,
}

impl<'a, Arg: Send + 'a, Ret: Send + Sync + 'a> WorkerThread<Arg,Ret> {
    pub fn new(id: usize,
            port: Receiver<WorkerMsg<Arg,Ret>>,
            channel: Sender<SupervisorMsg<Arg,Ret>>,
            steal_counter: Arc<AtomicUsize>) -> WorkerThread<Arg,Ret> {
        let pool = BufferPool::new();
        let (worker, stealer) = pool.deque();

        WorkerThread {
            id: id,
            started: false,
            supervisor_port: port,
            supervisor_channel: channel,
            deque: worker,
            stealer: stealer,
            other_stealers: vec![],
            rng: weak_rng(),
            steal_counter: steal_counter,
            threadcount: 1,
        }
    }

    pub fn get_stealer(&self) -> Stealer<Task<Arg,Ret>> {
        assert!(!self.started);
        self.stealer.clone()
    }

    pub fn add_other_stealer(&mut self, stealer: Stealer<Task<Arg,Ret>>) {
        assert!(!self.started);
        self.other_stealers.push(stealer);
        self.threadcount += 1;
    }

    pub fn spawn(mut self) -> thread::JoinGuard<'a, ()> {
        assert!(!self.started);
        self.started = true;
        let builder = thread::Builder::new().name(format!("fork-join worker {}", self.id+1));
        let joinguard = builder.scoped(move|| {
            self.main_loop();
        });
        joinguard.unwrap()
    }

    fn main_loop(mut self) {
        loop {
            let message = match self.supervisor_port.try_recv() {
                Ok(msg) => Ok(msg),
                Err(_) => {
                    self.steal_counter.fetch_add(1, Ordering::SeqCst);
                    let recv = self.supervisor_port.recv();
                    self.steal_counter.fetch_sub(1, Ordering::SeqCst);
                    recv
                },
            };
            match message {
                Err(_) => break, // PoolSupervisor has been dropped, lets quit.
                Ok(msg) => {
                    match msg {
                        WorkerMsg::Schedule(task) => self.execute_task(task),
                        WorkerMsg::Steal => (), // Do nothing, it will come to steal further down
                    }
                    loop {
                        self.process_queue();
                        self.steal_counter.fetch_add(1, Ordering::SeqCst);
                        match self.steal() {
                            Some(task) => {
                                self.steal_counter.fetch_sub(1, Ordering::SeqCst);
                                self.execute_task(task);
                            },
                            None => break, // Give up for now
                        }
                    }
                }
            }
            if self.supervisor_channel.send(SupervisorMsg::OutOfWork(self.id)).is_err() {
                break; // Supervisor shut down, so we also shut down
            }
        }
    }

    fn process_queue(&self) {
        loop {
            match self.deque.pop() {
                Some(task) => self.execute_task(task),
                None => break,
            }
        }
    }

    fn execute_task(&self, task: Task<Arg, Ret>) {
        let fun = task.algo.fun;
        match (fun)(task.arg) {
            TaskResult::Done(ret) => {
                self.handle_done(&task.join, ret);
            },
            TaskResult::Fork(args, joinarg) => {
                self.handle_fork(task.algo, task.join, args, joinarg);
            }
        }
    }

    fn steal(&mut self) -> Option<Task<Arg,Ret>> {
        if self.other_stealers.len() == 0 {
            None // No one to steal from
        } else {
            let mut backoff_sleep: u32 = BACKOFF_INC_US;
            for try in 0.. {
                if try % STEAL_TRIES_BETWEEN_SYNC == 0 {
                    if self.threadcount == self.steal_counter.load(Ordering::SeqCst) {
                        break;
                    }
                }
                match self.try_steal() {
                    Some(task) => return Some(task),
                    None => if try > STEAL_TRIES_UNTIL_BACKOFF {
                        unsafe { usleep(backoff_sleep); }
                        backoff_sleep = min(backoff_sleep + BACKOFF_INC_US, BACKOFF_MAX_US);
                    },
                }
            }
            None
        }
    }

    /// Try to steal tasks from the other workers.
    /// Starts at a random worker and tries every worker until a task is stolen or
    /// every worker has been tried once.
    fn try_steal(&mut self) -> Option<Task<Arg,Ret>> {
        let len = self.other_stealers.len();
        let start_victim = self.rng.gen_range(0, len);
        for offset in 0..len {
            match self.other_stealers[(start_victim + offset) % len].steal() {
                Stolen::Data(task) => return Some(task),
                Stolen::Empty | Stolen::Abort => continue,
            }
        }
        None
    }

    fn handle_fork(&self, algo: Algorithm<Arg, Ret>, join: ResultReceiver<Ret>, args: Vec<Arg>, joinarg: Option<Ret>) {
        let len: usize = args.len();
        if len == 0 {
            self.handle_fork_zero(algo, join, joinarg);
        } else {
            let resultreceivers = self.create_result_receivers(len, algo, join, joinarg);
            for (arg,resultreceiver) in args.into_iter().zip(resultreceivers.into_iter()) {
                let forked_task = Task {
                    algo: algo.clone(),
                    arg: arg,
                    join: resultreceiver,
                };
                self.deque.push(forked_task);
            }
        }
    }

    fn handle_fork_zero(&self, algo: Algorithm<Arg, Ret>, join: ResultReceiver<Ret>, joinarg: Option<Ret>) {
        match algo.style {
            AlgoStyle::Summa(ref summastyle) => {
                let joinres = match *summastyle {
                    SummaStyle::NoArg(ref joinfun) => (joinfun)(&Vec::new()[..]),
                    SummaStyle::Arg(ref joinfun) => {
                        let arg = joinarg.unwrap();
                        (joinfun)(&arg, &Vec::new()[..])
                    }
                };
                self.handle_done(&join, joinres);
            },
            _ => (),
        }
    }

    fn create_result_receivers(&self, len: usize, algo: Algorithm<Arg, Ret>, join: ResultReceiver<Ret>, joinarg: Option<Ret>) -> Vec<ResultReceiver<Ret>> {
        let mut resultreceivers = Vec::with_capacity(len);
        match algo.style {
            AlgoStyle::Summa(summastyle) => {
                let (vector, elem_ptrs) = create_result_vec::<Ret>(len);

                let join_arc = Arc::new(JoinBarrier {
                    ret_counter: AtomicUsize::new(len),
                    joinfun: summastyle,
                    joinarg: joinarg,
                    joinfunarg: vector,
                    parent: join,
                });

                for ptr in elem_ptrs.into_iter() {
                    resultreceivers.push(ResultReceiver::Join(ptr, join_arc.clone()));
                }
            },
            AlgoStyle::Search => {
                for _ in 0..len {
                    resultreceivers.push(join.clone());
                }
            }
        }
        resultreceivers
    }

    fn handle_done(&self, join: &ResultReceiver<Ret>, value: Ret) {
        match *join {
            ResultReceiver::Join(ref ptr, ref joinbarrier) => {
                unsafe { write(**ptr, value); } // Writes without dropping since only null in place
                if joinbarrier.ret_counter.fetch_sub(1, Ordering::SeqCst) == 1 {
                    let joinres = match joinbarrier.joinfun {
                        SummaStyle::NoArg(ref joinfun) => (joinfun)(&joinbarrier.joinfunarg),
                        SummaStyle::Arg(ref joinfun) => {
                            let joinarg = match joinbarrier.joinarg.as_ref() {
                                None => panic!("Algorithm has SummaStyle::Arg, but no extra arg passed"),
                                Some(arg) => arg,
                            };
                            (joinfun)(joinarg, &joinbarrier.joinfunarg)
                        },
                    };
                    self.handle_done(&joinbarrier.parent, joinres);
                }
            }
            ResultReceiver::Channel(ref channel) => {
                channel.lock().unwrap().send(value).unwrap();
            }
        }
    }
}

// impl<Arg: Send, Ret: Send + Sync> Drop for WorkerThread<Arg, Ret> {
//     fn drop(&mut self) {
//         println!("Dropping WorkerThread");
//     }
// }

fn create_result_vec<Ret>(n: usize) -> (Vec<Ret>, Vec<Unique<Ret>>) {
    let mut rets: Vec<Ret> = Vec::with_capacity(n);
    let mut rets_ptr: Vec<Unique<Ret>> = Vec::with_capacity(n);
    unsafe {
        rets.set_len(n); // Force it to expand. Values in this will be invalid
        let ptr_0: *mut Ret = rets.get_unchecked_mut(0);
        for i in 0..(n as isize) {
            rets_ptr.push(Unique::new(ptr_0.offset(i)));
        }
    }
    (rets, rets_ptr)
}
