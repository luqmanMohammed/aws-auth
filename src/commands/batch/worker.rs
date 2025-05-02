use std::marker::PhantomData;
use std::panic::{catch_unwind, UnwindSafe};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

pub trait Job: Send + UnwindSafe + 'static {
    fn execute(self);
}

struct Worker<J>
where
    J: Job,
{
    _phantom: PhantomData<J>,
    thread: Option<JoinHandle<()>>,
}

impl<J> Worker<J>
where
    J: Job,
{
    fn new(id: usize, receiver: Arc<Mutex<Receiver<J>>>) -> Self {
        let thread_receiver = receiver.clone();
        let thread = std::thread::spawn(move || {
            loop {
                let reciever_lock = thread_receiver.lock().unwrap();
                let job = reciever_lock.recv();
                drop(reciever_lock);
                match job {
                    Ok(job) => {
                        // Execute the job, if it panics, catch the panic and continue
                        if let Err(err) = catch_unwind(|| job.execute()) {
                            eprintln!("Job in Thread {id} panic'd with error: {err:?}")
                        };
                    }
                    Err(_) => {
                        break;
                    }
                }
            }
        });
        Worker {
            _phantom: PhantomData {},
            thread: Some(thread),
        }
    }
}

pub struct ThreadPool<J>
where
    J: Job,
{
    workers: Vec<Worker<J>>,
    job_sender: Option<Sender<J>>,
}

impl<J> ThreadPool<J>
where
    J: Job,
{
    pub fn new(num_workers: usize) -> Self {
        let (sender, receiver) = mpsc::channel();
        let receiver = Arc::new(Mutex::new(receiver));
        let mut workers = Vec::with_capacity(num_workers);
        for id in 0..num_workers {
            workers.push(Worker::new(id, receiver.clone()));
        }
        ThreadPool {
            workers,
            job_sender: Some(sender),
        }
    }

    pub fn execute(&self, job: J) {
        self.job_sender.as_ref().unwrap().send(job).unwrap();
    }
}

impl<J> Drop for ThreadPool<J>
where
    J: Job,
{
    fn drop(&mut self) {
        if let Some(sender) = self.job_sender.take() {
            drop(sender);
        }
        for mut worker in self.workers.drain(..) {
            if let Some(thread) = worker.thread.take() {
                thread.join().unwrap();
            }
        }
    }
}
