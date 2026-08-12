# D3 gate-runner resource measurement

Date: 2026-08-08

Status: runner sizing accepted for the final exact-tree gate; this measurement
is not the D3 release certificate.

## Boundary

The measured command was the then-fixed Gate 2 command. In the current
31-command manifest it is Gate 3, after the tracked-secret scan and formatting:

```text
cargo build --locked --workspace --all-targets
```

It ran from a detached clean worktree at `22b6286624e67e2c8bb95d38431a27958ea4e551`
in the pinned arm64 gate image
`sha256:a5be6637ef180ad0abdafb84c15be41482f83a203d2906c33fdf0756c13c1b19`.
Each attempt used a fresh tmpfs Cargo target, one Cargo build job, a 14 GiB
container memory and memory-plus-swap limit, and
`CARGO_PROFILE_DEV_DEBUG=0` plus `CARGO_PROFILE_TEST_DEBUG=0`. The profiles
retain debug assertions and omit DWARF from dev and test artifacts.

Memory and target bytes were sampled from inside the running container at
approximately one-second intervals. The monitor read cgroup
`memory.current` and the apparent byte size of `/scratch/target`. The sampling
process itself ran in the same container cgroup.

## Failed sizing probes

| Cargo jobs | Target cap | Result | Elapsed | Confirmed cause |
| ---: | ---: | --- | ---: | --- |
| 4 | 10 GiB | exit 101 | 131.8 s | memcg killed a linker child |
| 2 | 10 GiB | exit 101 | 194.3 s | memcg killed a linker child |
| 1 | 10 GiB | exit 101 | 351.9 s | linker failed with `ENOSPC` in `/scratch/target` |

Docker reported `OOMKilled=false` for the aggregate-memory failures because
the shell and Cargo parent survived while a linker child was killed. The final
runner therefore checks both Docker's PID 1 OOM state and the cgroup
`memory.events` `oom_kill` delta.

## Accepted cold runs

| Run | Exit | Samples | Elapsed | Peak memory | Peak target |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 0 | 323 | 341.669 s | 5,301,723,136 B / 4.938 GiB | 3,979,094,450 B / 3.706 GiB |
| 2 | 0 | 339 | 358.999 s | 5,283,438,592 B / 4.921 GiB | 3,975,939,665 B / 3.703 GiB |
| 3 | 0 | 350 | 371.022 s | 5,295,955,968 B / 4.932 GiB | 3,977,133,219 B / 3.704 GiB |

All three cold runs completed without a cgroup OOM event. Peak target usage
varied by less than 3.2 MiB and remained below 3.71 GiB. Peak memory remained
below 4.94 GiB.

## Decision

The final per-attempt target cap is 8 GiB. The largest observed target leaves
more than 4.29 GiB below that cap, while keeping the target in tmpfs prevents a
Docker sparse disk from consuming unbounded host storage. The container remains
at 14 GiB with swap disabled and one Cargo job. A disk-backed target, a 12 GiB
tmpfs, and a 16 GiB container were rejected because this Mac's Colima VM has an
18 GiB allocation and the host free-space boundary is tighter than the sparse
VM disk reports.

The final exact-tree D3 run must still pass all 36 gates, including the
tracked-secret scan, D3 self-tests, D2A Python and Node tests, and D2A issuer
format, test, and Clippy gates, with the 8 GiB policy.
