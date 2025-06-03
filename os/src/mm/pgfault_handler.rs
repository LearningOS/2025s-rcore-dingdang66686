use crate::task::current_task_do_pgfault;

/// Handle page fault in user space
pub fn do_pgfault(addr: usize) -> Result<(), ()> {
    trace!("do_pgfault: addr = {:#x}", addr);
    current_task_do_pgfault(addr)
}