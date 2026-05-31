// Copyright 2026 Andrea Gilot
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

//! Generic parallel worker-pool pipeline with per-worker owned state.

use anyhow::{anyhow, Result};
use std::sync::Mutex;

/// Run a parallel pipeline: spawn one thread per worker, distribute items
/// from a shared queue, stream results back to the caller's thread.
///
/// Each worker owns one `W` for its entire lifetime. Use this when workers
/// hold per-thread resources (an API token, a matcher, a connection).
/// For stateless workers, pass `vec![(); n]`.
///
/// Results arrive at `handle` on the calling thread in arrival order
/// (non-deterministic across runs).
///
/// # Errors
///
/// The first error from any worker or from `handle` aborts the pipeline.
/// Workers finish their in-flight item and then exit; further results
/// already in the queue are discarded. The function returns that first
/// error; subsequent errors from other workers are silently dropped.
///
/// # Panics
///
/// Returns an error (does not panic) if a worker thread panics; the panic
/// payload is included in the error message.
pub fn parallel_pipeline<T, W, P, R, H>(
    items: Vec<T>,
    workers: Vec<W>,
    process: P,
    mut handle: H,
) -> Result<()>
where
    T: Send,
    W: Send,
    P: Fn(&mut W, T) -> Result<R> + Send + Sync,
    R: Send,
    H: FnMut(R) -> Result<()>,
{
    let queue = Mutex::new(items.into_iter());
    let (tx, rx) = crossbeam_channel::unbounded::<Result<R>>();

    crossbeam::thread::scope(|s| -> Result<()> {
        for mut worker in workers {
            let tx = tx.clone();
            let queue = &queue;
            let process = &process;
            s.spawn(move |_| loop {
                let item = match queue.lock().expect("queue mutex poisoned").next() {
                    Some(x) => x,
                    None => break,
                };
                let result = process(&mut worker, item);
                if tx.send(result).is_err() {
                    break;
                }
            });
        }
        drop(tx);

        while let Ok(result) = rx.recv() {
            handle(result?)?;
        }
        Ok(())
    })
    .map_err(|e| anyhow!("Worker thread panicked: {e:?}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn stateless_workers_square_numbers() -> Result<()> {
        let items: Vec<i32> = (1..=10).collect();
        let workers = vec![(); 4];
        let sum = Arc::new(AtomicUsize::new(0));
        let sum_clone = sum.clone();

        parallel_pipeline(
            items,
            workers,
            |_, n: i32| Ok(n * n),
            |squared: i32| {
                sum_clone.fetch_add(squared as usize, Ordering::Relaxed);
                Ok(())
            },
        )?;

        assert_eq!(sum.load(Ordering::Relaxed), 385);
        Ok(())
    }

    #[test]
    fn stateful_workers_count_per_worker() -> Result<()> {
        // Workers carry a usize counter; process increments it.
        // Verify the total across all workers equals item count.
        let items: Vec<()> = vec![(); 100];
        let workers: Vec<usize> = vec![0; 4];

        let total_processed = Arc::new(AtomicUsize::new(0));
        let total_clone = total_processed.clone();

        parallel_pipeline(
            items,
            workers,
            |counter: &mut usize, _| {
                *counter += 1;
                Ok(*counter)
            },
            |latest: usize| {
                total_clone.fetch_add(1, Ordering::Relaxed);
                assert!(latest > 0);
                Ok(())
            },
        )?;

        assert_eq!(total_processed.load(Ordering::Relaxed), 100);
        Ok(())
    }

    #[test]
    fn first_error_aborts() -> Result<()> {
        let items: Vec<i32> = (1..=1000).collect();
        let workers = vec![(); 4];

        let result = parallel_pipeline(
            items,
            workers,
            |_, n: i32| {
                if n == 42 {
                    Err(anyhow!("hit 42"))
                } else {
                    Ok(n)
                }
            },
            |_: i32| Ok(()),
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("hit 42"));
        Ok(())
    }

    #[test]
    fn handler_error_aborts() -> Result<()> {
        let items: Vec<i32> = (1..=1000).collect();
        let workers = vec![(); 4];

        let result = parallel_pipeline(
            items,
            workers,
            |_, n: i32| Ok(n),
            |n: i32| {
                if n > 500 {
                    Err(anyhow!("too big"))
                } else {
                    Ok(())
                }
            },
        );

        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn empty_items_succeeds() -> Result<()> {
        let items: Vec<i32> = vec![];
        let workers = vec![(); 4];
        parallel_pipeline(items, workers, |_, n: i32| Ok(n), |_| Ok(()))
    }

    #[test]
    fn empty_workers_with_empty_items_succeeds() -> Result<()> {
        let items: Vec<i32> = vec![];
        let workers: Vec<()> = vec![];
        parallel_pipeline(items, workers, |_, n: i32| Ok(n), |_| Ok(()))
    }
}
