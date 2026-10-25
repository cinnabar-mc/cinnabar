//! A seccomp filter that leaves the helper the system calls a compiler and a sandboxed
//! runtime use and fails the rest of the kernel's attack surface with `EPERM`: running
//! programs, creating processes (threads remain), sockets, tracing or reading other
//! processes or changing their resource limits, namespaces and mounts, BPF, performance
//! counters, keys, io_uring, modules and system administration. A foreign architecture or the
//! x32 ABI kills the process.

use anyhow::{Result, ensure};
use libc::{
    BPF_ABS, BPF_JEQ, BPF_JMP, BPF_JSET, BPF_K, BPF_LD, BPF_RET, BPF_W, SECCOMP_RET_ALLOW,
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

/// The filter program for the process `own_pid`.
pub(super) fn program(own_pid: u32) -> Vec<sock_filter> {
    let deny = statement(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | libc::EPERM as u32);
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
    // prlimit64 may read and set only the helper's own limits: process 0 or its own id, never
    // the client's or another process of the same user. The kernel takes the id as a 32-bit
    // `pid_t`, so the low half of the argument is the whole of it.
    filter.extend([
        jump(BPF_JMP | BPF_JEQ | BPF_K, libc::SYS_prlimit64 as u32, 0, 5),
        statement(BPF_LD | BPF_W | BPF_ABS, ARG0_LOW),
        jump(BPF_JMP | BPF_JEQ | BPF_K, 0, 2, 0),
        jump(BPF_JMP | BPF_JEQ | BPF_K, own_pid, 1, 0),
        deny,
        statement(BPF_RET | BPF_K, SECCOMP_RET_ALLOW),
    ]);
    // clone3 reports ENOSYS so the C library falls back to clone, whose flags a filter can
    // read; clone may create threads but not processes.
    filter.extend([
        jump(BPF_JMP | BPF_JEQ | BPF_K, libc::SYS_clone3 as u32, 0, 1),
        statement(BPF_RET | BPF_K, SECCOMP_RET_ERRNO | libc::ENOSYS as u32),
        jump(BPF_JMP | BPF_JEQ | BPF_K, libc::SYS_clone as u32, 0, 3),
        statement(BPF_LD | BPF_W | BPF_ABS, ARG0_LOW),
        jump(BPF_JMP | BPF_JSET | BPF_K, libc::CLONE_THREAD as u32, 1, 0),
        deny,
        statement(BPF_RET | BPF_K, SECCOMP_RET_ALLOW),
    ]);
    filter
}

/// Installs the filter on every thread of the process.
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
