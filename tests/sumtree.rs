// Copyright (c) 2015-2016 Linus Färnstrand.
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or http://opensource.org/licenses/MIT>,
// at your option. All files in the project carrying such
// notice may not be copied, modified, or distributed except
// according to those terms.

extern crate forkjoin;

use forkjoin::{TaskResult,ForkPool,AlgoStyle,ReduceStyle,Algorithm};

#[cfg(test)]
struct Tree {
    value: usize,
    children: Vec<Tree>,
}

#[cfg(test)]
fn create_tree() -> Tree {
    Tree {
        value: 100,
        children: vec![Tree {
            value: 250,
            children: vec![
                Tree {
                    value: 500,
                    children: vec![],
                },
                Tree {
                    value: 10,
                    children: vec![],
                }],
        }],
    }
}

#[test]
fn sum_tree() {
    let tree = create_tree();

    let seq_sum = sum_tree_seq(&tree);
    let par_sum = sum_tree_par(&tree, 1);
    assert_eq!(860, seq_sum);
    assert_eq!(seq_sum, par_sum);
}

#[cfg(test)]
fn sum_tree_seq(t: &Tree) -> usize {
    t.value + t.children.iter().fold(0, |acc, t2| acc + sum_tree_seq(t2))
}

#[cfg(test)]
fn sum_tree_par(t: &Tree, nthreads: usize) -> usize {
    let forkpool = ForkPool::with_threads(nthreads);
    let sumpool = forkpool.init_algorithm(Algorithm {
        fun: sum_tree_task,
        style: AlgoStyle::Reduce(ReduceStyle::Arg(sum_tree_join)),
    });

    let job = sumpool.schedule(t);
    job.recv().unwrap()
}

#[cfg(test)]
fn sum_tree_task(t: &Tree) -> TaskResult<&Tree, usize> {
    let val = t.value;

    if t.children.is_empty() {
        TaskResult::Done(val)
    } else {
        let mut fork_args: Vec<&Tree> = vec![];
        for c in t.children.iter() {
            fork_args.push(c);
        }

        TaskResult::Fork(fork_args, Some(val))
    }
}

#[cfg(test)]
fn sum_tree_join(value: &usize, values: &[usize]) -> usize {
    *value + values.iter().fold(0, |acc, &v| acc + v)
}
