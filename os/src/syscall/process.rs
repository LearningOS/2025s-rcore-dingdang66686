//! Process management syscalls
use alloc::sync::Arc;

use crate::{
    config::PAGE_SIZE, loader::get_app_data_by_name, mm::{copy_to_user, translated_refmut, translated_str, MapPermission, VirtAddr}, task::{
        add_task, current_task, current_user_token, exit_current_and_run_next,
        suspend_current_and_run_next, TaskControlBlock,
    }, timer::get_time_us
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(exit_code: i32) -> ! {
    trace!("kernel:pid[{}] sys_exit", current_task().unwrap().pid.0);
    exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel:pid[{}] sys_yield", current_task().unwrap().pid.0);
    suspend_current_and_run_next();
    0
}

pub fn sys_getpid() -> isize {
    trace!("kernel: sys_getpid pid:{}", current_task().unwrap().pid.0);
    current_task().unwrap().pid.0 as isize
}

pub fn sys_fork() -> isize {
    trace!("kernel:pid[{}] sys_fork", current_task().unwrap().pid.0);
    let current_task = current_task().unwrap();
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;
    // modify trap context of new_task, because it returns immediately after switching
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    // we do not have to move to next instruction since we have done it before
    // for child process, fork returns 0
    trap_cx.x[10] = 0;
    // add new task to scheduler
    add_task(new_task);
    new_pid as isize
}

pub fn sys_exec(path: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_exec", current_task().unwrap().pid.0);
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let task = current_task().unwrap();
        task.exec(data);
        0
    } else {
        -1
    }
}

/// If there is not a child process whose pid is same as given, return -1.
/// Else if there is a child process but it is still running, return -2.
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    trace!("kernel::pid[{}] sys_waitpid [{}]", current_task().unwrap().pid.0, pid);
    let task = current_task().unwrap();
    // find a child process

    // ---- access current PCB exclusively
    let mut inner = task.inner_exclusive_access();
    if !inner
        .children
        .iter()
        .any(|p| pid == -1 || pid as usize == p.getpid())
    {
        return -1;
        // ---- release current PCB
    }
    let pair = inner.children.iter().enumerate().find(|(_, p)| {
        // ++++ temporarily access child PCB exclusively
        p.inner_exclusive_access().is_zombie() && (pid == -1 || pid as usize == p.getpid())
        // ++++ release child PCB
    });
    if let Some((idx, _)) = pair {
        let child = inner.children.remove(idx);
        // confirm that child will be deallocated after being removed from children list
        assert_eq!(Arc::strong_count(&child), 1);
        let found_pid = child.getpid();
        // ++++ temporarily access child PCB exclusively
        let exit_code = child.inner_exclusive_access().exit_code;
        // ++++ release child PCB
        *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code;
        found_pid as isize
    } else {
        -2
    }
    // ---- release current PCB automatically
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    trace!(
        "kernel:pid[{}] sys_get_time",
        current_task().unwrap().pid.0
    );
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

/// YOUR JOB: Implement mmap.
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

    match current_task()
        .unwrap()
        .inner_exclusive_access()
        .memory_set
        .map_area(start_va, end_va, perm) {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

/// YOUR JOB: Implement munmap.
pub fn sys_munmap(_start: usize, mut _len: usize) -> isize {
    fn current_task_unmap(ptr: VirtAddr) -> Result<usize, ()> {
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .memory_set
            .unmap_area(ptr)
    }
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
    trace!("kernel:pid[{}] sys_sbrk", current_task().unwrap().pid.0);
    if let Some(old_brk) = current_task().unwrap().change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}

/// YOUR JOB: Implement spawn.
/// HINT: fork + exec =/= spawn
pub fn sys_spawn(_path: *const u8) -> isize {
    trace!(
        "kernel:pid[{}] sys_spawn",
        current_task().unwrap().pid.0
    );
    let token = current_user_token();
    let path = translated_str(token, _path);
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let task = Arc::new(TaskControlBlock::new(data));
        let pid = task.pid.0;
        task.exec(data);
        task.inner_exclusive_access().parent = Some(Arc::downgrade(&current_task().unwrap()));
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .children
            .push(task.clone());
        add_task(task);
        pid as isize
    } else {
        -1
    }
}

// YOUR JOB: Set task priority.
pub fn sys_set_priority(_prio: isize) -> isize {
    trace!(
        "kernel:pid[{}] sys_set_priority",
        current_task().unwrap().pid.0
    );
    if _prio < 2 || _prio > 256 { return -1; }
    current_task().unwrap().inner_exclusive_access().prio = _prio as usize;
    _prio
}
