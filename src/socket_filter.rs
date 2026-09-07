//! Confined commands retain IP networking but cannot create host Unix endpoints.
use std::{fs::File, io::Write};

use anyhow::{Context, Result};
use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, memfd_create};
use seccompiler::{
    BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
    SeccompRule, sock_filter,
};

fn program() -> Result<BpfProgram> {
    let domain = || {
        SeccompCondition::new(
            0,
            SeccompCmpArgLen::Dword,
            SeccompCmpOp::Eq,
            libc::AF_UNIX as u64,
        )
    };
    // Removing only the two supported flags means unknown types fail closed.
    let pairs = [
        0,
        libc::SOCK_CLOEXEC,
        libc::SOCK_NONBLOCK,
        libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
    ]
    .into_iter()
    .map(|flags| {
        SeccompCondition::new(
            1,
            SeccompCmpArgLen::Dword,
            SeccompCmpOp::Ne,
            (libc::SOCK_STREAM | flags) as u64,
        )
    })
    .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut pair_conditions = vec![domain()?];
    pair_conditions.extend(pairs);
    let filter = SeccompFilter::new(
        [
            (libc::SYS_socket, vec![SeccompRule::new(vec![domain()?])?]),
            (
                libc::SYS_socketpair,
                vec![SeccompRule::new(pair_conditions)?],
            ),
            // io_uring can create sockets and connect without the socket syscall.
            (libc::SYS_io_uring_setup, vec![]),
        ]
        .into_iter()
        .collect(),
        SeccompAction::Allow,
        SeccompAction::Errno(libc::EPERM as u32),
        std::env::consts::ARCH.try_into()?,
    )?;
    let compiled: BpfProgram = filter.try_into()?;
    // x32 shares the x86_64 audit architecture but sets bit 30 in syscall IDs.
    // Reject that ABI before the compiler's native-architecture/rule checks.
    let mut result = if cfg!(target_arch = "x86_64") {
        vec![
            sock_filter {
                code: 0x20,
                jt: 0,
                jf: 0,
                k: 0,
            }, // LD syscall number
            sock_filter {
                code: 0x35,
                jt: 0,
                jf: 1,
                k: 0x4000_0000,
            }, // JGE x32
            sock_filter {
                code: 0x06,
                jt: 0,
                jf: 0,
                k: libc::SECCOMP_RET_KILL_PROCESS,
            },
        ]
    } else {
        Vec::new()
    };
    result.extend(compiled);
    Ok(result)
}

pub(crate) fn file() -> Result<File> {
    // No pathname remains for another command to rewrite between launches.
    let mut file = File::from(
        memfd_create(
            "demoncoder-socket-filter",
            MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
        )
        .context("create confined socket filter")?,
    );
    for instruction in program().context("compile confined socket filter")? {
        file.write_all(&instruction.code.to_ne_bytes())?;
        file.write_all(&[instruction.jt, instruction.jf])?;
        file.write_all(&instruction.k.to_ne_bytes())?;
    }
    fcntl_add_seals(
        &file,
        SealFlags::WRITE | SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL,
    )
    .context("seal confined socket filter")?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_filter_cannot_be_rewritten_or_truncated() {
        let mut policy = file().unwrap();
        assert_eq!(
            policy.write(b"replacement").unwrap_err().raw_os_error(),
            Some(libc::EPERM)
        );
        assert_eq!(
            policy.set_len(0).unwrap_err().raw_os_error(),
            Some(libc::EPERM)
        );
        let mut reopened = std::fs::OpenOptions::new()
            .write(true)
            .open(format!(
                "/proc/self/fd/{}",
                std::os::fd::AsRawFd::as_raw_fd(&policy)
            ))
            .unwrap();
        assert_eq!(
            reopened.write(b"replacement").unwrap_err().raw_os_error(),
            Some(libc::EPERM)
        );
    }

    // Evaluate the actual emitted BPF, including its architecture guards and
    // jumps. These cases catch incorrect argument width and compiler defaults.
    fn evaluate(arch: u32, syscall: u32, args: [u64; 6]) -> u32 {
        let mut data = [0u8; 64];
        data[..4].copy_from_slice(&syscall.to_ne_bytes());
        data[4..8].copy_from_slice(&arch.to_ne_bytes());
        for (i, arg) in args.iter().enumerate() {
            data[16 + 8 * i..24 + 8 * i].copy_from_slice(&arg.to_ne_bytes());
        }
        let program = program().unwrap();
        let (mut pc, mut value) = (0, 0);
        for _ in 0..4096 {
            let op = &program[pc];
            pc += 1;
            match op.code {
                0x20 => {
                    value = u32::from_ne_bytes(
                        data[op.k as usize..op.k as usize + 4].try_into().unwrap(),
                    )
                }
                0x54 => value &= op.k,
                0x05 => pc += op.k as usize,
                0x15 | 0x25 | 0x35 => {
                    let condition = match op.code {
                        0x15 => value == op.k,
                        0x25 => value > op.k,
                        _ => value >= op.k,
                    };
                    pc += usize::from(if condition { op.jt } else { op.jf });
                }
                0x06 => return op.k,
                code => panic!("unsupported instruction {code:x}"),
            }
        }
        panic!("filter did not terminate")
    }

    #[test]
    fn socket_filter_enforces_domains_types_and_abis() {
        let arch = match std::env::consts::ARCH {
            "x86_64" => 0xc000_003e,
            "aarch64" => 0xc000_00b7,
            "riscv64" => 0xc000_00f3,
            other => panic!("unsupported architecture {other}"),
        };
        let denied = libc::SECCOMP_RET_ERRNO | libc::EPERM as u32;
        for high in [0, 1u64 << 32, 0xffff_ffff_0000_0000] {
            for kind in [libc::SOCK_STREAM, libc::SOCK_DGRAM, libc::SOCK_SEQPACKET] {
                let args = [high | libc::AF_UNIX as u64, kind as u64, 0, 0, 0, 0];
                assert_eq!(evaluate(arch, libc::SYS_socket as u32, args), denied);
            }
        }
        for flags in [
            0,
            libc::SOCK_CLOEXEC,
            libc::SOCK_NONBLOCK,
            libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
        ] {
            for kind in [libc::SOCK_STREAM, libc::SOCK_DGRAM, libc::SOCK_SEQPACKET, 0] {
                let args = [libc::AF_UNIX as u64, (kind | flags) as u64, 0, 0, 0, 0];
                assert_eq!(
                    evaluate(arch, libc::SYS_socketpair as u32, args),
                    if kind == libc::SOCK_STREAM {
                        libc::SECCOMP_RET_ALLOW
                    } else {
                        denied
                    }
                );
            }
        }
        for domain in [libc::AF_INET, libc::AF_INET6] {
            assert_eq!(
                evaluate(
                    arch,
                    libc::SYS_socket as u32,
                    [domain as u64, 1, 0, 0, 0, 0]
                ),
                libc::SECCOMP_RET_ALLOW
            );
        }
        assert_eq!(
            evaluate(arch, libc::SYS_io_uring_setup as u32, [0; 6]),
            denied
        );
        for foreign in [0x4000_0003, 0, arch ^ 1] {
            assert_eq!(
                evaluate(foreign, libc::SYS_socket as u32, [0; 6]),
                libc::SECCOMP_RET_KILL_PROCESS
            );
        }
        if cfg!(target_arch = "x86_64") {
            for nr in [0x4000_0000, 0x4000_0029, 0x4000_0035, u32::MAX] {
                assert_eq!(evaluate(arch, nr, [0; 6]), libc::SECCOMP_RET_KILL_PROCESS);
            }
        }
    }
}
