//! Implementation of [`TaskContext`]

#[derive(Copy, Clone)]
#[repr(C)]
/// task context structure containing some registers
pub struct TaskContext {
    /// Ret position after task switching
    ra: usize,
    /// Stack pointer
    sp: usize,
    /// s0-11 register, callee saved
    s: [usize; 12],
    /// syscall counter
    syscall_counter: [usize; crate::syscall::SYSCALL_MAX_ID + 1],
}

impl TaskContext {
    /// Create a new empty task context
    pub fn zero_init() -> Self {
        Self {
            ra: 0,
            sp: 0,
            s: [0; 12],
            syscall_counter: [0; crate::syscall::SYSCALL_MAX_ID + 1],
        }
    }
    /// Create a new task context with a trap return addr and a kernel stack pointer
    pub fn goto_restore(kstack_ptr: usize) -> Self {
        extern "C" {
            fn __restore();
        }
        Self {
            ra: __restore as usize,
            sp: kstack_ptr,
            s: [0; 12],
            syscall_counter: [0; crate::syscall::SYSCALL_MAX_ID + 1],
        }
    }
    /// Increment current task syscall count
    pub fn increment_syscall_stat(&mut self, id: usize) {
        self.syscall_counter[id] += 1;
    }
    /// Get current task syscall count
    pub fn get_syscall_count(&self, id: usize) -> usize {
        self.syscall_counter[id]
    }
    /// Get current task user stack
    pub fn get_user_stack(&self) -> usize {
        self.sp
    }
}
