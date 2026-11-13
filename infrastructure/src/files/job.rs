//! The job a Find in Files or Replace in Files run is: a thread with its own sink, stopped
//! through a shared [`CancelToken`]. Waiting for the job also waits for the sink's last call,
//! so nothing reaches the caller after [`JobHandle::wait`] returns.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

use super::StartError;

/// A request to stop some work. Clones share one flag, so the side that asks and the side
/// that stops each hold their own.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// A token nobody has cancelled yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Asks every holder of this token, or of its clones, to stop.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    #[must_use]
    /// Whether [`Self::cancel`] was called on this token or any of its clones.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    /// The flag itself, for the search engine's cancellable calls.
    pub(crate) fn flag(&self) -> &AtomicBool {
        &self.0
    }
}

/// Which run a job is: it names the threads and the error when they cannot start, and decides
/// what the sink gets after a cancel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JobKind {
    /// Results stop at a cancel: the sink gets no more files.
    FindInFiles,
    /// Every event reaches the sink, those after a cancel too, so that the sink hears of every
    /// file written.
    ReplaceInFiles,
}

impl JobKind {
    fn thread_name(self) -> &'static str {
        match self {
            Self::FindInFiles => "stet-find-in-files",
            Self::ReplaceInFiles => "stet-replace-in-files",
        }
    }

    fn sink_thread_name(self) -> &'static str {
        match self {
            Self::FindInFiles => "stet-find-in-files-sink",
            Self::ReplaceInFiles => "stet-replace-in-files-sink",
        }
    }

    fn delivers_after_cancel(self) -> bool {
        self == Self::ReplaceInFiles
    }

    fn start_error(self, error: io::Error) -> StartError {
        match self {
            Self::FindInFiles => StartError::SearchThread(error),
            Self::ReplaceInFiles => StartError::ReplaceThread(error),
        }
    }
}

/// The flags a job and its handle share.
#[derive(Debug, Clone, Default)]
pub(crate) struct Control {
    /// Set only by a cancel, for the summary.
    cancelled: CancelToken,
    /// Set by a cancel, or by the job itself when it ends early (a hit cap); the job polls it.
    stop: CancelToken,
}

impl Control {
    /// Stops the job and marks it cancelled.
    pub(crate) fn cancel(&self) {
        self.cancelled.cancel();
        self.stop.cancel();
    }

    /// Stops the job without marking it cancelled.
    pub(crate) fn stop(&self) {
        self.stop.cancel();
    }

    pub(crate) fn stopping(&self) -> bool {
        self.stop.is_cancelled()
    }

    pub(crate) fn cancelled(&self) -> bool {
        self.cancelled.is_cancelled()
    }

    /// The token the job's work polls.
    pub(crate) fn stop_token(&self) -> &CancelToken {
        &self.stop
    }
}

/// A Find in Files or Replace in Files run on threads of its own, with `P` for what it shares
/// while it runs. Dropping the handle cancels the run.
#[derive(Debug)]
pub struct JobHandle<S, P = ()> {
    control: Control,
    progress: P,
    thread: Option<JoinHandle<S>>,
    /// The summary when the run's thread panicked.
    panicked: fn() -> S,
}

impl<S, P> JobHandle<S, P> {
    /// Stops the run. Find in Files hands the sink no more files. Replace in Files writes no
    /// file after this; a file being written when it stops is written whole and still
    /// reported, so every change on disk reaches the sink.
    pub fn cancel(&self) {
        self.control.cancel();
    }

    /// Whether the run's thread has ended, so [`Self::wait`] won't block.
    pub fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Waits for the run, and for the sink's last call, to finish.
    pub fn wait(mut self) -> S {
        let panicked = self.panicked;
        self.thread.take().map_or_else(panicked, |thread| {
            thread.join().unwrap_or_else(|_| panicked())
        })
    }

    pub(crate) fn progress_state(&self) -> &P {
        &self.progress
    }

    #[cfg(test)]
    pub(crate) fn control(&self) -> &Control {
        &self.control
    }
}

impl<S, P> Drop for JobHandle<S, P> {
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.cancel();
        }
    }
}

/// Hands a job's events to its sink, which runs on a thread of its own so that a slow sink
/// never holds up the walk.
pub(crate) struct Reporter<E> {
    sender: Sender<E>,
    sink: JoinHandle<()>,
}

impl<E> Reporter<E> {
    pub(crate) fn report(&self, event: E) {
        let _ = self.sender.send(event);
    }

    /// Waits until the sink has taken every event reported.
    pub(crate) fn finish(self) {
        drop(self.sender);
        let _ = self.sink.join();
    }
}

/// Starts `work` on a thread of its own, with a reporter whose events go to `sink` on another.
/// `control` is the job's, already shared with whatever `work` polls; `progress` stays with
/// the handle.
pub(crate) fn spawn<E, S, P>(
    kind: JobKind,
    control: Control,
    progress: P,
    panicked: fn() -> S,
    mut sink: impl FnMut(E) + Send + 'static,
    work: impl FnOnce(Reporter<E>) -> S + Send + 'static,
) -> Result<JobHandle<S, P>, StartError>
where
    E: Send + 'static,
    S: Send + 'static,
{
    let (sender, receiver) = mpsc::channel::<E>();
    let sink_control = control.clone();
    let sink = thread::Builder::new()
        .name(kind.sink_thread_name().to_owned())
        .spawn(move || {
            for event in receiver {
                if !kind.delivers_after_cancel() && sink_control.cancelled() {
                    break;
                }
                sink(event);
            }
        })
        .map_err(|error| kind.start_error(error))?;
    let reporter = Reporter { sender, sink };
    let thread = thread::Builder::new()
        .name(kind.thread_name().to_owned())
        .spawn(move || work(reporter))
        .map_err(|error| kind.start_error(error))?;
    Ok(JobHandle {
        control,
        progress,
        thread: Some(thread),
        panicked,
    })
}
