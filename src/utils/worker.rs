use crate::elog;
use std::marker::PhantomData;
use std::panic::catch_unwind;
use std::panic::UnwindSafe;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

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

#[derive(Debug)]
pub enum JobError<J: Job> {
    Error(J::Error),
    Panicked(String),
}

impl<J: Job> std::fmt::Display for JobError<J> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JobError::Error(err) => write!(f, "Job failed with safe error: {err}"),
            JobError::Panicked(err) => write!(f, "Job panicked with error: {err}"),
        }
    }
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct JobResult<J: Job> {
    pub job_id: String,
    pub result: Result<J::Output, JobError<J>>,
}

type AtomicJMReciever<J> = Arc<Mutex<Receiver<JobMessage<J>>>>;

enum JobResultMessage<J: Job> {
    Result {
        job_id: String,
        job_result: Result<J::Output, J::Error>,
    },
    Panicked {
        job_id: String,
        panic_error: String,
    },
    Terminated(usize),
}

type AtomicJRSender<J> = Arc<Sender<JobResultMessage<J>>>;

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
        receiver: AtomicJMReciever<J>,
        sender: AtomicJRSender<J>,
    ) -> Self {
        elog!(debug, "[{id}] Worker initialising");
        let thread_receiver = receiver.clone();
        let thread = std::thread::spawn(move || {
            elog!(debug, "[{id}] Starting work loop");
            loop {
                let job = {
                    let reciever_lock = thread_receiver.lock().unwrap();
                    reciever_lock.recv()
                };
                match job {
                    Ok(job_message) => {
                        match job_message {
                            JobMessage::Execute(job) => {
                                let jid = job.get_job_id().to_string();
                                elog!(debug, "[{id}] Recieved Job with Id: {jid}",);
                                // Job.execute consumes the Job object by taking ownership of it to be UnwindSafe.
                                // Execute the job, if it panics, catch the panic and continue
                                match catch_unwind(|| job.execute()) {
                                    Ok(job_result) => {
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
                                        let dwn_panic_err =
                                            panic_err.downcast_ref::<Box<dyn ToString>>();
                                        if let Some(dwn_panic_err) = dwn_panic_err {
                                            if sender
                                                .send(JobResultMessage::Panicked {
                                                    job_id: jid,
                                                    panic_error: dwn_panic_err.to_string(),
                                                })
                                                .is_err()
                                            {
                                                break;
                                            }
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
    job_sender: Option<Sender<JobMessage<J>>>,
    result_reciever: Option<Receiver<JobResultMessage<J>>>,
    debug: bool,
    num_workers: usize,
}

impl<J> ThreadPool<J>
where
    J: Job,
{
    pub fn new(num_workers: usize, debug: bool) -> Self {
        elog!(debug, "Starting thread pool with {num_workers} threads");

        let (job_sender, job_receiver) = mpsc::channel();
        let job_receiver = Arc::new(Mutex::new(job_receiver));

        let (result_sender, result_reciever) = mpsc::channel();
        let result_sender = Arc::new(result_sender);

        let mut workers = Vec::with_capacity(num_workers);
        for id in 0..num_workers {
            workers.push(Worker::new(
                id,
                debug,
                job_receiver.clone(),
                result_sender.clone(),
            ));
        }
        ThreadPool {
            workers,
            job_sender: Some(job_sender),
            result_reciever: Some(result_reciever),
            debug,
            num_workers,
        }
    }

    pub fn execute(&self, job: J) {
        self.job_sender
            .as_ref()
            .unwrap()
            .send(JobMessage::Execute(job))
            .expect("execute cannot be called after closing the channel");
    }

    pub fn wait(mut self) -> Vec<JobResult<J>> {
        let job_sender = self.job_sender.as_ref().unwrap();
        for _ in 0..self.num_workers {
            job_sender.send(JobMessage::Terminate).unwrap();
        }
        let result_reciever = self.result_reciever.as_ref().unwrap();
        let mut terminated = 0;
        let mut results = Vec::new();
        while terminated < self.num_workers {
            match result_reciever.recv().unwrap() {
                JobResultMessage::Result { job_id, job_result } => {
                    results.push(JobResult::<J> {
                        job_id,
                        result: job_result.map_err(JobError::Error),
                    });
                }
                JobResultMessage::Terminated(thread_id) => {
                    terminated += 1;
                    elog!(self.debug, "[{thread_id}] Work loop Successully terminated")
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
            }
        }
        for mut worker in self.workers.drain(..) {
            if let Some(thread) = worker.thread.take() {
                thread
                    .join()
                    .expect("panices are handled in thread, should not reach error");
                elog!(self.debug, "[{wid}] Thread stopped", wid = worker.id)
            }
        }
        elog!(self.debug, "Stopped thread pool");
        results
    }
}
