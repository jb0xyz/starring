import os
import pathlib
import stat


SANDBOX_EXECUTABLE = pathlib.Path("/usr/bin/sandbox-exec")
SANDBOX_PROFILE = " ".join(
    (
        "(version 1)",
        "(deny default)",
        "(allow process-fork process-exec*)",
        "(allow signal (target same-sandbox))",
        "(allow sysctl-read)",
        "(allow file-read-metadata)",
        "(allow file-read*",
        '(literal "/")',
        '(subpath (param "STARRING_MUTABLE_ROOT"))',
        '(subpath (param "STARRING_WORKTREE"))',
        '(subpath (param "STARRING_RUST_SYSROOT"))',
        '(subpath "/System")',
        '(subpath "/Library/Developer/CommandLineTools")',
        '(subpath "/usr")',
        '(subpath "/bin")',
        '(subpath "/sbin")',
        '(subpath "/opt/homebrew")',
        '(subpath "/usr/local")',
        '(subpath "/private/etc")',
        '(subpath "/private/var/db/timezone")',
        '(subpath "/private/var/select")',
        '(subpath "/var/select")',
        '(subpath "/dev")',
        ")",
        "(allow file-write*",
        '(subpath (param "STARRING_MUTABLE_ROOT"))',
        '(literal "/dev/null")',
        ")",
        "(deny file-link)",
        "(deny file-clone)",
        "(deny mach-priv-task-port)",
        "(allow mach-lookup",
        '(global-name "com.apple.SystemConfiguration.configd")',
        '(global-name "com.apple.system.notification_center")',
        '(global-name "com.apple.system.opendirectoryd.libinfo")',
        '(global-name "com.apple.system.opendirectoryd.membership")',
        '(global-name "com.apple.system.opendirectoryd.api")',
        '(global-name "com.apple.logd")',
        ")",
    )
)
EXTERNAL_NETWORK_PROFILE = " ".join(
    (
        "(allow network-outbound",
        "(require-all",
        "(require-any",
        '(remote tcp "*:*")',
        '(remote udp "*:*")',
        '(literal "/private/var/run/mDNSResponder")',
        ")",
        '(require-not (remote ip "localhost:*"))',
        ")",
        ")",
        "(allow mach-lookup",
        '(global-name "com.apple.SystemConfiguration.configd")',
        '(global-name "com.apple.system.opendirectoryd.membership")',
        '(global-name "com.apple.bsd.dirhelper")',
        ")",
        '(deny network-outbound (remote ip "localhost:*"))',
    )
)
LOCAL_NETWORK_PROFILE = " ".join(
    (
        '(allow network-outbound (remote tcp (param "STARRING_LOCAL_TCP_ENDPOINT")))',
    )
)


class ProcessSandboxError(Exception):
    pass


def fail(code):
    raise ProcessSandboxError(code)


def canonical_directory(value, label):
    candidate = pathlib.Path(value)
    if not candidate.is_absolute() or "\x00" in str(candidate):
        fail(f"{label}_invalid")
    try:
        resolved = candidate.resolve(strict=True)
        metadata = resolved.lstat()
    except OSError as error:
        fail(f"{label}_unavailable:{error.__class__.__name__}")
    if (
        resolved != candidate
        or not stat.S_ISDIR(metadata.st_mode)
        or resolved.is_symlink()
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) & 0o022
    ):
        fail(f"{label}_invalid")
    return resolved


def require_sandbox_executable():
    try:
        metadata = SANDBOX_EXECUTABLE.lstat()
    except OSError as error:
        fail(f"process_sandbox_unavailable:{error.__class__.__name__}")
    if (
        not stat.S_ISREG(metadata.st_mode)
        or SANDBOX_EXECUTABLE.is_symlink()
        or metadata.st_uid != 0
        or stat.S_IMODE(metadata.st_mode) & 0o022
        or not stat.S_IMODE(metadata.st_mode) & 0o111
    ):
        fail("process_sandbox_identity_invalid")
    return SANDBOX_EXECUTABLE


def rust_sysroot_from_environment(environment):
    rustc = environment.get("RUSTC")
    if not isinstance(rustc, str):
        fail("process_sandbox_rustc_missing")
    rustc_path = pathlib.Path(rustc)
    if not rustc_path.is_absolute() or rustc_path.name != "rustc":
        fail("process_sandbox_rustc_invalid")
    return canonical_directory(
        rustc_path.parent.parent, "process_sandbox_rust_sysroot"
    )


def sandboxed_argv(
    argv,
    mutable_root,
    worktree,
    environment,
    network_scope="none",
    additional_mutable_roots=(),
    additional_read_roots=(),
    local_tcp_port=None,
):
    if (
        not isinstance(argv, list)
        or not argv
        or any(not isinstance(value, str) or not value for value in argv)
        or network_scope not in {"none", "external", "local"}
        or not isinstance(additional_mutable_roots, (tuple, list))
        or len(additional_mutable_roots) > 4
        or not isinstance(additional_read_roots, (tuple, list))
        or len(additional_read_roots) > 4
    ):
        fail("process_sandbox_arguments_invalid")
    mutable = canonical_directory(mutable_root, "process_sandbox_mutable_root")
    source = canonical_directory(worktree, "process_sandbox_worktree")
    rust_sysroot = rust_sysroot_from_environment(environment)
    if mutable == source or mutable in source.parents or source in mutable.parents:
        fail("process_sandbox_root_overlap")
    additional = [
        canonical_directory(value, f"process_sandbox_mutable_root_{index}")
        for index, value in enumerate(additional_mutable_roots)
    ]
    if len({mutable, *additional}) != len(additional) + 1:
        fail("process_sandbox_root_overlap")
    if any(
        root == source or root in source.parents
        for root in additional
    ):
        fail("process_sandbox_root_overlap")
    read_roots = [
        canonical_directory(value, f"process_sandbox_read_root_{index}")
        for index, value in enumerate(additional_read_roots)
    ]
    if len(set(read_roots)) != len(read_roots):
        fail("process_sandbox_root_overlap")
    profile = SANDBOX_PROFILE
    definitions = []
    for index, root in enumerate(additional):
        name = f"STARRING_MUTABLE_ROOT_{index}"
        definitions.extend(("-D", f"{name}={root}"))
        profile = (
            f'{profile} (allow file-write* (subpath (param "{name}")))'
        )
    for index, root in enumerate(read_roots):
        name = f"STARRING_READ_ROOT_{index}"
        definitions.extend(("-D", f"{name}={root}"))
        profile = f'{profile} (allow file-read* (subpath (param "{name}")))'
    if network_scope == "external":
        profile = f"{profile} {EXTERNAL_NETWORK_PROFILE}"
    elif network_scope == "local":
        if type(local_tcp_port) is not int or not 1 <= local_tcp_port <= 65535:
            fail("process_sandbox_local_port_invalid")
        profile = f"{profile} {LOCAL_NETWORK_PROFILE}"
        definitions.extend(
            ("-D", f"STARRING_LOCAL_TCP_ENDPOINT=localhost:{local_tcp_port}")
        )
    elif local_tcp_port is not None:
        fail("process_sandbox_local_port_invalid")
    return [
        str(require_sandbox_executable()),
        "-D",
        f"STARRING_MUTABLE_ROOT={mutable}",
        "-D",
        f"STARRING_WORKTREE={source}",
        "-D",
        f"STARRING_RUST_SYSROOT={rust_sysroot}",
        *definitions,
        "-p",
        profile,
        *argv,
    ]
