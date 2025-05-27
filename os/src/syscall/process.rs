//! Process management syscalls
use crate::{config::PAGE_SIZE, mm::{MapPermission, VirtAddr}, task::{change_program_brk, current_task_map, current_task_unmap, exit_current_and_run_next, suspend_current_and_run_next}, timer::get_time_us};

use crate::mm::{copy_to_user, read_user_addr, write_user_addr};
use crate::task::current_user_token;

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    let time_val = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };
    copy_to_user(
        current_user_token(),
        _ts as *mut u8,
        &time_val as *const TimeVal as *mut u8,
        core::mem::size_of::<TimeVal>(),
    );
    0
}

/// TODO: Finish sys_trace to pass testcases
/// HINT: You might reimplement it with virtual memory management.
pub fn sys_trace(_trace_request: usize, _id: usize, _data: usize) -> isize {
    trace!("kernel: sys_trace, trace_request: {}", _trace_request);
    match _trace_request {
        0 => {
            read_user_addr(current_user_token(), _id)
                .map(|value| {
                    trace!("kernel: sys_trace: read id {} value {}", _id, value);
                    value as isize
                })
                .unwrap_or_else(|_| {
                    error!("kernel: sys_trace: failed to read id {}", _id);
                    -1
                })
        }
        1 => {
            write_user_addr(current_user_token(), _id, _data as u8)
                .map(|_| {
                    trace!("kernel: sys_trace: wrote id {} value {}", _id, _data);
                    0
                })
                .unwrap_or_else(|_| {
                    error!("kernel: sys_trace: failed to write id {}", _id);
                    -1
                })
        }
        2 => {
            crate::task::get_syscall_count(_id) as isize
        }
        _ => {
            error!("kernel: sys_trace: unknown");
            -1
        }
    }
}

/// allocates a memory region with `len` bytes
pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
    trace!("kernel: sys_mmap");
    if _port & !0x7 != 0 || _port & 0x7 == 0 {
        error!("Bad permission bits");
        return -1;
    }
    let start_va = VirtAddr::from(_start);
    if start_va.page_offset() != 0 {
        error!("Address must be page-aligned");
        return -1
    }

    if _len == 0 {
        error!("Length must be greater than 0");
        return -1;
    }

    let _len = if _len % PAGE_SIZE != 0 {
        (_len + PAGE_SIZE - 1) / PAGE_SIZE * PAGE_SIZE
    } else {
        _len
    };

    let end_va = VirtAddr::from(_start + _len - 1);

    let mut perm = MapPermission::U;
    if _port & 0x1 != 0 {
        perm |= MapPermission::R;
    }
    if _port & 0x2 != 0 {
        perm |= MapPermission::W;
    }
    if _port & 0x4 != 0 {
        perm |= MapPermission::X;
    }

    if current_task_map(start_va, end_va, perm) == Ok(()) {
        0
    } else {
        error!("Out of memory or page allready mapped");
        -1
    }
}

/// free a memory region with `len` bytes
pub fn sys_munmap(_start: usize, mut _len: usize) -> isize {
    trace!("kernel: sys_munmap");
    while _len > 0 {
        if let Ok(cnt) = current_task_unmap(VirtAddr::from(_start)) {
            _len = if _len >= cnt {
                _len - cnt
            } else { 
                warn!("kernel: sys_munmap: _len({}) < cnt({}), this should not happen", _len, cnt);
                0
             }

        } else {
            return -1;
        }
    }
    0
}
/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}
