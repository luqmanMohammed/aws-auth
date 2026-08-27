use crate::elog;
use std::marker::PhantomData;
use std::panic::UnwindSafe;
use std::panic::catch_unwind;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use thiserror::Error;

pub trait Job: Send + UnwindSafe + 'static {
    type Error: std::error::Error + Send + 'static;
    type Output: std::fmt::Debug + Send + 'static;

    fn get_job_id(&self) -> &str;
    fn execute(self) -> Result<Self::Output, Self::Error>;
}

enum JobMessage<J: Job> {
    Execute(J),
    Terminate,
}

#[derive(Debug, Error)]
pub enum JobError<J: Job> {
    #[error("job failed with a safe error: {0}")]
    Error(J::Error),
    #[error("job panicked with error: {0}")]
    Panicked(String),
    #[error("job was not run because an earlier job failed")]
    Skipped,
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct JobResult<J: Job> {
    pub job_id: String,
    pub result: Result<J::Output, JobError<J>>,
}

type SharedJMReceiver<J> = Arc<Mutex<Receiver<JobMessage<J>>>>;

enum JobResultMessage<J: Job> {
    Result {
        job_id: String,
        job_result: Result<J::Output, J::Error>,
    },
    Panicked {
        job_id: String,
        panic_error: String,
    },
    Skipped {
        job_id: String,
    },
    Terminated(usize),
}

type SharedJRSender<J> = Arc<Sender<JobResultMessage<J>>>;

struct Worker<J>
where
    J: Job,
{
    _phantom: PhantomData<J>,
    id: usize,
    thread: Option<JoinHandle<()>>,
}

impl<J> Worker<J>
where
    J: Job,
{
    fn new(
        id: usize,
        debug: bool,
        receiver: SharedJMReceiver<J>,
        sender: SharedJRSender<J>,
        failed: Option<Arc<AtomicBool>>,
    ) -> Self {
        elog!(debug, "[{id}] Worker initialising");
        let thread_receiver = receiver.clone();
        let thread = std::thread::spawn(move || {
            elog!(debug, "[{id}] Starting work loop");
            loop {
                let job = {
                    // unwrapping as its safe since nothing in the locked scope can panic poisoning the lock
                    let receiver_lock = thread_receiver.lock().unwrap();
                    receiver_lock.recv()
                };
                match job {
                    Ok(job_message) => {
                        match job_message {
                            JobMessage::Execute(job) => {
                                let jid = job.get_job_id().to_string();
                                elog!(debug, "[{id}] Received Job with Id: {jid}",);
                                // Queued jobs cannot be unsent, so fail-fast drains the rest of the
                                // queue instead, reporting each as skipped rather than running it.
                                if failed.as_ref().is_some_and(|f| f.load(Ordering::Relaxed)) {
                                    elog!(debug, "[{id}] Skipping job with id: {jid}");
                                    if sender
                                        .send(JobResultMessage::Skipped { job_id: jid })
                                        .is_err()
                                    {
                                        break;
                                    }
                                    continue;
                                }
                                // Job.execute consumes the Job object by taking ownership of it to be UnwindSafe.
                                // Execute the job, if it panics, catch the panic and continue
                                match catch_unwind(|| job.execute()) {
                                    Ok(job_result) => {
                                        if let (Err(_), Some(failed)) = (&job_result, &failed) {
                                            failed.store(true, Ordering::Relaxed);
                                        }
                                        if sender
                                            .send(JobResultMessage::Result {
                                                job_id: jid,
                                                job_result,
                                            })
                                            .is_err()
                                        {
                                            break;
                                        }
                                    }
                                    Err(panic_err) => {
                                        elog!(
                                            debug,
                                            "[{id}] Job with id: {jid} panicked with error: {panic_err:?}"
                                        );
                                        if let Some(failed) = &failed {
                                            failed.store(true, Ordering::Relaxed);
                                        }

                                        let panic_message = panic_err
                                            .downcast_ref::<&str>()
                                            .map(|s| s.to_string())
                                            .or_else(|| panic_err.downcast_ref::<String>().cloned())
                                            .unwrap_or_else(|| {
                                                "<unknown panic payload>".to_string()
                                            });

                                        if sender
                                            .send(JobResultMessage::Panicked {
                                                job_id: jid,
                                                panic_error: panic_message,
                                            })
                                            .is_err()
                                        {
                                            break;
                                        }
                                    }
                                }
                            }
                            JobMessage::Terminate => {
                                let _ = sender.send(JobResultMessage::Terminated(id));
                                break;
                            }
                        }
                    }
                    Err(_) => {
                        // Errors only if the sender side of the channel is closed, ignoring errors and
                        // breaking work loop as terminated to be safe. Ideally this state wont be reached
                        // since the work loop should be terminated using the JobMessage::Terminate message.
                        let _ = sender.send(JobResultMessage::Terminated(id));
                        break;
                    }
                }
            }
        });
        Worker {
            _phantom: PhantomData {},
            id,
            thread: Some(thread),
        }
    }
}

pub struct ThreadPool<J>
where
    J: Job,
{
    workers: Vec<Worker<J>>,
    job_sender: Sender<JobMessage<J>>,
    result_receiver: Receiver<JobResultMessage<J>>,
    debug: bool,
    num_workers: usize,
}

impl<J> ThreadPool<J>
where
    J: Job,
{
    /// With `fail_fast`, the first job to fail stops the rest of the queue from running; those
    /// jobs come back from [`ThreadPool::wait`] as [`JobError::Skipped`].
    pub fn new(num_workers: usize, debug: bool, fail_fast: bool) -> Self {
        elog!(debug, "Starting thread pool with {num_workers} threads");

        let (job_sender, job_receiver) = mpsc::channel();
        let job_receiver = Arc::new(Mutex::new(job_receiver));

        let (result_sender, result_receiver) = mpsc::channel();
        let result_sender = Arc::new(result_sender);

        let failed = fail_fast.then(|| Arc::new(AtomicBool::new(false)));

        let mut workers = Vec::with_capacity(num_workers);
        for id in 0..num_workers {
            workers.push(Worker::new(
                id,
                debug,
                job_receiver.clone(),
                result_sender.clone(),
                failed.clone(),
            ));
        }
        ThreadPool {
            workers,
            job_sender,
            result_receiver,
            debug,
            num_workers,
        }
    }

    pub fn execute(&self, job: J) {
        self.job_sender
            .send(JobMessage::Execute(job))
            .expect("execute cannot be called after closing the channel");
    }

    pub fn wait(mut self) -> Vec<JobResult<J>> {
        for _ in 0..self.num_workers {
            self.job_sender.send(JobMessage::Terminate).unwrap();
        }
        let mut terminated = 0;
        let mut results = Vec::new();
        while terminated < self.num_workers {
            match self.result_receiver.recv().unwrap() {
                JobResultMessage::Result { job_id, job_result } => {
                    results.push(JobResult::<J> {
                        job_id,
                        result: job_result.map_err(JobError::Error),
                    });
                }
                JobResultMessage::Terminated(thread_id) => {
                    terminated += 1;
                    elog!(
                        self.debug,
                        "[{thread_id}] Work loop Successfully terminated"
                    )
                }
                JobResultMessage::Panicked {
                    job_id,
                    panic_error,
                } => {
                    results.push(JobResult {
                        job_id,
                        result: Err(JobError::Panicked(panic_error)),
                    });
                }
                JobResultMessage::Skipped { job_id } => {
                    results.push(JobResult {
                        job_id,
                        result: Err(JobError::Skipped),
                    });
                }
            }
        }
        for mut worker in self.workers.drain(..) {
            if let Some(thread) = worker.thread.take() {
                thread
                    .join()
                    .expect("panics are handled in thread, should not reach error");
                elog!(self.debug, "[{wid}] Thread stopped", wid = worker.id)
            }
        }
        elog!(self.debug, "Stopped thread pool");
        results
    }
}

// Tests were written by AI (Claude Opus 5), not reviewed by Author
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[derive(Debug, Error)]
    #[error("boom")]
    struct Boom;

    struct CountingJob {
        id: String,
        succeed: bool,
        ran: Arc<AtomicUsize>,
    }

    impl UnwindSafe for CountingJob {}

    impl Job for CountingJob {
        type Error = Boom;
        type Output = ();

        fn get_job_id(&self) -> &str {
            &self.id
        }

        fn execute(self) -> Result<(), Boom> {
            self.ran.fetch_add(1, Ordering::SeqCst);
            if self.succeed { Ok(()) } else { Err(Boom) }
        }
    }

    /// One worker, so the queue order below is also the execution order.
    fn run(fail_fast: bool, succeed: &[bool]) -> (Vec<JobResult<CountingJob>>, usize) {
        let ran = Arc::new(AtomicUsize::new(0));
        let pool: ThreadPool<CountingJob> = ThreadPool::new(1, false, fail_fast);
        for (index, succeed) in succeed.iter().enumerate() {
            pool.execute(CountingJob {
                id: index.to_string(),
                succeed: *succeed,
                ran: ran.clone(),
            });
        }
        let results = pool.wait();
        let ran = ran.load(Ordering::SeqCst);
        (results, ran)
    }

    #[test]
    fn without_fail_fast_every_job_runs_despite_a_failure() {
        let (results, ran) = run(false, &[false, true, true]);

        assert_eq!(ran, 3, "all three jobs should have executed");
        assert_eq!(results.len(), 3);
        assert!(
            !results
                .iter()
                .any(|r| matches!(r.result, Err(JobError::Skipped))),
            "nothing should be skipped without fail-fast"
        );
    }

    #[test]
    fn fail_fast_skips_the_jobs_queued_behind_a_failure() {
        let (results, ran) = run(true, &[false, true, true]);

        assert_eq!(ran, 1, "only the failing job should have executed");
        let skipped = results
            .iter()
            .filter(|r| matches!(r.result, Err(JobError::Skipped)))
            .count();
        assert_eq!(
            skipped, 2,
            "the two queued behind it are reported as skipped"
        );
        assert_eq!(results.len(), 3, "every job is still accounted for");
    }

    #[test]
    fn fail_fast_runs_everything_when_nothing_fails() {
        let (results, ran) = run(true, &[true, true, true]);

        assert_eq!(ran, 3);
        assert!(results.iter().all(|r| r.result.is_ok()));
    }
}
