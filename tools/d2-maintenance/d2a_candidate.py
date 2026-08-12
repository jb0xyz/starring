#!/usr/bin/env python3

"""Build an immutable, non-certifying local candidate bundle for D2A."""

import argparse
import contextlib
import datetime
import fcntl
import hashlib
import json
import os
import pathlib
import re
import selectors
import secrets
import shutil
import signal
import stat
import subprocess
import sys
import time


SCHEMA_VERSION = 1
CANDIDATE_SCHEMA_VERSION = 2
CONFIG_KIND = "starring.d2a.candidate-operator-config.v2"
CANDIDATE_KIND = "starring.d2a.candidate-spec.v2"
PROVENANCE_KIND = "starring.d2a.candidate-provenance.v1"
DEPENDENCY_SNAPSHOT_KIND = "starring.d2a.candidate-dependency-snapshot.v1"
STATE_KIND = "starring.d2a.candidate-build-state.v1"
RESULT_KIND = "starring.d2a.candidate-build-result.v1"
GLOBAL_LOCK_PATH = pathlib.Path("/private/tmp/starring-d2a-candidate-builder.lock")
DEFAULT_BUNDLE_PARENT = (
    pathlib.Path.home()
    / "Library"
    / "Application Support"
    / "Starring"
    / "d2a-candidates"
)
DEFAULT_RUST_TOOLCHAIN_BIN = (
    pathlib.Path.home()
    / ".rustup"
    / "toolchains"
    / "stable-aarch64-apple-darwin"
    / "bin"
)
FIXED_GIT = pathlib.Path("/usr/bin/git")
FIXED_XCRUN = pathlib.Path("/usr/bin/xcrun")
FIXED_XCODE_SELECT = pathlib.Path("/usr/bin/xcode-select")
FIXED_SW_VERS = pathlib.Path("/usr/bin/sw_vers")
FIXED_LINKERS = tuple(
    pathlib.Path(path)
    for path in ("/usr/bin/cc", "/usr/bin/clang", "/usr/bin/ld", "/usr/bin/ar", "/usr/bin/ranlib")
)
MAX_INPUT_BYTES = 64 * 1024
MAX_OUTPUT_BYTES = 1024 * 1024
MAX_DEPENDENCY_ENTRIES = 500000
MAX_DEPENDENCY_BYTES = 4 * 1024 * 1024 * 1024
BUILD_TIMEOUT_SECONDS = 60 * 60
TOOL_TIMEOUT_SECONDS = 60
PROCESS_GROUP_GRACE_SECONDS = 2
COMMIT = re.compile(r"^[0-9a-f]{40}$")
DIGEST = re.compile(r"^[0-9a-f]{64}$")
ERROR_CODE = re.compile(r"^[a-z][a-z0-9_]{0,95}$")

CODEX_WORKER_FILES = (
    "admission-registry.mjs",
    "codex-runner.mjs",
    "metrics-log.mjs",
    "protocol.mjs",
    "request-timeline.mjs",
    "scheduler.mjs",
    "worker.mjs",
)
REPO_ARTIFACTS = (
    ("api", "workspace-target/release/starring-api", "starring-api"),
    ("runtime", "workspace-target/release/starring-runtime", "starring-runtime"),
    (
        "db_bootstrap",
        "workspace-target/release/starring-d2-db-bootstrap",
        "starring-d2-db-bootstrap",
    ),
    (
        "sealed_provisioner",
        "workspace-target/release/starring-d2-sealed-provisioner",
        "starring-d2-sealed-provisioner",
    ),
    (
        "certification_transport",
        "transport-target/release/d2-certification-transport",
        "d2-certification-transport",
    ),
)
OPERATOR_NAMES = ("codex", "node", "cloudflared")
CANDIDATE_NAMES = frozenset(
    {name for name, _source, _destination in REPO_ARTIFACTS}
    | {"codex_worker", *OPERATOR_NAMES}
)
CANDIDATE_FIELDS = {
    "schema_version",
    "kind",
    "commit_sha",
    "source_tree_sha",
    "bundle",
    "provenance_sha256",
    "candidates",
}
CANDIDATE_RECORD_FIELDS = {"path", "sha256"}
CANDIDATE_RELATIVE_PATHS = {
    candidate: pathlib.Path(destination)
    for candidate, _source, destination in REPO_ARTIFACTS
}
CANDIDATE_RELATIVE_PATHS.update(
    {name: pathlib.Path(name) for name in OPERATOR_NAMES}
)
CANDIDATE_RELATIVE_PATHS["codex_worker"] = pathlib.Path("codex-worker/worker.mjs")
WORKSPACE_COMMANDS = (
    (
        "build", "--frozen", "--release", "--target-dir", "{workspace_target}",
        "-p", "starring-api", "--bin", "starring-api",
    ),
    (
        "build", "--frozen", "--release", "--target-dir", "{workspace_target}",
        "-p", "starring-runtime", "--bin", "starring-runtime",
    ),
    (
        "build", "--frozen", "--release", "--target-dir", "{workspace_target}",
        "-p", "starring-db-bootstrap", "--bin", "starring-d2-db-bootstrap",
    ),
    (
        "build", "--frozen", "--release", "--target-dir", "{workspace_target}",
        "-p", "starring-staging-provisioner", "--bin", "starring-d2-sealed-provisioner",
    ),
)
TRANSPORT_COMMAND = (
    "build", "--frozen", "--release", "--manifest-path",
    "tools/d2-certification-transport/Cargo.toml", "--target-dir", "{transport_target}",
)
CONFIG_FIELDS = {"schema_version", "kind", "operators", "dependencies"}
CONFIG_DEPENDENCY_FIELDS = {
    "bootstrap_root", "record_path", "record_sha256", "tree_sha256",
}
DEPENDENCY_RECORD_FIELDS = {
    "schema_version", "kind", "gate_runtime_sha256", "entries",
    "total_bytes", "tree_sha256", "record_sha256",
}
DEPENDENCY_SNAPSHOT_FIELDS = {
    "schema_version", "kind", "bootstrap_root", "record",
    "gate_runtime_sha256", "record_sha256", "tree_sha256", "entries",
    "total_bytes", "workspace", "transport", "source_inputs",
}
DEPENDENCY_SOURCE_FIELDS = {"vendor_root", "cargo_config"}
DEPENDENCY_SOURCE_INPUTS = {
    "workspace_manifest": pathlib.Path("Cargo.toml"),
    "workspace_lock": pathlib.Path("Cargo.lock"),
    "transport_manifest": pathlib.Path("tools/d2-certification-transport/Cargo.toml"),
    "transport_lock": pathlib.Path("tools/d2-certification-transport/Cargo.lock"),
}
IDENTITY_FIELDS = {"path", "sha256", "size", "mode", "uid", "links"}
PROVENANCE_FIELDS = {
    "schema_version", "kind", "status", "release_eligible",
    "commercial_certification", "source", "commands", "environment",
    "dependencies", "toolchain", "artifacts", "worker", "operators",
    "bundle", "builder", "built_at",
}
STATE_FIELDS = {
    "schema_version", "kind", "build_id", "status", "phase",
    "config_path", "config_sha256", "source_root", "source_commit",
    "source_tree", "output_parent", "build_root", "publication_staging",
    "final_bundle", "candidate_spec_sha256", "provenance_sha256",
    "artifact_sha256", "build_processes_quiescent", "last_error", "updated_at",
}
RESULT_FIELDS = {
    "schema_version", "kind", "status", "error_code", "state", "bundle",
    "candidate_spec", "provenance", "source_commit", "release_eligible",
    "commercial_certification",
}


class CandidateError(Exception):
    def __init__(self, code):
        super().__init__(code)
        self.code = code if isinstance(code, str) and ERROR_CODE.fullmatch(code) else "candidate_internal_error"


def fail(code):
    raise CandidateError(code)


def process_group_exists(process_group):
    try:
        os.killpg(process_group, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def terminate_process_group(process, process_group):
    for signal_value, grace in (
        (signal.SIGTERM, PROCESS_GROUP_GRACE_SECONDS),
        (signal.SIGKILL, PROCESS_GROUP_GRACE_SECONDS),
    ):
        if not process_group_exists(process_group):
            break
        try:
            os.killpg(process_group, signal_value)
        except ProcessLookupError:
            break
        deadline = time.monotonic() + grace
        while process_group_exists(process_group) and time.monotonic() < deadline:
            try:
                process.wait(timeout=0.05)
            except subprocess.TimeoutExpired:
                pass
            time.sleep(0.01)
    try:
        process.wait(timeout=PROCESS_GROUP_GRACE_SECONDS)
    except subprocess.TimeoutExpired:
        pass
    return not process_group_exists(process_group)


def bounded_subprocess(argv, cwd, environment, timeout_seconds, maximum=MAX_OUTPUT_BYTES):
    try:
        process = subprocess.Popen(
            argv,
            cwd=cwd,
            env=environment,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            start_new_session=True,
            close_fds=True,
            bufsize=0,
        )
    except OSError:
        fail("candidate_subprocess_failed")
    process_group = process.pid
    streams = {process.stdout: bytearray(), process.stderr: bytearray()}
    exceeded = {process.stdout: False, process.stderr: False}
    selector = selectors.DefaultSelector()
    for stream in streams:
        os.set_blocking(stream.fileno(), False)
        selector.register(stream, selectors.EVENT_READ)
    deadline = time.monotonic() + timeout_seconds
    timed_out = False
    output_exceeded = False
    try:
        while selector.get_map():
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                timed_out = True
                terminate_process_group(process, process_group)
                break
            events = selector.select(min(0.25, remaining))
            if not events and process.poll() is not None:
                events = [(key, selectors.EVENT_READ) for key in selector.get_map().values()]
            for key, _mask in events:
                stream = key.fileobj
                try:
                    chunk = os.read(stream.fileno(), 64 * 1024)
                except BlockingIOError:
                    continue
                except OSError:
                    chunk = b""
                if not chunk:
                    try:
                        selector.unregister(stream)
                    except Exception:
                        pass
                    continue
                retained = max(0, maximum + 1 - len(streams[stream]))
                streams[stream].extend(chunk[:retained])
                if len(chunk) > retained or len(streams[stream]) > maximum:
                    exceeded[stream] = True
                    output_exceeded = True
                    break
            if output_exceeded:
                terminate_process_group(process, process_group)
                break
            if process.poll() is not None and process_group_exists(process_group):
                # A leader that exits while a descendant retains either pipe is
                # an abnormal detached build.  Do not wait out the full deadline.
                terminate_process_group(process, process_group)
                timed_out = True
                break
        if process.poll() is None:
            try:
                process.wait(timeout=max(0.1, deadline - time.monotonic()))
            except subprocess.TimeoutExpired:
                timed_out = True
                terminate_process_group(process, process_group)
        if not timed_out and process_group_exists(process_group):
            terminate_process_group(process, process_group)
            timed_out = True
    except BaseException:
        terminate_process_group(process, process_group)
        raise
    finally:
        selector.close()
        for stream in streams:
            try:
                stream.close()
            except OSError:
                pass
    result = subprocess.CompletedProcess(
        argv,
        process.returncode if process.returncode is not None else -signal.SIGKILL,
        bytes(streams[process.stdout]),
        bytes(streams[process.stderr]),
    )
    result.timed_out = timed_out
    result.output_exceeded = output_exceeded or exceeded[process.stdout] or exceeded[process.stderr]
    result.process_group_id = process_group
    result.process_group_quiescent = not process_group_exists(process_group)
    return result


def canonical_json(value):
    return json.dumps(value, ensure_ascii=False, allow_nan=False, sort_keys=True, separators=(",", ":"))


def sha256_bytes(value):
    return hashlib.sha256(value).hexdigest()


def utc_now():
    return (
        datetime.datetime.now(datetime.timezone.utc)
        .replace(microsecond=0)
        .isoformat()
        .replace("+00:00", "Z")
    )


def strict_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate_key")
        result[key] = value
    return result


def reject_constant(_value):
    raise ValueError("non_finite")


def absolute_normal(raw, label, must_exist=False):
    if not isinstance(raw, (str, os.PathLike)):
        fail(f"{label}_path_invalid")
    path = pathlib.Path(raw)
    if not path.is_absolute() or path != pathlib.Path(os.path.normpath(path)):
        fail(f"{label}_path_invalid")
    try:
        resolved = path.resolve(strict=must_exist)
    except OSError:
        fail(f"{label}_path_invalid")
    if resolved != path:
        fail(f"{label}_path_invalid")
    return path


def require_owned(path, label, mode, directory=False, allow_empty=False):
    try:
        metadata = path.lstat()
    except OSError:
        fail(f"{label}_unavailable")
    expected = stat.S_ISDIR if directory else stat.S_ISREG
    if (
        path.is_symlink()
        or not expected(metadata.st_mode)
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) != mode
        or (not directory and metadata.st_nlink != 1)
        or (not directory and not allow_empty and metadata.st_size < 1)
    ):
        fail(f"{label}_invalid")
    return metadata


def read_private_json(raw_path, label):
    path = absolute_normal(raw_path, label, must_exist=True)
    expected = require_owned(path, label, 0o600)
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError:
        fail(f"{label}_unavailable")
    try:
        before = os.fstat(descriptor)
        if (before.st_dev, before.st_ino) != (expected.st_dev, expected.st_ino):
            fail(f"{label}_invalid")
        raw = b""
        while len(raw) <= MAX_INPUT_BYTES:
            chunk = os.read(descriptor, min(16384, MAX_INPUT_BYTES + 1 - len(raw)))
            if not chunk:
                break
            raw += chunk
        after = os.fstat(descriptor)
        if (
            not raw
            or len(raw) > MAX_INPUT_BYTES
            or (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns)
            != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns)
        ):
            fail(f"{label}_invalid")
    finally:
        os.close(descriptor)
    try:
        value = json.loads(raw, object_pairs_hook=strict_object, parse_constant=reject_constant)
    except (UnicodeDecodeError, json.JSONDecodeError, ValueError):
        fail(f"{label}_invalid")
    return path, value, sha256_bytes(raw)


def read_file(
    path,
    label,
    maximum=512 * 1024 * 1024,
    expected_mode=None,
    expected_uid=None,
    require_single_link=True,
):
    path = absolute_normal(path, label, must_exist=True)
    try:
        metadata = path.lstat()
    except OSError:
        fail(f"{label}_unavailable")
    if (
        path.is_symlink()
        or not stat.S_ISREG(metadata.st_mode)
        or metadata.st_uid != (os.getuid() if expected_uid is None else expected_uid)
        or (require_single_link and metadata.st_nlink != 1)
        or metadata.st_nlink < 1
        or metadata.st_size < 1
        or metadata.st_size > maximum
        or (expected_mode is not None and stat.S_IMODE(metadata.st_mode) != expected_mode)
    ):
        fail(f"{label}_invalid")
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError:
        fail(f"{label}_unavailable")
    try:
        before = os.fstat(descriptor)
        if (before.st_dev, before.st_ino) != (metadata.st_dev, metadata.st_ino):
            fail(f"{label}_invalid")
        digest = hashlib.sha256()
        chunks = []
        size = 0
        while True:
            chunk = os.read(descriptor, 1024 * 1024)
            if not chunk:
                break
            size += len(chunk)
            if size > maximum:
                fail(f"{label}_invalid")
            digest.update(chunk)
            chunks.append(chunk)
        after = os.fstat(descriptor)
        if (
            (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns)
            != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns)
        ):
            fail(f"{label}_changed")
        return b"".join(chunks), {
            "path": str(path),
            "sha256": digest.hexdigest(),
            "size": before.st_size,
            "mode": stat.S_IMODE(before.st_mode),
            "uid": before.st_uid,
            "links": before.st_nlink,
        }
    finally:
        os.close(descriptor)


def fsync_directory(path, label):
    flags = os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0)
    descriptor = None
    try:
        descriptor = os.open(path, flags)
        os.fsync(descriptor)
    except OSError:
        fail(f"{label}_fsync_failed")
    finally:
        if descriptor is not None:
            os.close(descriptor)


def write_new(path, payload, mode):
    descriptor = None
    try:
        descriptor = os.open(
            path,
            os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0),
            mode,
        )
        offset = 0
        while offset < len(payload):
            offset += os.write(descriptor, payload[offset:])
        os.fsync(descriptor)
    except OSError:
        fail("candidate_write_failed")
    finally:
        if descriptor is not None:
            os.close(descriptor)
    require_owned(path, "candidate_written_file", mode)


def ensure_private_directory(path, label):
    path = absolute_normal(path, label, must_exist=False)
    if not path.exists():
        try:
            path.mkdir(mode=0o700, parents=True)
        except OSError:
            fail(f"{label}_unavailable")
    absolute_normal(path, label, must_exist=True)
    require_owned(path, label, 0o700, directory=True)
    return path


def write_state(path, state):
    validate_state(state)
    parent = ensure_private_directory(path.parent, "candidate_output_parent")
    if path.exists() or path.is_symlink():
        require_owned(absolute_normal(path, "candidate_state", True), "candidate_state", 0o600)
    payload = (canonical_json(state) + "\n").encode()
    temporary = parent / f".{path.name}.tmp-{secrets.token_hex(8)}"
    write_new(temporary, payload, 0o600)
    try:
        os.replace(temporary, path)
    except OSError:
        fail("candidate_state_write_failed")
    finally:
        try:
            if temporary.exists() or temporary.is_symlink():
                temporary.unlink()
        except OSError:
            pass
    fsync_directory(parent, "candidate_output_parent")
    require_owned(path, "candidate_state", 0o600)


def write_initial_state(path, state):
    validate_state(state)
    parent = ensure_private_directory(path.parent, "candidate_output_parent")
    if path.exists() or path.is_symlink():
        fail("candidate_state_collision")
    write_new(path, (canonical_json(state) + "\n").encode(), 0o600)
    fsync_directory(parent, "candidate_output_parent")


def validate_config(value):
    if (
        not isinstance(value, dict)
        or set(value) != CONFIG_FIELDS
        or type(value.get("schema_version")) is not int
        or value.get("schema_version") != 2
        or value.get("kind") != CONFIG_KIND
        or not isinstance(value.get("operators"), dict)
        or set(value["operators"]) != set(OPERATOR_NAMES)
        or not isinstance(value.get("dependencies"), dict)
        or set(value["dependencies"]) != CONFIG_DEPENDENCY_FIELDS
    ):
        fail("candidate_config_invalid")
    if len(set(value["operators"].values())) != len(OPERATOR_NAMES):
        fail("candidate_config_invalid")
    identities = {
        name: validate_operator(value["operators"][name], name)
        for name in OPERATOR_NAMES
    }
    return value, identities


def validate_operator(raw, name):
    path = absolute_normal(raw, f"operator_{name}", must_exist=True)
    _raw, identity = read_file(path, f"operator_{name}", expected_mode=0o555)
    try:
        parent = path.parent.lstat()
    except OSError:
        fail(f"operator_{name}_invalid")
    if (
        path.parent.is_symlink()
        or not stat.S_ISDIR(parent.st_mode)
        or parent.st_uid != os.getuid()
        or stat.S_IMODE(parent.st_mode) & 0o222
    ):
        fail(f"operator_{name}_invalid")
    return identity


def validate_state(value):
    if (
        not isinstance(value, dict)
        or set(value) != STATE_FIELDS
        or type(value.get("schema_version")) is not int
        or value.get("schema_version") != 1
        or value.get("kind") != STATE_KIND
        or not isinstance(value.get("build_id"), str)
        or not re.fullmatch(r"d2ac-[0-9a-f]{32}", value["build_id"])
        or value.get("status") not in {"building", "failed", "publishing", "passed", "cleaned"}
        or value.get("phase") not in {
            "initialized", "building", "copying", "publishing", "published", "cleanup", "complete"
        }
        or not isinstance(value.get("source_commit"), str)
        or not COMMIT.fullmatch(value["source_commit"])
        or not isinstance(value.get("source_tree"), str)
        or not COMMIT.fullmatch(value["source_tree"])
        or type(value.get("build_processes_quiescent")) is not bool
    ):
        fail("candidate_state_invalid")
    for field in (
        "config_path", "source_root", "output_parent", "build_root",
        "publication_staging", "final_bundle",
    ):
        absolute_normal(value.get(field), f"candidate_state_{field}", must_exist=False)
    if not DIGEST.fullmatch(value.get("config_sha256", "")):
        fail("candidate_state_invalid")
    for field in ("candidate_spec_sha256", "provenance_sha256"):
        if value[field] is not None and not DIGEST.fullmatch(value[field]):
            fail("candidate_state_invalid")
    artifacts = value.get("artifact_sha256")
    if not isinstance(artifacts, dict) or not set(artifacts).issubset(CANDIDATE_NAMES):
        fail("candidate_state_invalid")
    if any(not isinstance(digest, str) or not DIGEST.fullmatch(digest) for digest in artifacts.values()):
        fail("candidate_state_invalid")
    error = value.get("last_error")
    if error is not None and (not isinstance(error, str) or not ERROR_CODE.fullmatch(error)):
        fail("candidate_state_invalid")
    if not isinstance(value.get("updated_at"), str) or not value["updated_at"].endswith("Z"):
        fail("candidate_state_invalid")
    return value


def load_state(path):
    path, state, _digest = read_private_json(path, "candidate_state")
    validate_state(state)
    if path.parent != pathlib.Path(state["output_parent"]) or path.name != f"state-{state['build_id']}.json":
        fail("candidate_state_invalid")
    return path, state


def executable_identity(path, label, expected_uid=None, require_single_link=True):
    path = absolute_normal(path, label, must_exist=True)
    raw, identity = read_file(
        path,
        label,
        expected_uid=expected_uid,
        require_single_link=require_single_link,
    )
    if identity["mode"] & 0o111 == 0 or identity["mode"] & 0o022:
        fail(f"{label}_invalid")
    return raw, identity


def worker_identities(source_root):
    root = absolute_normal(source_root / "tools" / "codex-worker", "worker_source_root", True)
    try:
        production = {
            path.name
            for path in root.iterdir()
            if path.name.endswith(".mjs") and not path.name.endswith(".test.mjs")
        }
    except OSError:
        fail("worker_source_inventory_invalid")
    if production != set(CODEX_WORKER_FILES):
        fail("worker_source_inventory_invalid")
    result = {}
    digest = hashlib.sha256(b"starring.d2a.codex-worker.v1\0")
    for name in CODEX_WORKER_FILES:
        raw, identity = read_file(root / name, f"worker_source_{name}", maximum=4 * 1024 * 1024)
        encoded = name.encode()
        digest.update(len(encoded).to_bytes(4, "big"))
        digest.update(encoded)
        digest.update(len(raw).to_bytes(8, "big"))
        digest.update(raw)
        result[name] = identity
    return root, result, digest.hexdigest()


class CommandExecutor:
    def __call__(self, argv, cwd, environment):
        timeout = (
            BUILD_TIMEOUT_SECONDS
            if (
                len(argv) > 1
                and pathlib.Path(argv[0]).name == "cargo"
                and "build" in argv[1:4]
            )
            else TOOL_TIMEOUT_SECONDS
        )
        return bounded_subprocess(argv, cwd, environment, timeout)


def bounded_result(result, label, allowed=(0,)):
    stdout = getattr(result, "stdout", b"")
    stderr = getattr(result, "stderr", b"")
    if isinstance(stdout, str):
        stdout = stdout.encode()
    if isinstance(stderr, str):
        stderr = stderr.encode()
    if getattr(result, "timed_out", False):
        fail(f"{label}_timeout")
    if getattr(result, "process_group_quiescent", True) is not True:
        fail(f"{label}_process_group_active")
    if (
        getattr(result, "output_exceeded", False)
        or len(stdout) > MAX_OUTPUT_BYTES
        or len(stderr) > MAX_OUTPUT_BYTES
    ):
        fail(f"{label}_output_invalid")
    returncode = getattr(result, "returncode", None)
    if type(returncode) is not int or returncode not in allowed:
        fail(f"{label}_failed")
    return returncode, stdout, stderr


def reject_cargo_configuration(source_root):
    """Reject every config Cargo could discover above the build working tree."""

    current = absolute_normal(source_root, "source_root", True)
    while True:
        cargo_directory = current / ".cargo"
        for name in ("config", "config.toml"):
            candidate = cargo_directory / name
            if os.path.lexists(candidate):
                fail("cargo_config_present")
        if current.parent == current:
            break
        current = current.parent


def prepare_isolated_cargo_home(build_root):
    cargo_home = build_root / "cargo-home"
    try:
        cargo_home.mkdir(mode=0o700)
    except OSError:
        fail("cargo_home_create_failed")
    try:
        if any(cargo_home.iterdir()):
            fail("cargo_home_invalid")
    except OSError:
        fail("cargo_home_invalid")
    return cargo_home


def workspace_cargo_configuration(bootstrap_root):
    workspace = bootstrap_root / "vendor" / "workspace"
    transport = bootstrap_root / "vendor" / "transport"
    return (
        '[source.crates-io]\nreplace-with = "workspace-vendored-sources"\n\n'
        '[source."git+https://github.com/twilight-rs/twilight.git?rev='
        'b4ce13b727e7731b917576ad977300ab6926bb6b"]\n'
        'git = "https://github.com/twilight-rs/twilight.git"\n'
        'rev = "b4ce13b727e7731b917576ad977300ab6926bb6b"\n'
        'replace-with = "twilight-vendored-sources"\n\n'
        '[source.workspace-vendored-sources]\n'
        f'directory = "{workspace}"\n\n'
        '[source.twilight-vendored-sources]\n'
        f'directory = "{transport}"\n\n'
        '[net]\noffline = true\n'
    ).encode("utf-8")


def transport_cargo_configuration(bootstrap_root):
    transport = bootstrap_root / "vendor" / "transport"
    return (
        '[source.crates-io]\nreplace-with = "vendored-sources"\n\n'
        '[source."git+https://github.com/twilight-rs/twilight.git?rev='
        'b4ce13b727e7731b917576ad977300ab6926bb6b"]\n'
        'git = "https://github.com/twilight-rs/twilight.git"\n'
        'rev = "b4ce13b727e7731b917576ad977300ab6926bb6b"\n'
        'replace-with = "vendored-sources"\n\n'
        '[source.vendored-sources]\n'
        f'directory = "{transport}"\n\n'
        '[net]\noffline = true\n'
    ).encode("utf-8")


def gate_bootstrap_tree_identity(raw_root):
    """Reproduce the D3 sealed-bootstrap digest with stable file reads."""

    root = absolute_normal(raw_root, "dependency_bootstrap_root", True)
    try:
        root_before = root.lstat()
    except OSError:
        fail("dependency_bootstrap_invalid")
    if (
        root.name != "gate-bootstrap"
        or root.is_symlink()
        or not stat.S_ISDIR(root_before.st_mode)
        or root_before.st_uid != os.getuid()
        or stat.S_IMODE(root_before.st_mode) != 0o555
    ):
        fail("dependency_bootstrap_invalid")
    try:
        paths = sorted(root.rglob("*"), key=lambda path: path.relative_to(root).as_posix())
    except OSError:
        fail("dependency_tree_unavailable")
    digest = hashlib.sha256()
    entries = 0
    total_bytes = 0
    inventory = []
    for path in paths:
        relative = path.relative_to(root).as_posix()
        inventory.append(relative)
        try:
            before = path.lstat()
        except OSError:
            fail("dependency_tree_unavailable")
        mode = stat.S_IMODE(before.st_mode)
        if stat.S_ISLNK(before.st_mode) or before.st_uid != os.getuid() or mode & 0o222:
            fail("dependency_tree_invalid")
        entries += 1
        if entries > MAX_DEPENDENCY_ENTRIES:
            fail("dependency_tree_too_large")
        if stat.S_ISDIR(before.st_mode):
            kind = "directory"
            size = 0
        elif stat.S_ISREG(before.st_mode) and before.st_nlink == 1:
            kind = "file"
            size = before.st_size
        else:
            fail("dependency_tree_invalid")
        header = canonical_json(
            {"path": relative, "kind": kind, "mode": mode, "size": size}
        ).encode("utf-8")
        digest.update(len(header).to_bytes(8, "big"))
        digest.update(header)
        if kind == "directory":
            continue
        total_bytes += size
        if total_bytes > MAX_DEPENDENCY_BYTES:
            fail("dependency_tree_too_large")
        flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
        try:
            descriptor = os.open(path, flags)
        except OSError:
            fail("dependency_tree_unavailable")
        observed = 0
        try:
            while True:
                chunk = os.read(descriptor, 1024 * 1024)
                if not chunk:
                    break
                observed += len(chunk)
                if observed > size:
                    fail("dependency_tree_changed")
                digest.update(chunk)
            after = os.fstat(descriptor)
        finally:
            os.close(descriptor)
        if (
            observed != size
            or (
                before.st_dev, before.st_ino, before.st_uid, before.st_gid,
                before.st_mode, before.st_nlink, before.st_size,
                before.st_mtime_ns, before.st_ctime_ns,
            )
            != (
                after.st_dev, after.st_ino, after.st_uid, after.st_gid,
                after.st_mode, after.st_nlink, after.st_size,
                after.st_mtime_ns, after.st_ctime_ns,
            )
        ):
            fail("dependency_tree_changed")
    try:
        final_inventory = [
            path.relative_to(root).as_posix()
            for path in sorted(
                root.rglob("*"), key=lambda path: path.relative_to(root).as_posix()
            )
        ]
        root_after = root.lstat()
    except OSError:
        fail("dependency_tree_unavailable")
    if (
        inventory != final_inventory
        or (
            root_before.st_dev, root_before.st_ino, root_before.st_uid,
            root_before.st_gid, root_before.st_mode, root_before.st_mtime_ns,
            root_before.st_ctime_ns,
        )
        != (
            root_after.st_dev, root_after.st_ino, root_after.st_uid,
            root_after.st_gid, root_after.st_mode, root_after.st_mtime_ns,
            root_after.st_ctime_ns,
        )
    ):
        fail("dependency_tree_changed")
    return {
        "entries": entries,
        "total_bytes": total_bytes,
        "tree_sha256": digest.hexdigest(),
    }


def dependency_record(raw_path, expected_root):
    record_path = absolute_normal(raw_path, "dependency_record", True)
    if record_path != expected_root.parent / "gate-bootstrap.json":
        fail("dependency_record_path_invalid")
    require_owned(expected_root.parent, "dependency_state_root", 0o700, directory=True)
    raw, identity = read_file(
        record_path, "dependency_record", maximum=MAX_INPUT_BYTES, expected_mode=0o600
    )
    try:
        value = json.loads(raw, object_pairs_hook=strict_object, parse_constant=reject_constant)
    except (UnicodeDecodeError, json.JSONDecodeError, ValueError):
        fail("dependency_record_invalid")
    if (
        not isinstance(value, dict)
        or set(value) != DEPENDENCY_RECORD_FIELDS
        or type(value.get("schema_version")) is not int
        or value.get("schema_version") != 1
        or value.get("kind") != "starring.d3.gate-bootstrap.v1"
        or not DIGEST.fullmatch(value.get("gate_runtime_sha256", ""))
        or type(value.get("entries")) is not int
        or not 0 < value["entries"] <= MAX_DEPENDENCY_ENTRIES
        or type(value.get("total_bytes")) is not int
        or not 0 < value["total_bytes"] <= MAX_DEPENDENCY_BYTES
        or not DIGEST.fullmatch(value.get("tree_sha256", ""))
        or not DIGEST.fullmatch(value.get("record_sha256", ""))
    ):
        fail("dependency_record_invalid")
    payload = dict(value)
    seal = payload.pop("record_sha256")
    if (
        sha256_bytes(canonical_json(payload).encode("utf-8")) != seal
        or raw != (canonical_json(value) + "\n").encode("utf-8")
    ):
        fail("dependency_record_invalid")
    return value, identity


def dependency_file_identity(path, label, mode):
    _raw, identity = read_file(path, label, expected_mode=mode)
    if set(identity) != IDENTITY_FIELDS:
        fail(f"{label}_invalid")
    return identity


def load_dependency_snapshot(value, source_root):
    if (
        not isinstance(value, dict)
        or set(value) != CONFIG_DEPENDENCY_FIELDS
        or not DIGEST.fullmatch(value.get("record_sha256", ""))
        or not DIGEST.fullmatch(value.get("tree_sha256", ""))
    ):
        fail("candidate_dependencies_invalid")
    bootstrap = absolute_normal(value.get("bootstrap_root"), "dependency_bootstrap_root", True)
    if any(character in str(bootstrap) for character in ('"', "\\", "\x00", "\n", "\r")):
        fail("dependency_bootstrap_path_invalid")
    record, record_identity = dependency_record(value.get("record_path"), bootstrap)
    if (
        record["record_sha256"] != value["record_sha256"]
        or record["tree_sha256"] != value["tree_sha256"]
    ):
        fail("candidate_dependencies_invalid")
    tree = gate_bootstrap_tree_identity(bootstrap)
    if any(tree[name] != record[name] for name in tree):
        fail("dependency_tree_mismatch")
    workspace_vendor = bootstrap / "vendor" / "workspace"
    transport_vendor = bootstrap / "vendor" / "transport"
    require_owned(workspace_vendor, "dependency_workspace_vendor", 0o500, directory=True)
    require_owned(transport_vendor, "dependency_transport_vendor", 0o500, directory=True)
    workspace_config = bootstrap / "native-cargo-config.toml"
    transport_config = bootstrap / "native-transport-cargo-config.toml"
    workspace_raw, workspace_identity = read_file(
        workspace_config, "dependency_workspace_config", maximum=MAX_INPUT_BYTES, expected_mode=0o400
    )
    transport_raw, transport_identity = read_file(
        transport_config, "dependency_transport_config", maximum=MAX_INPUT_BYTES, expected_mode=0o400
    )
    if (
        workspace_raw != workspace_cargo_configuration(bootstrap)
        or transport_raw != transport_cargo_configuration(bootstrap)
    ):
        fail("dependency_cargo_config_invalid")
    source_root = absolute_normal(source_root, "source_root", True)
    source_inputs = {
        name: dependency_file_identity(
            source_root / relative, f"dependency_source_{name}", 0o644
        )
        for name, relative in DEPENDENCY_SOURCE_INPUTS.items()
    }
    return {
        "schema_version": 1,
        "kind": DEPENDENCY_SNAPSHOT_KIND,
        "bootstrap_root": str(bootstrap),
        "record": record_identity,
        "gate_runtime_sha256": record["gate_runtime_sha256"],
        "record_sha256": record["record_sha256"],
        "tree_sha256": record["tree_sha256"],
        "entries": record["entries"],
        "total_bytes": record["total_bytes"],
        "workspace": {
            "vendor_root": str(workspace_vendor),
            "cargo_config": workspace_identity,
        },
        "transport": {
            "vendor_root": str(transport_vendor),
            "cargo_config": transport_identity,
        },
        "source_inputs": source_inputs,
    }


def validate_dependency_snapshot(value, source_root):
    if (
        not isinstance(value, dict)
        or set(value) != DEPENDENCY_SNAPSHOT_FIELDS
        or type(value.get("schema_version")) is not int
        or value.get("schema_version") != 1
        or value.get("kind") != DEPENDENCY_SNAPSHOT_KIND
        or not isinstance(value.get("record"), dict)
        or set(value["record"]) != IDENTITY_FIELDS
        or not isinstance(value.get("workspace"), dict)
        or set(value["workspace"]) != DEPENDENCY_SOURCE_FIELDS
        or not isinstance(value.get("transport"), dict)
        or set(value["transport"]) != DEPENDENCY_SOURCE_FIELDS
        or not isinstance(value.get("source_inputs"), dict)
        or set(value["source_inputs"]) != set(DEPENDENCY_SOURCE_INPUTS)
        or any(
            not isinstance(record, dict) or set(record) != IDENTITY_FIELDS
            for record in (
                value["workspace"].get("cargo_config"),
                value["transport"].get("cargo_config"),
                *value["source_inputs"].values(),
            )
        )
    ):
        fail("dependency_snapshot_invalid")
    observed = load_dependency_snapshot(
        {
            "bootstrap_root": value.get("bootstrap_root"),
            "record_path": value["record"].get("path"),
            "record_sha256": value.get("record_sha256"),
            "tree_sha256": value.get("tree_sha256"),
        },
        source_root,
    )
    if observed != value:
        fail("dependency_snapshot_changed")
    return value


def stable_file_digest(path, label, maximum=1024 * 1024 * 1024):
    try:
        before = path.lstat()
    except OSError:
        fail(f"{label}_unavailable")
    if (
        path.is_symlink()
        or not stat.S_ISREG(before.st_mode)
        or before.st_uid not in {0, os.getuid()}
        or before.st_nlink < 1
        or stat.S_IMODE(before.st_mode) & 0o022
        or before.st_size > maximum
    ):
        fail(f"{label}_invalid")
    digest = hashlib.sha256()
    observed = 0
    try:
        with path.open("rb", buffering=0) as handle:
            while True:
                chunk = handle.read(1024 * 1024)
                if not chunk:
                    break
                observed += len(chunk)
                if observed > maximum:
                    fail(f"{label}_invalid")
                digest.update(chunk)
            after = os.fstat(handle.fileno())
    except OSError:
        fail(f"{label}_unavailable")
    immutable = (
        before.st_dev,
        before.st_ino,
        before.st_uid,
        before.st_gid,
        before.st_mode,
        before.st_nlink,
        before.st_size,
        before.st_mtime_ns,
    )
    if immutable != (
        after.st_dev,
        after.st_ino,
        after.st_uid,
        after.st_gid,
        after.st_mode,
        after.st_nlink,
        after.st_size,
        after.st_mtime_ns,
    ):
        fail(f"{label}_changed")
    return digest.hexdigest(), before


def rust_toolchain_manifest(bin_root):
    sysroot = absolute_normal(bin_root.parent, "rust_sysroot", True)
    records = []
    try:
        paths = [sysroot, *sorted(sysroot.rglob("*"), key=lambda item: str(item.relative_to(sysroot)))]
    except OSError:
        fail("rust_sysroot_invalid")
    for path in paths:
        relative = "." if path == sysroot else path.relative_to(sysroot).as_posix()
        try:
            metadata = path.lstat()
        except OSError:
            fail("rust_sysroot_invalid")
        if path.is_symlink() or metadata.st_uid != os.getuid() or stat.S_IMODE(metadata.st_mode) & 0o022:
            fail("rust_sysroot_invalid")
        if stat.S_ISDIR(metadata.st_mode):
            records.append({"path": relative, "kind": "directory", "mode": stat.S_IMODE(metadata.st_mode)})
        elif stat.S_ISREG(metadata.st_mode):
            digest, stable = stable_file_digest(path, "rust_sysroot_file")
            if stable.st_nlink != 1:
                fail("rust_sysroot_invalid")
            records.append({
                "path": relative,
                "kind": "file",
                "mode": stat.S_IMODE(stable.st_mode),
                "size": stable.st_size,
                "sha256": digest,
            })
        else:
            fail("rust_sysroot_invalid")
    linker_records = []
    digest_cache = {}
    for path in FIXED_LINKERS:
        fixed = absolute_normal(path, "rust_linker", True)
        try:
            observed = fixed.lstat()
        except OSError:
            fail("rust_linker_unavailable")
        key = (observed.st_dev, observed.st_ino, observed.st_size, observed.st_mtime_ns)
        if key in digest_cache:
            digest, metadata = digest_cache[key], observed
            if (
                fixed.is_symlink()
                or not stat.S_ISREG(metadata.st_mode)
                or metadata.st_uid != 0
                or stat.S_IMODE(metadata.st_mode) & 0o022
            ):
                fail("rust_linker_invalid")
        else:
            digest, metadata = stable_file_digest(fixed, "rust_linker")
            digest_cache[key] = digest
        key = (metadata.st_dev, metadata.st_ino, metadata.st_size, metadata.st_mtime_ns)
        if digest_cache[key] != digest or stat.S_IMODE(metadata.st_mode) & 0o111 == 0:
            fail("rust_linker_invalid")
        linker_records.append({
            "path": str(fixed),
            "mode": stat.S_IMODE(metadata.st_mode),
            "uid": metadata.st_uid,
            "nlink": metadata.st_nlink,
            "size": metadata.st_size,
            "sha256": digest_cache[key],
        })
    return {
        "sysroot": str(sysroot),
        "files": records,
        "sha256": sha256_bytes(canonical_json(records).encode("utf-8")),
        "linkers": linker_records,
        "linkers_sha256": sha256_bytes(canonical_json(linker_records).encode("utf-8")),
    }


def rooted_tree_digest(root, label):
    root = absolute_normal(root, label, True)
    digest = hashlib.sha256(f"starring.d2a.{label}.v1\0".encode("ascii"))
    ancestors = []
    for directory in (root, *root.parents):
        try:
            metadata = directory.lstat()
        except OSError:
            fail(f"{label}_invalid")
        if (
            directory.is_symlink()
            or not stat.S_ISDIR(metadata.st_mode)
            or metadata.st_uid != 0
            or stat.S_IMODE(metadata.st_mode) & 0o022
        ):
            fail(f"{label}_invalid")
        ancestors.append({
            "path": str(directory),
            "mode": stat.S_IMODE(metadata.st_mode),
            "uid": metadata.st_uid,
        })
    root_identity = ancestors[0]
    encoded_root = canonical_json(root_identity).encode("utf-8")
    digest.update(len(encoded_root).to_bytes(8, "big"))
    digest.update(encoded_root)
    count = 0
    try:
        paths = sorted(root.rglob("*"), key=lambda item: item.relative_to(root).as_posix())
    except OSError:
        fail(f"{label}_invalid")
    for path in paths:
        relative = path.relative_to(root).as_posix()
        try:
            metadata = path.lstat()
        except OSError:
            fail(f"{label}_invalid")
        if metadata.st_uid != 0 or (
            not stat.S_ISLNK(metadata.st_mode)
            and stat.S_IMODE(metadata.st_mode) & 0o022
        ):
            fail(f"{label}_invalid")
        if stat.S_ISDIR(metadata.st_mode):
            record = {"path": relative, "kind": "directory", "mode": stat.S_IMODE(metadata.st_mode)}
        elif stat.S_ISLNK(metadata.st_mode):
            try:
                target = os.readlink(path)
                resolved = path.resolve(strict=True)
            except OSError:
                fail(f"{label}_invalid")
            if resolved != root and root not in resolved.parents:
                fail(f"{label}_invalid")
            record = {
                "path": relative,
                "kind": "symlink",
                "target": target,
                "resolved_path": resolved.relative_to(root).as_posix(),
            }
        elif stat.S_ISREG(metadata.st_mode):
            file_digest, stable = stable_file_digest(path, f"{label}_file")
            if stable.st_uid != 0:
                fail(f"{label}_invalid")
            record = {
                "path": relative,
                "kind": "file",
                "mode": stat.S_IMODE(stable.st_mode),
                "size": stable.st_size,
                "sha256": file_digest,
            }
        else:
            fail(f"{label}_invalid")
        encoded = canonical_json(record).encode("utf-8")
        digest.update(len(encoded).to_bytes(8, "big"))
        digest.update(encoded)
        count += 1
    return {
        "root": str(root),
        "root_identity": root_identity,
        "ancestors": ancestors,
        "entries": count,
        "sha256": digest.hexdigest(),
    }


@contextlib.contextmanager
def global_lock(path=GLOBAL_LOCK_PATH):
    path = absolute_normal(path, "candidate_lock", must_exist=False)
    try:
        descriptor = os.open(path, os.O_RDWR | os.O_CREAT | getattr(os, "O_NOFOLLOW", 0), 0o600)
    except OSError:
        fail("candidate_lock_unavailable")
    try:
        metadata = os.fstat(descriptor)
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_uid != os.getuid()
            or stat.S_IMODE(metadata.st_mode) != 0o600
        ):
            fail("candidate_lock_invalid")
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            fail("candidate_lock_busy")
        yield
    finally:
        try:
            fcntl.flock(descriptor, fcntl.LOCK_UN)
        except OSError:
            pass
        os.close(descriptor)


class CandidateBuilder:
    def __init__(
        self,
        executor=None,
        source_root=None,
        bundle_parent=DEFAULT_BUNDLE_PARENT,
        rust_toolchain_bin=DEFAULT_RUST_TOOLCHAIN_BIN,
        lock_path=GLOBAL_LOCK_PATH,
        git_path=FIXED_GIT,
    ):
        self.executor = executor or CommandExecutor()
        self.source_root = pathlib.Path(source_root or pathlib.Path(__file__).resolve().parents[2])
        self.bundle_parent = pathlib.Path(bundle_parent)
        self.rust_toolchain_bin = pathlib.Path(rust_toolchain_bin)
        self.lock_path = pathlib.Path(lock_path)
        self.git_path = pathlib.Path(git_path)

    def run_process(self, argv, cwd, environment, label, allowed=(0,)):
        return bounded_result(self.executor(argv, cwd, environment), label, allowed)

    @staticmethod
    def run_process_result(completed, label, allowed=(0,)):
        return bounded_result(completed, label, allowed)

    def git_environment(self):
        return {
            "HOME": str(pathlib.Path.home()),
            "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_TERMINAL_PROMPT": "0",
            "LC_ALL": "C",
        }

    def git(self, *arguments, allowed=(0,)):
        argv = [str(self.git_path), "-C", str(self.source_root), *arguments]
        return self.run_process(argv, self.source_root, self.git_environment(), "source_git", allowed)

    def source_snapshot(self):
        root = absolute_normal(self.source_root, "source_root", True)
        try:
            metadata = root.lstat()
        except OSError:
            fail("source_root_invalid")
        if root.is_symlink() or not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid != os.getuid():
            fail("source_root_invalid")
        git = absolute_normal(self.git_path, "git", True)
        if git != FIXED_GIT and self.git_path == FIXED_GIT:
            fail("git_invalid")
        _raw, git_identity = executable_identity(
            git,
            "git",
            expected_uid=0 if git == FIXED_GIT else os.getuid(),
            require_single_link=git != FIXED_GIT,
        )
        _code, stdout, _stderr = self.git("rev-parse", "--show-toplevel")
        if stdout.decode("utf-8", "strict").strip() != str(root):
            fail("source_root_mismatch")
        _code, stdout, _stderr = self.git("rev-parse", "--verify", "HEAD")
        commit = stdout.decode("ascii", "strict").strip()
        _code, stdout, _stderr = self.git("rev-parse", "--verify", "HEAD^{tree}")
        tree = stdout.decode("ascii", "strict").strip()
        if not COMMIT.fullmatch(commit) or not COMMIT.fullmatch(tree):
            fail("source_identity_invalid")
        _code, stdout, _stderr = self.git("status", "--porcelain=v1", "--untracked-files=all")
        if stdout:
            fail("source_dirty")
        code, _stdout, _stderr = self.git("diff", "--quiet", "--no-ext-diff", "HEAD", "--", allowed=(0, 1))
        if code == 1:
            fail("source_dirty")
        code, _stdout, _stderr = self.git("diff", "--cached", "--quiet", "--no-ext-diff", "HEAD", "--", allowed=(0, 1))
        if code == 1:
            fail("source_dirty")
        return {"root": str(root), "commit": commit, "tree": tree, "clean": True, "git": git_identity}

    def strict_tool_output(self, argv, environment, label):
        _code, stdout, stderr = self.run_process(
            argv, self.source_root, environment, label
        )
        if stderr:
            fail(f"{label}_invalid")
        try:
            value = stdout.decode("utf-8").strip()
        except UnicodeDecodeError:
            fail(f"{label}_invalid")
        if not value or "\n" in value or "\r" in value:
            fail(f"{label}_invalid")
        return value

    def darwin_toolchain(self, environment):
        fixed = {}
        for name, path in (
            ("xcrun", FIXED_XCRUN),
            ("xcode_select", FIXED_XCODE_SELECT),
            ("sw_vers", FIXED_SW_VERS),
        ):
            _raw, identity = executable_identity(
                path, f"darwin_{name}", expected_uid=0, require_single_link=False
            )
            fixed[name] = identity
        developer_root_raw = self.strict_tool_output(
            [str(FIXED_XCODE_SELECT), "-p"], environment, "xcode_select"
        )
        developer_root = pathlib.Path(developer_root_raw)
        if not developer_root.is_absolute():
            fail("xcode_select_invalid")
        developer_root = developer_root.resolve(strict=True)
        try:
            developer_metadata = developer_root.lstat()
        except OSError:
            fail("xcode_select_invalid")
        if (
            developer_root.is_symlink()
            or not stat.S_ISDIR(developer_metadata.st_mode)
            or developer_metadata.st_uid != 0
            or stat.S_IMODE(developer_metadata.st_mode) & 0o022
        ):
            fail("xcode_select_invalid")
        selected = {}
        for name in ("clang", "ld", "ar", "ranlib", "otool"):
            raw = self.strict_tool_output(
                [str(FIXED_XCRUN), "--find", name], environment, f"xcrun_{name}"
            )
            selector = pathlib.Path(raw)
            if not selector.is_absolute() or selector != pathlib.Path(os.path.normpath(selector)):
                fail("xcrun_tool_invalid")
            try:
                selector_metadata = selector.lstat()
                resolved = selector.resolve(strict=True)
            except OSError:
                fail("xcrun_tool_invalid")
            if developer_root not in resolved.parents:
                fail("xcrun_tool_invalid")
            link_target = os.readlink(selector) if stat.S_ISLNK(selector_metadata.st_mode) else None
            digest, metadata = stable_file_digest(resolved, "xcrun_tool")
            if metadata.st_uid != 0 or stat.S_IMODE(metadata.st_mode) & 0o111 == 0:
                fail("xcrun_tool_invalid")
            selected[name] = {
                "selected_path": str(selector),
                "selected_link_target": link_target,
                "resolved_path": str(resolved),
                "sha256": digest,
                "size": metadata.st_size,
                "mode": stat.S_IMODE(metadata.st_mode),
            }
        sdk_raw = self.strict_tool_output(
            [str(FIXED_XCRUN), "--sdk", "macosx", "--show-sdk-path"],
            environment,
            "xcrun_sdk",
        )
        sdk_selector = pathlib.Path(sdk_raw)
        if not sdk_selector.is_absolute() or sdk_selector != pathlib.Path(os.path.normpath(sdk_selector)):
            fail("xcrun_sdk_invalid")
        try:
            sdk_metadata = sdk_selector.lstat()
            sdk_resolved = sdk_selector.resolve(strict=True)
        except OSError:
            fail("xcrun_sdk_invalid")
        if developer_root not in sdk_resolved.parents or not sdk_resolved.is_dir():
            fail("xcrun_sdk_invalid")
        sdk_link_target = os.readlink(sdk_selector) if stat.S_ISLNK(sdk_metadata.st_mode) else None
        sdk = rooted_tree_digest(sdk_resolved, "macos_sdk")
        sdk.update({"selected_path": str(sdk_selector), "selected_link_target": sdk_link_target})
        os_build = self.strict_tool_output(
            [str(FIXED_SW_VERS), "-buildVersion"], environment, "os_build_version"
        )
        if not re.fullmatch(r"[0-9A-Za-z.]+", os_build):
            fail("os_build_version_invalid")
        return {
            "fixed_tools": fixed,
            "developer_root": str(developer_root),
            "selected_tools": selected,
            "sdk": sdk,
            "os_build_version": os_build,
        }

    def validate_macho_linkage(self, path, toolchain, label):
        path = absolute_normal(path, label, True)
        otool = toolchain["darwin"]["selected_tools"]["otool"]["resolved_path"]
        _code, stdout, stderr = self.run_process(
            [otool, "-L", str(path)],
            self.source_root,
            {
                "HOME": str(pathlib.Path.home()),
                "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
                "LC_ALL": "C",
            },
            f"{label}_linkage",
        )
        if stderr:
            fail(f"{label}_linkage_invalid")
        try:
            lines = stdout.decode("utf-8").splitlines()
        except UnicodeDecodeError:
            fail(f"{label}_linkage_invalid")
        if not lines or lines[0] != f"{path}:" or len(lines) < 2:
            fail(f"{label}_linkage_invalid")
        dependencies = []
        for line in lines[1:]:
            if not line.startswith("\t"):
                fail(f"{label}_linkage_invalid")
            dependency = line.strip().split(" ", 1)[0]
            if not dependency.startswith(
                (
                    "/usr/lib/",
                    "/System/Library/Frameworks/",
                    "/System/Library/PrivateFrameworks/",
                )
            ):
                fail(f"{label}_linkage_invalid")
            dependencies.append(dependency)
        if not dependencies:
            fail(f"{label}_linkage_invalid")
        return dependencies

    def toolchain(self):
        root = absolute_normal(self.rust_toolchain_bin, "rust_toolchain_bin", True)
        require_owned(root, "rust_toolchain_bin", 0o755, directory=True)
        tools = {}
        environment = {
            "HOME": str(pathlib.Path.home()),
            "PATH": f"{root}:/usr/bin:/bin:/usr/sbin:/sbin",
            "LC_ALL": "C",
        }
        manifest = rust_toolchain_manifest(root)
        darwin = self.darwin_toolchain(environment)
        for name in ("cargo", "rustc"):
            path = root / name
            _raw, identity = executable_identity(path, f"rust_{name}")
            _code, stdout, stderr = self.run_process(
                [str(path), "--version"], self.source_root, environment, f"rust_{name}_version"
            )
            if stderr:
                fail(f"rust_{name}_version_invalid")
            try:
                version = stdout.decode("ascii").strip()
            except UnicodeDecodeError:
                fail(f"rust_{name}_version_invalid")
            if not version.startswith(f"{name} 1.97.0 "):
                fail(f"rust_{name}_version_invalid")
            tools[name] = {**identity, "version": version}
        _code, stdout, stderr = self.run_process(
            [tools["rustc"]["path"], "-vV"],
            self.source_root,
            environment,
            "rust_host_version",
        )
        if stderr:
            fail("rust_host_version_invalid")
        try:
            verbose = stdout.decode("ascii").splitlines()
        except UnicodeDecodeError:
            fail("rust_host_version_invalid")
        fields = {
            key: value
            for line in verbose
            if ": " in line
            for key, value in [line.split(": ", 1)]
        }
        if fields.get("release") != "1.97.0" or fields.get("host") != "aarch64-apple-darwin":
            fail("rust_host_version_invalid")
        return {
            "target": "aarch64-apple-darwin",
            "root": str(root),
            "rustc_verbose_version": verbose,
            "tools": tools,
            "sysroot_manifest": manifest,
            "darwin": darwin,
        }

    def operator_identities(self, config):
        return {name: validate_operator(config["operators"][name], name) for name in OPERATOR_NAMES}

    def build_commands(self, build_root, cargo, dependencies):
        workspace_target = build_root / "workspace-target"
        transport_target = build_root / "transport-target"
        workspace_config = dependencies["workspace"]["cargo_config"]["path"]
        transport_config = dependencies["transport"]["cargo_config"]["path"]
        commands = []
        for command in WORKSPACE_COMMANDS:
            commands.append([
                str(cargo),
                "--config",
                workspace_config,
                *(str(workspace_target) if value == "{workspace_target}" else value for value in command),
            ])
        commands.append([
            str(cargo),
            "--config",
            transport_config,
            *(str(transport_target) if value == "{transport_target}" else value for value in TRANSPORT_COMMAND),
        ])
        return commands

    def require_dependencies(self, config, expected):
        observed = load_dependency_snapshot(config["dependencies"], self.source_root)
        if observed != expected:
            fail("dependency_snapshot_changed")
        return observed

    def verify_dependency_compatibility(
        self, cargo, environment, dependencies, state_path, state
    ):
        checks = (
            (
                "workspace",
                dependencies["workspace"]["cargo_config"]["path"],
                self.source_root / DEPENDENCY_SOURCE_INPUTS["workspace_manifest"],
            ),
            (
                "transport",
                dependencies["transport"]["cargo_config"]["path"],
                self.source_root / DEPENDENCY_SOURCE_INPUTS["transport_manifest"],
            ),
        )
        for name, configuration, manifest in checks:
            command = [
                    str(cargo),
                    "--config",
                    configuration,
                    "metadata",
                    "--locked",
                    "--offline",
                    "--format-version=1",
                    "--manifest-path",
                    str(manifest),
                ]
            self.save(state_path, state, build_processes_quiescent=False)
            completed = self.executor(command, self.source_root, environment)
            if getattr(completed, "process_group_quiescent", True) is True:
                self.save(state_path, state, build_processes_quiescent=True)
            self.run_process_result(completed, f"dependency_{name}_metadata")

    def build_environment(self, toolchain, commit, build_root):
        temporary = build_root / "tmp"
        temporary.mkdir(mode=0o700)
        reject_cargo_configuration(self.source_root)
        cargo_home = prepare_isolated_cargo_home(build_root)
        return {
            "HOME": str(pathlib.Path.home()),
            "PATH": f"{toolchain['root']}:/usr/bin:/bin:/usr/sbin:/sbin",
            "RUSTC": toolchain["tools"]["rustc"]["path"],
            "CC": toolchain["darwin"]["selected_tools"]["clang"]["resolved_path"],
            "CXX": toolchain["darwin"]["selected_tools"]["clang"]["resolved_path"],
            "AR": toolchain["darwin"]["selected_tools"]["ar"]["resolved_path"],
            "RANLIB": toolchain["darwin"]["selected_tools"]["ranlib"]["resolved_path"],
            "SDKROOT": toolchain["darwin"]["sdk"]["root"],
            "CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER": toolchain["darwin"]["selected_tools"]["clang"]["resolved_path"],
            "CARGO_HOME": str(cargo_home),
            "CARGO_INCREMENTAL": "0",
            "CARGO_BUILD_JOBS": "1",
            "CARGO_NET_OFFLINE": "true",
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_TERMINAL_PROMPT": "0",
            "LC_ALL": "C",
            "STARRING_RUNTIME_BUILD_REVISION": commit,
            "TMPDIR": str(temporary),
        }

    def save(self, state_path, state, **changes):
        state.update(changes)
        state["updated_at"] = utc_now()
        write_state(state_path, state)

    def copy_verified(self, source, destination, source_label, destination_mode, expected=None):
        raw, source_identity = read_file(
            source,
            source_label,
            expected_mode=expected["mode"] if expected is not None else None,
        )
        if expected is not None and source_identity != expected:
            fail(f"{source_label}_changed")
        if destination_mode == 0o555 and (
            source_identity["mode"] & 0o111 == 0
            or source_identity["mode"] & 0o022
        ):
            fail(f"{source_label}_not_executable")
        write_new(destination, raw, destination_mode)
        copied, artifact_identity = read_file(
            destination,
            f"artifact_{destination.name}",
            expected_mode=destination_mode,
        )
        if copied != raw or artifact_identity["sha256"] != source_identity["sha256"]:
            fail("candidate_copy_mismatch")
        return source_identity, artifact_identity

    def provenance(
        self,
        source,
        toolchain,
        commands,
        environment,
        dependencies,
        artifacts,
        workers,
        worker_tree_sha,
        operators,
        final_bundle,
    ):
        builder_raw, builder_identity = read_file(pathlib.Path(__file__).resolve(), "candidate_builder", 4 * 1024 * 1024)
        del builder_raw
        return {
            "schema_version": 1,
            "kind": PROVENANCE_KIND,
            "status": "built",
            "release_eligible": False,
            "commercial_certification": False,
            "source": source,
            "commands": commands,
            "dependencies": dependencies,
            "environment": {
                key: environment[key]
                for key in (
                    "AR", "CARGO_BUILD_JOBS", "CARGO_HOME", "CARGO_INCREMENTAL",
                    "CARGO_NET_OFFLINE", "CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER",
                    "CC", "CXX", "GIT_TERMINAL_PROMPT", "HOME", "LC_ALL",
                    "GIT_CONFIG_NOSYSTEM", "PATH", "RANLIB", "RUSTC", "SDKROOT", "TMPDIR",
                    "STARRING_RUNTIME_BUILD_REVISION",
                )
            },
            "toolchain": toolchain,
            "artifacts": artifacts,
            "worker": {"tree_sha256": worker_tree_sha, "files": workers},
            "operators": operators,
            "bundle": str(final_bundle),
            "builder": builder_identity,
            "built_at": utc_now(),
        }

    def validate_bundle(self, final_bundle, state):
        final_bundle = absolute_normal(final_bundle, "candidate_bundle", True)
        require_owned(final_bundle, "candidate_bundle", 0o555, directory=True)
        spec_path = final_bundle / "candidate-spec.json"
        provenance_path = final_bundle / "provenance.json"
        spec_raw, _spec_identity = read_file(spec_path, "candidate_spec", 64 * 1024, 0o400)
        provenance_raw, _provenance_identity = read_file(
            provenance_path,
            "candidate_provenance",
            1024 * 1024,
            0o400,
        )
        if sha256_bytes(spec_raw) != state["candidate_spec_sha256"] or sha256_bytes(provenance_raw) != state["provenance_sha256"]:
            fail("candidate_publication_changed")
        try:
            spec = json.loads(spec_raw, object_pairs_hook=strict_object, parse_constant=reject_constant)
            provenance = json.loads(provenance_raw, object_pairs_hook=strict_object, parse_constant=reject_constant)
        except (UnicodeDecodeError, json.JSONDecodeError, ValueError):
            fail("candidate_publication_invalid")
        if (
            not isinstance(spec, dict)
            or set(spec) != CANDIDATE_FIELDS
            or type(spec.get("schema_version")) is not int
            or spec.get("schema_version") != CANDIDATE_SCHEMA_VERSION
            or spec.get("kind") != CANDIDATE_KIND
            or spec.get("commit_sha") != state["source_commit"]
            or spec.get("source_tree_sha") != state["source_tree"]
            or spec.get("bundle") != str(final_bundle)
            or spec.get("provenance_sha256") != sha256_bytes(provenance_raw)
            or not isinstance(spec.get("candidates"), dict)
            or set(spec["candidates"]) != CANDIDATE_NAMES
            or not isinstance(provenance, dict)
            or set(provenance) != PROVENANCE_FIELDS
            or type(provenance.get("schema_version")) is not int
            or provenance.get("schema_version") != 1
            or provenance.get("kind") != PROVENANCE_KIND
            or provenance.get("release_eligible") is not False
            or provenance.get("commercial_certification") is not False
            or provenance.get("status") != "built"
            or provenance.get("source", {}).get("commit") != state["source_commit"]
            or provenance.get("source", {}).get("tree") != state["source_tree"]
            or provenance.get("source", {}).get("clean") is not True
            or provenance.get("environment", {}).get("STARRING_RUNTIME_BUILD_REVISION")
            != state["source_commit"]
        ):
            fail("candidate_publication_invalid")
        validate_dependency_snapshot(
            provenance.get("dependencies"), provenance["source"].get("root")
        )
        expected_inventory = {
            "candidate-spec.json",
            "provenance.json",
            "codex-worker",
            *OPERATOR_NAMES,
            *(destination for _candidate, _source, destination in REPO_ARTIFACTS),
        }
        if {path.name for path in final_bundle.iterdir()} != expected_inventory:
            fail("candidate_bundle_inventory_invalid")
        worker_root = final_bundle / "codex-worker"
        require_owned(worker_root, "candidate_worker_root", 0o555, directory=True)
        if {path.name for path in worker_root.iterdir()} != set(CODEX_WORKER_FILES):
            fail("candidate_worker_inventory_invalid")
        worker_records = provenance.get("worker", {}).get("files")
        if not isinstance(worker_records, dict) or set(worker_records) != set(CODEX_WORKER_FILES):
            fail("candidate_worker_inventory_invalid")
        for name in CODEX_WORKER_FILES:
            _raw, identity = read_file(
                worker_root / name,
                f"candidate_worker_{name}",
                expected_mode=0o444,
            )
            record = worker_records.get(name)
            if (
                not isinstance(record, dict)
                or record.get("artifact", {}).get("path") != str(worker_root / name)
                or record.get("artifact", {}).get("sha256") != identity["sha256"]
            ):
                fail("candidate_worker_changed")
        for name, candidate_record in spec["candidates"].items():
            if (
                not isinstance(candidate_record, dict)
                or set(candidate_record) != CANDIDATE_RECORD_FIELDS
                or not DIGEST.fullmatch(candidate_record.get("sha256", ""))
            ):
                fail("candidate_publication_invalid")
            path = pathlib.Path(candidate_record["path"])
            expected = 0o444 if name == "codex_worker" else 0o555
            _raw, identity = read_file(path, f"candidate_{name}", expected_mode=expected)
            if path != final_bundle / CANDIDATE_RELATIVE_PATHS[name]:
                fail("candidate_path_invalid")
            if name in {candidate for candidate, _source, _destination in REPO_ARTIFACTS}:
                provenance_record = provenance.get("artifacts", {}).get(name)
            elif name == "codex_worker":
                provenance_record = worker_records.get("worker.mjs")
            else:
                provenance_record = provenance.get("operators", {}).get(name)
            if (
                candidate_record["sha256"] != identity["sha256"]
                or identity["sha256"] != state["artifact_sha256"].get(name)
                or not isinstance(provenance_record, dict)
                or provenance_record.get("artifact", {}).get("path") != str(path)
                or provenance_record.get("artifact", {}).get("sha256")
                != identity["sha256"]
            ):
                fail("candidate_artifact_changed")
        return spec_path, provenance_path

    def publish(
        self,
        state_path,
        state,
        source,
        config,
        toolchain,
        commands,
        environment,
        dependencies,
        operator_before,
        worker_before,
        worker_tree_sha,
    ):
        self.require_dependencies(config, dependencies)
        build_root = pathlib.Path(state["build_root"])
        staging = pathlib.Path(state["publication_staging"])
        final_bundle = pathlib.Path(state["final_bundle"])
        try:
            staging.mkdir(mode=0o700)
        except OSError:
            fail("candidate_staging_create_failed")
        worker_destination = staging / "codex-worker"
        worker_destination.mkdir(mode=0o700)
        artifact_records = {}
        artifact_hashes = {}
        for candidate, source_relative, destination_name in REPO_ARTIFACTS:
            source_path = build_root / source_relative
            source_identity, artifact_identity = self.copy_verified(
                source_path, staging / destination_name, f"build_{candidate}", 0o555
            )
            artifact_identity["path"] = str(final_bundle / destination_name)
            artifact_records[candidate] = {"source": source_identity, "artifact": artifact_identity}
            artifact_hashes[candidate] = artifact_identity["sha256"]
        worker_records = {}
        worker_root = pathlib.Path(source["root"]) / "tools" / "codex-worker"
        for name in CODEX_WORKER_FILES:
            source_identity, artifact_identity = self.copy_verified(
                worker_root / name,
                worker_destination / name,
                f"worker_source_{name}",
                0o444,
                worker_before[name],
            )
            artifact_identity["path"] = str(final_bundle / "codex-worker" / name)
            worker_records[name] = {"source": source_identity, "artifact": artifact_identity}
        worker_destination.chmod(0o555)
        operator_records = {}
        for name in OPERATOR_NAMES:
            source_identity, artifact_identity = self.copy_verified(
                pathlib.Path(config["operators"][name]),
                staging / name,
                f"operator_{name}",
                0o555,
                operator_before[name],
            )
            artifact_identity["path"] = str(final_bundle / name)
            operator_records[name] = {"source": source_identity, "artifact": artifact_identity}
            artifact_hashes[name] = artifact_identity["sha256"]
        artifact_hashes["codex_worker"] = worker_records["worker.mjs"]["artifact"]["sha256"]
        candidate_paths = {
            candidate: str(final_bundle / destination)
            for candidate, _source, destination in REPO_ARTIFACTS
        }
        candidate_paths.update({name: str(final_bundle / name) for name in OPERATOR_NAMES})
        candidate_paths["codex_worker"] = str(final_bundle / "codex-worker" / "worker.mjs")
        provenance = self.provenance(
            source,
            toolchain,
            commands,
            environment,
            dependencies,
            artifact_records,
            worker_records,
            worker_tree_sha,
            operator_records,
            final_bundle,
        )
        provenance_payload = (canonical_json(provenance) + "\n").encode()
        spec = {
            "schema_version": CANDIDATE_SCHEMA_VERSION,
            "kind": CANDIDATE_KIND,
            "commit_sha": source["commit"],
            "source_tree_sha": source["tree"],
            "bundle": str(final_bundle),
            "provenance_sha256": sha256_bytes(provenance_payload),
            "candidates": {
                name: {"path": path, "sha256": artifact_hashes[name]}
                for name, path in candidate_paths.items()
            },
        }
        spec_payload = (canonical_json(spec) + "\n").encode()
        write_new(staging / "candidate-spec.json", spec_payload, 0o400)
        write_new(staging / "provenance.json", provenance_payload, 0o400)
        staging.chmod(0o555)
        fsync_directory(worker_destination, "candidate_worker_root")
        fsync_directory(staging, "candidate_staging")
        # Close the copy/publish race: the final rename is permitted only while
        # HEAD, the fixed compiler, operators, and all seven worker inputs still
        # match the identities embedded in this exact staging bundle.
        if self.source_snapshot() != source:
            fail("source_changed")
        if self.toolchain() != toolchain:
            fail("rust_toolchain_changed")
        if self.operator_identities(config) != operator_before:
            fail("operator_changed")
        self.require_dependencies(config, dependencies)
        _worker_root, final_workers, final_worker_sha = worker_identities(self.source_root)
        if final_workers != worker_before or final_worker_sha != worker_tree_sha:
            fail("worker_source_changed")
        self.save(
            state_path,
            state,
            status="publishing",
            phase="publishing",
            candidate_spec_sha256=sha256_bytes(spec_payload),
            provenance_sha256=sha256_bytes(provenance_payload),
            artifact_sha256=artifact_hashes,
        )
        if final_bundle.exists() or final_bundle.is_symlink():
            fail("candidate_final_collision")
        try:
            os.rename(staging, final_bundle)
        except OSError:
            fail("candidate_publish_failed")
        fsync_directory(final_bundle.parent, "candidate_output_parent")
        self.require_dependencies(config, dependencies)
        self.validate_bundle(final_bundle, state)
        self.save(state_path, state, phase="published")

    def remove_partial(self, path, state, label):
        path = pathlib.Path(path)
        parent = pathlib.Path(state["output_parent"])
        if path.parent != parent or not path.name.startswith((".build-", ".bundle-")):
            fail("candidate_cleanup_path_invalid")
        if not path.exists() and not path.is_symlink():
            return
        path = absolute_normal(path, label, True)
        try:
            metadata = path.lstat()
        except OSError:
            fail("candidate_cleanup_path_invalid")
        if path.is_symlink() or not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid != os.getuid():
            fail("candidate_cleanup_path_invalid")
        try:
            for current, directories, files in os.walk(path, topdown=False, followlinks=False):
                current_path = pathlib.Path(current)
                for name in files:
                    child = current_path / name
                    child_metadata = child.lstat()
                    if child_metadata.st_uid != os.getuid():
                        fail("candidate_cleanup_path_invalid")
                    if stat.S_ISLNK(child_metadata.st_mode):
                        child.unlink()
                    elif stat.S_ISREG(child_metadata.st_mode):
                        child.chmod(0o600)
                    else:
                        fail("candidate_cleanup_path_invalid")
                for name in directories:
                    child = current_path / name
                    child_metadata = child.lstat()
                    if child_metadata.st_uid != os.getuid():
                        fail("candidate_cleanup_path_invalid")
                    if stat.S_ISLNK(child_metadata.st_mode):
                        child.unlink()
                    elif stat.S_ISDIR(child_metadata.st_mode):
                        child.chmod(0o700)
                    else:
                        fail("candidate_cleanup_path_invalid")
                current_path.chmod(0o700)
            shutil.rmtree(path)
        except OSError:
            fail("candidate_cleanup_failed")
        fsync_directory(parent, "candidate_output_parent")

    def finish_cleanup(self, state_path, state):
        if state["build_processes_quiescent"] is not True:
            fail("candidate_manual_recovery_required")
        self.save(state_path, state, phase="cleanup")
        final = pathlib.Path(state["final_bundle"])
        if final.exists() or final.is_symlink():
            self.validate_bundle(final, state)
            self.remove_partial(state["build_root"], state, "candidate_build_root")
            self.remove_partial(state["publication_staging"], state, "candidate_publication_staging")
            self.save(state_path, state, status="passed", phase="complete", last_error=None)
            return "passed"
        self.remove_partial(state["publication_staging"], state, "candidate_publication_staging")
        self.remove_partial(state["build_root"], state, "candidate_build_root")
        self.save(state_path, state, status="cleaned", phase="complete")
        return "cleaned"

    def result(self, state_path, state, status=None, error_code=None):
        passed = (status or state["status"]) == "passed"
        bundle = pathlib.Path(state["final_bundle"])
        output = {
            "schema_version": 1,
            "kind": RESULT_KIND,
            "status": status or state["status"],
            "error_code": error_code,
            "state": str(state_path),
            "bundle": str(bundle) if passed else None,
            "candidate_spec": str(bundle / "candidate-spec.json") if passed else None,
            "provenance": str(bundle / "provenance.json") if passed else None,
            "source_commit": state["source_commit"],
            "release_eligible": False,
            "commercial_certification": False,
        }
        if set(output) != RESULT_FIELDS:
            fail("candidate_result_invalid")
        return output

    def build_locked(self, config_path):
        config_path, config, config_digest = read_private_json(config_path, "candidate_config")
        config, initial_operators = validate_config(config)
        source = self.source_snapshot()
        dependencies = load_dependency_snapshot(config["dependencies"], self.source_root)
        toolchain = self.toolchain()
        operators = self.operator_identities(config)
        if operators != initial_operators:
            fail("operator_changed")
        worker_root, workers, worker_tree_sha = worker_identities(self.source_root)
        del worker_root
        output_parent = ensure_private_directory(self.bundle_parent, "candidate_output_parent")
        build_id = f"d2ac-{secrets.token_hex(16)}"
        stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
        build_root = output_parent / f".build-{build_id}"
        publication = output_parent / f".bundle-{build_id}.tmp"
        final = output_parent / f"candidate-{stamp}-{source['commit'][:12]}-{secrets.token_hex(6)}"
        state_path = output_parent / f"state-{build_id}.json"
        state = {
            "schema_version": 1,
            "kind": STATE_KIND,
            "build_id": build_id,
            "status": "building",
            "phase": "initialized",
            "config_path": str(config_path),
            "config_sha256": config_digest,
            "source_root": source["root"],
            "source_commit": source["commit"],
            "source_tree": source["tree"],
            "output_parent": str(output_parent),
            "build_root": str(build_root),
            "publication_staging": str(publication),
            "final_bundle": str(final),
            "candidate_spec_sha256": None,
            "provenance_sha256": None,
            "artifact_sha256": {},
            "build_processes_quiescent": True,
            "last_error": None,
            "updated_at": utc_now(),
        }
        write_initial_state(state_path, state)
        try:
            build_root.mkdir(mode=0o700)
            fsync_directory(output_parent, "candidate_output_parent")
            self.save(state_path, state, phase="building")
            cargo = pathlib.Path(toolchain["tools"]["cargo"]["path"])
            commands = self.build_commands(build_root, cargo, dependencies)
            environment = self.build_environment(toolchain, source["commit"], build_root)
            self.require_dependencies(config, dependencies)
            self.verify_dependency_compatibility(
                cargo, environment, dependencies, state_path, state
            )
            self.require_dependencies(config, dependencies)
            for command in commands:
                self.require_dependencies(config, dependencies)
                self.save(state_path, state, build_processes_quiescent=False)
                completed = self.executor(command, self.source_root, environment)
                if getattr(completed, "process_group_quiescent", True) is True:
                    self.save(state_path, state, build_processes_quiescent=True)
                self.run_process_result(completed, "candidate_build")
                self.require_dependencies(config, dependencies)
            for candidate, source_relative, _destination in REPO_ARTIFACTS:
                self.validate_macho_linkage(
                    build_root / source_relative,
                    toolchain,
                    f"build_{candidate}",
                )
            self.save(state_path, state, phase="copying")
            after_source = self.source_snapshot()
            if after_source != source:
                fail("source_changed")
            if self.toolchain() != toolchain:
                fail("rust_toolchain_changed")
            if self.operator_identities(config) != operators:
                fail("operator_changed")
            _root, after_workers, after_worker_sha = worker_identities(self.source_root)
            if after_workers != workers or after_worker_sha != worker_tree_sha:
                fail("worker_source_changed")
            self.publish(
                state_path,
                state,
                source,
                config,
                toolchain,
                commands,
                environment,
                dependencies,
                operators,
                workers,
                worker_tree_sha,
            )
            self.finish_cleanup(state_path, state)
            return self.result(state_path, state, status="passed")
        except BaseException as error:
            code = "candidate_interrupted" if isinstance(error, KeyboardInterrupt) else (
                error.code if isinstance(error, CandidateError) else "candidate_internal_error"
            )
            self.save(state_path, state, status="failed", last_error=code)
            return self.result(state_path, state, status="failed", error_code=code)

    def build(self, config_path):
        with global_lock(self.lock_path):
            return self.build_locked(config_path)

    def resume_cleanup_locked(self, state_path):
        state_path, state = load_state(state_path)
        disposition = self.finish_cleanup(state_path, state)
        if disposition == "passed":
            return self.result(state_path, state, status="passed")
        return self.result(
            state_path,
            state,
            status="cleaned",
            error_code=state["last_error"] or "candidate_cleanup_only",
        )

    def resume_cleanup(self, state_path):
        with global_lock(self.lock_path):
            return self.resume_cleanup_locked(state_path)


class SafeArgumentParser(argparse.ArgumentParser):
    def error(self, _message):
        fail("cli_invalid")


def parser():
    root = SafeArgumentParser(prog="d2a-candidate")
    commands = root.add_subparsers(dest="command", required=True)
    build = commands.add_parser("build")
    build.add_argument("--config", required=True)
    cleanup = commands.add_parser("resume-cleanup")
    cleanup.add_argument("--state", required=True)
    return root


def error_result(code):
    return {
        "schema_version": 1,
        "kind": RESULT_KIND,
        "status": "failed",
        "error_code": code,
        "state": None,
        "bundle": None,
        "candidate_spec": None,
        "provenance": None,
        "source_commit": None,
        "release_eligible": False,
        "commercial_certification": False,
    }


def main(argv=None):
    try:
        arguments = parser().parse_args(argv)
        builder = CandidateBuilder()
        if arguments.command == "build":
            result = builder.build(arguments.config)
        else:
            result = builder.resume_cleanup(arguments.state)
    except CandidateError as error:
        result = error_result(error.code)
    except KeyboardInterrupt:
        result = error_result("candidate_interrupted")
    except SystemExit:
        raise
    except BaseException:
        result = error_result("candidate_internal_error")
    print(canonical_json(result))
    return 0 if result["status"] in {"passed", "cleaned"} else 1


if __name__ == "__main__":
    signal.signal(
        signal.SIGTERM,
        lambda _signal, _frame: (_ for _ in ()).throw(KeyboardInterrupt()),
    )
    raise SystemExit(main())
