// SPDX-License-Identifier: MPL-2.0

//! Tasks are the unit of code execution.

pub(crate) mod atomic_mode;
mod preempt;
pub mod scheduler;
mod utils;

use core::{any::Any, borrow::Borrow, future::Future, ops::Deref, option::Option, ptr::NonNull};

use maitake::task::JoinHandle;
pub use maitake::{
    future::yield_now,
    time::{sleep, Duration},
};
pub(crate) use preempt::cpu_local::reset_preempt_info;
// use processor::current_task;
use utils::ForceSync;

pub use self::preempt::{disable_preempt, DisabledPreemptGuard};
use crate::{prelude::*, trap::in_interrupt_context, user::UserSpace};

/// A task that executes a function to the end.
///
/// Each task is associated with per-task data and an optional user space.
/// If having a user space, the task can switch to the user space to
/// execute user code. Multiple tasks can share a single user space.
#[derive(Debug)]
pub struct Task {
    data: Box<dyn Any + Send + Sync>,
    local_data: ForceSync<Box<dyn Any + Send>>,
    user_space: Option<Arc<UserSpace>>,
}

impl Task {
    /// Gets the current task.
    ///
    /// It returns `None` if the function is called in the bootstrap context.
    pub fn current() -> Option<CurrentTask> {
        scheduler::get_current_task_ptr().map(|ptr| unsafe {
            // SAFETY: `get_current_task_ptr`为Some时，其内部的值源自`Arc::as_ptr(task)，非空。
            CurrentTask::new(NonNull::new_unchecked(ptr as *mut _))
        })
    }

    /// Gets thread-local storage pointer.
    pub fn tls_pointer(&self) -> Option<usize> {
        self.user_space
            .as_ref()
            .map(|user_space| user_space.tls_pointer())
    }

    /// Returns the task data.
    pub fn data(&self) -> &Box<dyn Any + Send + Sync> {
        &self.data
    }

    /// Returns the user space of this task, if it has.
    pub fn user_space(&self) -> Option<&Arc<UserSpace>> {
        if self.user_space.is_some() {
            Some(self.user_space.as_ref().unwrap())
        } else {
            None
        }
    }

    /// Saves the FPU state for user task.
    pub fn save_fpu_state(&self) {
        let Some(user_space) = self.user_space.as_ref() else {
            return;
        };

        user_space.fpu_state().save();
    }

    /// Restores the FPU state for user task.
    pub fn restore_fpu_state(&self) {
        let Some(user_space) = self.user_space.as_ref() else {
            return;
        };

        user_space.fpu_state().restore();
    }

    /// Sets the data associated with the task.
    pub fn set_data(&mut self, data: Box<dyn Any + Send + Sync>) {
        self.data = data;
    }

    /// Sets the local data associated with the task.
    pub fn set_local_data(&mut self, data: Box<dyn Any + Send>) {
        self.local_data = ForceSync::new(data);
    }
}

impl Task {
    /// Kicks the task scheduler to run the task.
    ///
    /// BUG: This method highly depends on the current scheduling policy.
    #[track_caller]
    pub fn run<Fut>(self: &Arc<Self>, future: Fut) -> JoinHandle<Fut::Output>
    where
        Fut: Future + Send + 'static,
        Fut::Output: Send + 'static,
    {
        scheduler::spawn_user_task(self.clone(), future)
    }
}

/// Options to create or spawn a new task.
pub struct TaskOptions {
    data: Option<Box<dyn Any + Send + Sync>>,
    local_data: Option<Box<dyn Any + Send>>,
    user_space: Option<Arc<UserSpace>>,
}

impl TaskOptions {
    /// Creates a set of options for a task.
    pub fn new() -> Self {
        Self {
            data: None,
            local_data: None,
            user_space: None,
        }
    }

    /// Sets the data associated with the task.
    pub fn data<T>(mut self, data: T) -> Self
    where
        T: Any + Send + Sync,
    {
        self.data = Some(Box::new(data));
        self
    }

    /// Sets the local data associated with the task.
    pub fn local_data<T>(mut self, data: T) -> Self
    where
        T: Any + Send,
    {
        self.local_data = Some(Box::new(data));
        self
    }

    /// Sets the user space associated with the task.
    pub fn user_space(mut self, user_space: Option<Arc<UserSpace>>) -> Self {
        self.user_space = user_space;
        self
    }

    /// Builds a new task without running it immediately.
    pub fn build(self) -> Task {
        Task {
            data: self.data.unwrap_or_else(|| Box::new(())),
            local_data: ForceSync::new(self.local_data.unwrap_or_else(|| Box::new(()))),
            user_space: self.user_space,
        }
    }
}

impl TaskOptions {
    /// Builds a new task and runs it immediately.
    #[track_caller]
    pub fn spawn<Fut>(self, future: Fut) -> JoinHandle<Fut::Output>
    where
        Fut: Future + Send + 'static,
        Fut::Output: Send + 'static,
    {
        let task = Arc::new(self.build());
        task.run(future)
    }
}

/// The current task.
///
/// This type is not `Send`, so it cannot outlive the current task.
///
/// This type is also not `Sync`, so it can provide access to the local data of the current task.
#[derive(Debug)]
pub struct CurrentTask(NonNull<Task>);

// The intern `NonNull<Task>` contained by `CurrentTask` implies that `CurrentTask` is `!Send` and
// `!Sync`. But it is still good to do this explicitly because these properties are key for
// soundness.
impl !Send for CurrentTask {}
impl !Sync for CurrentTask {}

impl CurrentTask {
    /// # Safety
    ///
    /// The caller must ensure that `task` is the current task.
    unsafe fn new(task: NonNull<Task>) -> Self {
        Self(task)
    }

    /// Returns the local data of the current task.
    ///
    /// Note that the local data is only accessible in the task context. Although there is a
    /// current task in the non-task context (e.g. IRQ handlers), access to the local data is
    /// forbidden as it may cause soundness problems.
    ///
    /// # Panics
    ///
    /// This method will panic if called in a non-task context.
    pub fn local_data(&self) -> &(dyn Any + Send) {
        assert!(!in_interrupt_context());

        let local_data = &self.local_data;

        // SAFETY: The `local_data` field will only be accessed by the current task in the task
        // context, so the data won't be accessed concurrently.
        &**unsafe { local_data.get() }
    }

    /// Returns a cloned `Arc<Task>`.
    pub fn cloned(&self) -> Arc<Task> {
        let ptr = self.0.as_ptr();

        // SAFETY: The current task is always a valid task and it is always contained in an `Arc`.
        unsafe { Arc::increment_strong_count(ptr) };

        // SAFETY: We've increased the reference count in the current `Arc<Task>` above.
        unsafe { Arc::from_raw(ptr) }
    }
}

impl Deref for CurrentTask {
    type Target = Task;

    fn deref(&self) -> &Self::Target {
        // SAFETY: The current task is always a valid task.
        unsafe { self.0.as_ref() }
    }
}

impl AsRef<Task> for CurrentTask {
    fn as_ref(&self) -> &Task {
        self
    }
}

impl Borrow<Task> for CurrentTask {
    fn borrow(&self) -> &Task {
        self
    }
}

#[cfg(ktest)]
mod test {
    use crate::prelude::*;

    #[ktest]
    fn create_task() {
        #[expect(clippy::eq_op)]
        let future = async {
            assert_eq!(1, 1);
        };
        let task = Arc::new(
            crate::task::TaskOptions::new()
                .data(())
                .build(),
        );
        task.run(future);
    }

    #[ktest]
    fn spawn_task() {
        #[expect(clippy::eq_op)]
        let future = async {
            assert_eq!(1, 1);
        };
        let _ = crate::task::TaskOptions::new().data(()).spawn(future);
    }
}
