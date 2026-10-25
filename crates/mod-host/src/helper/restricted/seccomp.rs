//! A seccomp filter that leaves the helper the system calls a compiler and a sandboxed
//! runtime use and fails the rest of the kernel's attack surface with `EPERM`: running
//! programs, creating processes (threads remain), sockets, tracing or reading other
//! processes, signalling, renicing, rescheduling or changing the resource limits of any
//! process but the helper itself, changing file modes, owners, times, extended attributes or
//! lengths (which Landlock does not cover, or covers only from ABI 3), System V IPC and POSIX
//! message queues, namespaces and mounts, BPF, performance counters, keys, io_uring, modules and
//! system administration. A foreign architecture or the x32 ABI kills
//! the process.

use anyhow::{Result, ensure};
use libc::{
    BPF_ABS, BPF_JA, BPF_JEQ, BPF_JMP, BPF_JSET, BPF_K, BPF_LD, BPF_RET, BPF_W, SECCOMP_RET_ALLOW,
    SECCOMP_RET_ERRNO, SECCOMP_RET_KILL_PROCESS, sock_filter, sock_fprog,
};

#[cfg(target_arch = "aarch64")]
const ARCH: u32 = 0xc000_00b7;
#[cfg(target_arch = "x86_64")]
const ARCH: u32 = 0xc000_003e;
/// System call numbers at or above this belong to the x32 ABI on x86-64.
#[cfg(target_arch = "x86_64")]
const X32_SYSCALL_BIT: u32 = 0x4000_0000;

/// `offsetof(struct seccomp_data, nr)`, `arch` and the low half of `args[0]`.
const NR: u32 = 0;
const ARCH_OFFSET: u32 = 4;
const ARG0_LOW: u32 = 16;

/// The offset of the low half of system call argument `index` in `struct seccomp_data` on a
/// little-endian target. The process ids, command numbers and selectors this filter reads are
/// 32-bit in the kernel, so the low half is the whole value.
const fn argument_low(index: u32) -> u32 {
    ARG0_LOW + 8 * index
}

/// `F_SETSIG` and `F_SETOWN_EX` (`asm-generic/fcntl.h`), which the C library crate lacks.
const F_SETSIG: u32 = 10;
const F_SETOWN_EX: u32 = 15;
/// `FIOSETOWN` and `SIOCSPGRP` (`asm-generic/sockios.h`).
const FIOSETOWN: u32 = 0x8901;
const SIOCSPGRP: u32 = 0x8902;
/// `IOPRIO_WHO_PROCESS` (`linux/ioprio.h`).
const IOPRIO_WHO_PROCESS: u32 = 1;

/// System calls the C library crate does not number on every architecture. They come from the
/// table every architecture shares for calls added since Linux 5.1.
const SYS_FCHMODAT2: libc::c_long = 452;
const SYS_SETXATTRAT: libc::c_long = 463;
const SYS_REMOVEXATTRAT: libc::c_long = 466;
#[cfg(target_arch = "x86_64")]
const _: () = assert!(SYS_FCHMODAT2 == libc::SYS_fchmodat2);

/// The system calls the helper is refused on every architecture.
const DENIED: &[libc::c_long] = &[
    libc::SYS_execve,
    libc::SYS_execveat,
    libc::SYS_socket,
    libc::SYS_socketpair,
    libc::SYS_connect,
    libc::SYS_bind,
    libc::SYS_listen,
    libc::SYS_accept,
    libc::SYS_accept4,
    libc::SYS_ptrace,
    libc::SYS_process_vm_readv,
    libc::SYS_process_vm_writev,
    libc::SYS_kcmp,
    libc::SYS_pidfd_getfd,
    // Signals and process handles: a thread id may name any thread of the user, and a
    // process descriptor any of its processes.
    libc::SYS_tkill,
    libc::SYS_pidfd_open,
    libc::SYS_pidfd_send_signal,
    libc::SYS_process_madvise,
    libc::SYS_process_mrelease,
    // File modes, owners, times, extended attributes and lengths, by path or descriptor:
    // Landlock grants no file, but it does not cover these calls on a path, and covers
    // truncation only from ABI 3. The runtime's own memory files need none of them.
    libc::SYS_fchmod,
    libc::SYS_fchmodat,
    SYS_FCHMODAT2,
    libc::SYS_fchown,
    libc::SYS_fchownat,
    libc::SYS_utimensat,
    libc::SYS_setxattr,
    libc::SYS_lsetxattr,
    libc::SYS_fsetxattr,
    SYS_SETXATTRAT,
    libc::SYS_removexattr,
    libc::SYS_lremovexattr,
    libc::SYS_fremovexattr,
    SYS_REMOVEXATTRAT,
    libc::SYS_truncate,
    libc::SYS_ftruncate,
    // System V IPC and POSIX message queues: objects any process of the user may share.
    libc::SYS_shmget,
    libc::SYS_shmat,
    libc::SYS_shmctl,
    libc::SYS_shmdt,
    libc::SYS_semget,
    libc::SYS_semop,
    libc::SYS_semtimedop,
    libc::SYS_semctl,
    libc::SYS_msgget,
    libc::SYS_msgsnd,
    libc::SYS_msgrcv,
    libc::SYS_msgctl,
    libc::SYS_mq_open,
    libc::SYS_mq_unlink,
    libc::SYS_mq_timedsend,
    libc::SYS_mq_timedreceive,
    libc::SYS_mq_notify,
    libc::SYS_mq_getsetattr,
    libc::SYS_mount,
    libc::SYS_umount2,
    libc::SYS_pivot_root,
    libc::SYS_chroot,
    libc::SYS_unshare,
    libc::SYS_setns,
    libc::SYS_open_tree,
    libc::SYS_move_mount,
    libc::SYS_fsopen,
    libc::SYS_fsconfig,
    libc::SYS_fsmount,
    libc::SYS_fspick,
    libc::SYS_mount_setattr,
    libc::SYS_bpf,
    libc::SYS_perf_event_open,
    libc::SYS_keyctl,
    libc::SYS_add_key,
    libc::SYS_request_key,
    libc::SYS_userfaultfd,
    libc::SYS_io_uring_setup,
    libc::SYS_io_uring_enter,
    libc::SYS_io_uring_register,
    libc::SYS_open_by_handle_at,
    libc::SYS_name_to_handle_at,
    libc::SYS_init_module,
    libc::SYS_finit_module,
    libc::SYS_delete_module,
    libc::SYS_kexec_load,
    libc::SYS_kexec_file_load,
    libc::SYS_reboot,
    libc::SYS_swapon,
    libc::SYS_swapoff,
    libc::SYS_syslog,
    libc::SYS_acct,
    libc::SYS_personality,
    libc::SYS_quotactl,
    libc::SYS_fanotify_init,
];

/// The x86-64-only system calls the helper is refused.
#[cfg(target_arch = "x86_64")]
const DENIED_ARCH: &[libc::c_long] = &[
    libc::SYS_fork,
    libc::SYS_vfork,
    libc::SYS_uselib,
    libc::SYS_ioperm,
    libc::SYS_iopl,
    libc::SYS_chmod,
    libc::SYS_chown,
    libc::SYS_lchown,
    libc::SYS_utime,
    libc::SYS_utimes,
    libc::SYS_futimesat,
];
#[cfg(not(target_arch = "x86_64"))]
const DENIED_ARCH: &[libc::c_long] = &[];

/// A BPF statement.
const fn statement(code: u32, k: u32) -> sock_filter {
    sock_filter {
        code: code as u16,
        jt: 0,
        jf: 0,
        k,
    }
}

/// A BPF jump: `jt` or `jf` instructions forward.
const fn jump(code: u32, k: u32, jt: u8, jf: u8) -> sock_filter {
    sock_filter {
        code: code as u16,
        jt,
        jf,
        k,
    }
}

/// A jump distance within one filter block.
fn offset(instructions: usize) -> u8 {
    u8::try_from(instructions).expect("a filter block spans fewer than 256 instructions")
}

/// The block that allows `call` only when, for every `(argument, values)` condition, the low
/// half of that argument holds one of `values`, and fails it with `EPERM` otherwise. Any other
/// call passes through with its number still loaded.
fn allow_only(call: libc::c_long, conditions: &[(u32, &[u32])]) -> Vec<sock_filter> {
    let mut checks = Vec::new();
    // Instructions between the end of a check and the refusal: the later checks and the allow.
    let mut to_deny = 1;
    for &(argument, values) in conditions.iter().rev() {
        let mut check = vec![statement(BPF_LD | BPF_W | BPF_ABS, argument_low(argument))];
        for (position, &value) in values.iter().enumerate() {
            // A match skips the remaining values and the jump to the refusal.
            let skip = offset(values.len() - position);
            check.push(jump(BPF_JMP | BPF_JEQ | BPF_K, value, skip, 0));
        }
        check.push(statement(BPF_JMP | BPF_JA | BPF_K, to_deny));
        to_deny += check.len() as u32;
        checks.splice(0..0, check);
    }
    let mut block = vec![jump(
        BPF_JMP | BPF_JEQ | BPF_K,
        call as u32,
        0,
        offset(checks.len() + 2),
    )];
    block.extend(checks);
    block.extend([allow(), deny()]);
    block
}

/// The block that fails `call` with `EPERM` when the low half of `argument` holds one of
/// `values` and allows it otherwise. Any other call passes through with its number still
/// loaded.
fn deny_values(call: libc::c_long, argument: u32, values: &[u32]) -> Vec<sock_filter> {
    let mut block = vec![
        jump(
            BPF_JMP | BPF_JEQ | BPF_K,
            call as u32,
            0,
            offset(values.len() + 3),
        ),
        statement(BPF_LD | BPF_W | BPF_ABS, argument_low(argument)),
    ];
    for (position, &value) in values.iter().enumerate() {
        // A match skips the remaining values and the allow.
        let skip = offset(values.len() - position);
        block.push(jump(BPF_JMP | BPF_JEQ | BPF_K, value, skip, 0));
    }
    block.extend([allow(), deny()]);
    block
}

/// The verdict that allows a call.
const fn allow() -> sock_filter {
    statement(BPF_RET | BPF_K, SECCOMP_RET_ALLOW)
}

/// The verdict that fails a call with `EPERM`.
const fn deny() -> sock_filter {
    statement(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | libc::EPERM as u32)
}

/// The filter program for the process `own_pid`.
pub(super) fn program(own_pid: u32) -> Vec<sock_filter> {
    let deny = deny();
    let mut filter = vec![
        statement(BPF_LD | BPF_W | BPF_ABS, ARCH_OFFSET),
        jump(BPF_JMP | BPF_JEQ | BPF_K, ARCH, 1, 0),
        statement(BPF_RET | BPF_K, SECCOMP_RET_KILL_PROCESS),
        statement(BPF_LD | BPF_W | BPF_ABS, NR),
    ];
    #[cfg(target_arch = "x86_64")]
    filter.extend([
        jump(BPF_JMP | libc::BPF_JGE | BPF_K, X32_SYSCALL_BIT, 0, 1),
        statement(BPF_RET | BPF_K, SECCOMP_RET_KILL_PROCESS),
    ]);
    for &call in DENIED.iter().chain(DENIED_ARCH) {
        filter.extend([jump(BPF_JMP | BPF_JEQ | BPF_K, call as u32, 0, 1), deny]);
    }
    // Signals reach only the helper's own process and threads: process 0 names its process
    // group, which holds the client, a negative id another group or every process, and any
    // other id a process of the same user.
    let own: &[u32] = &[own_pid];
    // Limits, scheduling, priority and memory placement apply only to the helper: 0 names the
    // calling thread or process.
    let itself: &[u32] = &[0, own_pid];
    for call in [
        libc::SYS_kill,
        libc::SYS_tgkill,
        libc::SYS_rt_sigqueueinfo,
        libc::SYS_rt_tgsigqueueinfo,
    ] {
        filter.extend(allow_only(call, &[(0, own)]));
    }
    for call in [
        libc::SYS_prlimit64,
        libc::SYS_sched_setaffinity,
        libc::SYS_sched_setscheduler,
        libc::SYS_sched_setparam,
        libc::SYS_sched_setattr,
        libc::SYS_migrate_pages,
        libc::SYS_move_pages,
    ] {
        filter.extend(allow_only(call, &[(0, itself)]));
    }
    // Priorities only by process, not by process group or user.
    let by_process: &[u32] = &[libc::PRIO_PROCESS as u32];
    filter.extend(allow_only(
        libc::SYS_setpriority,
        &[(0, by_process), (1, itself)],
    ));
    let io_by_process: &[u32] = &[IOPRIO_WHO_PROCESS];
    filter.extend(allow_only(
        libc::SYS_ioprio_set,
        &[(0, io_by_process), (1, itself)],
    ));
    // A descriptor's owner receives its I/O signals, of any number `F_SETSIG` picks, so the
    // helper may not name another owner.
    filter.extend(deny_values(
        libc::SYS_fcntl,
        1,
        &[libc::F_SETOWN as u32, F_SETOWN_EX, F_SETSIG],
    ));
    filter.extend(deny_values(libc::SYS_ioctl, 1, &[FIOSETOWN, SIOCSPGRP]));
    // clone3 reports ENOSYS so the C library falls back to clone, whose flags a filter can
    // read; clone may create threads but not processes.
    filter.extend([
        jump(BPF_JMP | BPF_JEQ | BPF_K, libc::SYS_clone3 as u32, 0, 1),
        statement(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | libc::ENOSYS as u32),
        jump(BPF_JMP | BPF_JEQ | BPF_K, libc::SYS_clone as u32, 0, 3),
        statement(BPF_LD | BPF_W | BPF_ABS, ARG0_LOW),
        jump(BPF_JMP | BPF_JSET | BPF_K, libc::CLONE_THREAD as u32, 1, 0),
        deny,
        allow(),
    ]);
    filter
}

/// Installs the filter on every thread of the process. Seccomp filters pass to every thread
/// and child a process creates and survive `exec`, so the helper itself and any child it might
/// have stay under the filter.
pub(super) fn install() -> Result<()> {
    let filter = program(std::process::id());
    let program = sock_fprog {
        len: u16::try_from(filter.len())?,
        filter: filter.as_ptr().cast_mut(),
    };
    // SAFETY: `program` points at `filter`, which outlives the call.
    let status = unsafe {
        libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0);
        libc::syscall(
            libc::SYS_seccomp,
            libc::SECCOMP_SET_MODE_FILTER,
            libc::SECCOMP_FILTER_FLAG_TSYNC,
            &program as *const sock_fprog,
        )
    };
    ensure!(
        status == 0,
        "filter refused: {}",
        std::io::Error::last_os_error()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The helper's process id in these tests, and its parent's.
    const OWN: u32 = 4242;
    const PARENT: u32 = 4241;
    const ALLOW: u32 = SECCOMP_RET_ALLOW;
    const EPERM: u32 = SECCOMP_RET_ERRNO | libc::EPERM as u32;

    /// Runs `filter` as the kernel does on system call `call` with `args` from the native
    /// architecture and returns its verdict.
    fn verdict(filter: &[sock_filter], call: libc::c_long, args: [u64; 6]) -> u32 {
        verdict_on(filter, ARCH, call, args)
    }

    /// [`verdict`] for a call made through the ABI of audit architecture `arch`.
    fn verdict_on(filter: &[sock_filter], arch: u32, call: libc::c_long, args: [u64; 6]) -> u32 {
        let mut data = [0u8; 64];
        data[0..4].copy_from_slice(&(call as u32).to_le_bytes());
        data[4..8].copy_from_slice(&arch.to_le_bytes());
        for (index, arg) in args.iter().enumerate() {
            let at = ARG0_LOW as usize + 8 * index;
            data[at..at + 8].copy_from_slice(&arg.to_le_bytes());
        }
        let mut accumulator = 0u32;
        let mut next = 0usize;
        loop {
            let op = filter[next];
            next += 1;
            let code = u32::from(op.code);
            if code == BPF_LD | BPF_W | BPF_ABS {
                let at = op.k as usize;
                accumulator = u32::from_le_bytes(data[at..at + 4].try_into().unwrap());
            } else if code == BPF_RET | BPF_K {
                return op.k;
            } else if code == BPF_JMP | BPF_JA | BPF_K {
                next += op.k as usize;
            } else {
                let taken = if code == BPF_JMP | BPF_JEQ | BPF_K {
                    accumulator == op.k
                } else if code == BPF_JMP | libc::BPF_JGE | BPF_K {
                    accumulator >= op.k
                } else if code == BPF_JMP | BPF_JSET | BPF_K {
                    accumulator & op.k != 0
                } else {
                    panic!("unexpected instruction {code:#x}")
                };
                next += usize::from(if taken { op.jt } else { op.jf });
            }
        }
    }

    /// The verdict of the helper's filter on `call` with leading arguments `args`.
    fn helper(call: libc::c_long, args: &[u64]) -> u32 {
        let mut all = [0; 6];
        all[..args.len()].copy_from_slice(args);
        verdict(&program(OWN), call, all)
    }

    #[test]
    fn signals_reach_only_the_helper_itself() {
        let (own, parent) = (u64::from(OWN), u64::from(PARENT));
        let signal = libc::SIGCONT as u64;
        assert_eq!(helper(libc::SYS_kill, &[own, signal]), ALLOW);
        assert_eq!(helper(libc::SYS_kill, &[own, 0]), ALLOW);
        // The parent, the helper's own process group, every process and the parent's group.
        for target in [parent, 0, u64::MAX, (-i64::from(PARENT)) as u64] {
            assert_eq!(
                helper(libc::SYS_kill, &[target, signal]),
                EPERM,
                "{target:#x}"
            );
        }
        for call in [libc::SYS_tgkill, libc::SYS_rt_tgsigqueueinfo] {
            assert_eq!(helper(call, &[own, own + 1, signal]), ALLOW);
            assert_eq!(helper(call, &[parent, parent, signal]), EPERM);
        }
        assert_eq!(helper(libc::SYS_rt_sigqueueinfo, &[own, signal]), ALLOW);
        assert_eq!(helper(libc::SYS_rt_sigqueueinfo, &[parent, signal]), EPERM);
        assert_eq!(helper(libc::SYS_tkill, &[own, signal]), EPERM);
        assert_eq!(helper(libc::SYS_pidfd_open, &[parent]), EPERM);
        assert_eq!(helper(libc::SYS_pidfd_send_signal, &[3, signal]), EPERM);
    }

    #[test]
    fn limits_scheduling_and_priorities_apply_only_to_the_helper() {
        let (own, parent) = (u64::from(OWN), u64::from(PARENT));
        for call in [
            libc::SYS_prlimit64,
            libc::SYS_sched_setaffinity,
            libc::SYS_sched_setscheduler,
            libc::SYS_sched_setparam,
            libc::SYS_sched_setattr,
            libc::SYS_migrate_pages,
            libc::SYS_move_pages,
        ] {
            assert_eq!(helper(call, &[0]), ALLOW, "{call}");
            assert_eq!(helper(call, &[own]), ALLOW, "{call}");
            assert_eq!(helper(call, &[parent]), EPERM, "{call}");
        }
        let process = libc::PRIO_PROCESS as u64;
        assert_eq!(helper(libc::SYS_setpriority, &[process, 0, 10]), ALLOW);
        assert_eq!(helper(libc::SYS_setpriority, &[process, own, 10]), ALLOW);
        assert_eq!(helper(libc::SYS_setpriority, &[process, parent, 10]), EPERM);
        for which in [libc::PRIO_PGRP, libc::PRIO_USER] {
            assert_eq!(helper(libc::SYS_setpriority, &[which as u64, 0, 10]), EPERM);
        }
        let io_process = u64::from(IOPRIO_WHO_PROCESS);
        assert_eq!(helper(libc::SYS_ioprio_set, &[io_process, 0, 0]), ALLOW);
        assert_eq!(
            helper(libc::SYS_ioprio_set, &[io_process, parent, 0]),
            EPERM
        );
        assert_eq!(helper(libc::SYS_ioprio_set, &[io_process + 2, 0, 0]), EPERM);
    }

    #[test]
    fn descriptors_take_no_other_signal_owner() {
        let parent = u64::from(PARENT);
        for command in [libc::F_SETOWN as u32, F_SETOWN_EX, F_SETSIG] {
            assert_eq!(
                helper(libc::SYS_fcntl, &[1, u64::from(command), parent]),
                EPERM
            );
        }
        assert_eq!(helper(libc::SYS_fcntl, &[1, libc::F_GETFD as u64]), ALLOW);
        for command in [FIOSETOWN, SIOCSPGRP] {
            assert_eq!(helper(libc::SYS_ioctl, &[1, u64::from(command)]), EPERM);
        }
        // `FIOGETOWN` only reads the owner.
        assert_eq!(
            helper(libc::SYS_ioctl, &[1, u64::from(SIOCSPGRP + 1)]),
            ALLOW
        );
    }

    /// Every call that changes a file's mode, owner, times, extended attributes or length fails,
    /// by path or by descriptor.
    #[test]
    fn file_attribute_changes_are_refused() {
        #[cfg(target_arch = "x86_64")]
        let legacy: &[libc::c_long] = &[
            libc::SYS_chmod,
            libc::SYS_chown,
            libc::SYS_lchown,
            libc::SYS_utime,
            libc::SYS_utimes,
            libc::SYS_futimesat,
        ];
        #[cfg(not(target_arch = "x86_64"))]
        let legacy: &[libc::c_long] = &[];
        let calls = [
            libc::SYS_fchmod,
            libc::SYS_fchmodat,
            SYS_FCHMODAT2,
            libc::SYS_fchown,
            libc::SYS_fchownat,
            libc::SYS_utimensat,
            libc::SYS_setxattr,
            libc::SYS_lsetxattr,
            libc::SYS_fsetxattr,
            SYS_SETXATTRAT,
            libc::SYS_removexattr,
            libc::SYS_lremovexattr,
            libc::SYS_fremovexattr,
            SYS_REMOVEXATTRAT,
            libc::SYS_truncate,
            libc::SYS_ftruncate,
        ];
        for &call in calls.iter().chain(legacy) {
            assert_eq!(helper(call, &[0, 0, 0]), EPERM, "{call}");
        }
    }

    /// System V shared memory, semaphores and message queues and POSIX message queues are
    /// unreachable.
    #[test]
    fn ipc_objects_are_refused() {
        for call in [
            libc::SYS_shmget,
            libc::SYS_shmat,
            libc::SYS_shmctl,
            libc::SYS_shmdt,
            libc::SYS_semget,
            libc::SYS_semop,
            libc::SYS_semtimedop,
            libc::SYS_semctl,
            libc::SYS_msgget,
            libc::SYS_msgsnd,
            libc::SYS_msgrcv,
            libc::SYS_msgctl,
            libc::SYS_mq_open,
            libc::SYS_mq_unlink,
            libc::SYS_mq_timedsend,
            libc::SYS_mq_timedreceive,
            libc::SYS_mq_notify,
            libc::SYS_mq_getsetattr,
        ] {
            assert_eq!(helper(call, &[0, 0, 0]), EPERM, "{call}");
        }
    }

    /// A call through a 32-bit compatibility ABI, or through x32 on x86-64, kills the process
    /// whatever its number, so no call escapes the native table the filter checks.
    #[test]
    fn foreign_abis_kill_the_process() {
        /// `AUDIT_ARCH_I386` and `AUDIT_ARCH_ARM`, the compatibility ABIs.
        #[cfg(target_arch = "x86_64")]
        const COMPAT: u32 = 0x4000_0003;
        #[cfg(target_arch = "aarch64")]
        const COMPAT: u32 = 0x4000_0028;
        let filter = program(OWN);
        for call in [libc::SYS_read, libc::SYS_getpid, libc::SYS_execve] {
            assert_eq!(
                verdict_on(&filter, COMPAT, call, [0; 6]),
                SECCOMP_RET_KILL_PROCESS,
                "{call}"
            );
        }
        #[cfg(target_arch = "x86_64")]
        for call in [libc::SYS_read, libc::SYS_execve] {
            let x32 = call | X32_SYSCALL_BIT as libc::c_long;
            assert_eq!(verdict(&filter, x32, [0; 6]), SECCOMP_RET_KILL_PROCESS);
        }
    }

    #[test]
    fn other_calls_keep_their_verdicts() {
        assert_eq!(helper(libc::SYS_read, &[0]), ALLOW);
        assert_eq!(helper(libc::SYS_execve, &[0]), EPERM);
        assert_eq!(helper(libc::SYS_ptrace, &[0]), EPERM);
        assert_eq!(helper(libc::SYS_fstat, &[0]), ALLOW);
        let thread = libc::CLONE_THREAD as u64;
        assert_eq!(helper(libc::SYS_clone, &[thread]), ALLOW);
        assert_eq!(helper(libc::SYS_clone, &[0]), EPERM);
        assert_eq!(
            helper(libc::SYS_clone3, &[0]),
            SECCOMP_RET_ERRNO | libc::ENOSYS as u32
        );
    }
}
