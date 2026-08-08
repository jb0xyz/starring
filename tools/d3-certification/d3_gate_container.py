import hashlib
import json
import os
import pathlib
import secrets
import shlex
import shutil
import stat
import subprocess
import time
import urllib.parse


DOCKER_EXECUTABLE = pathlib.Path("/opt/homebrew/bin/docker")
DOCKER_SOCKET = pathlib.Path.home() / ".colima" / "default" / "docker.sock"
SHARED_ROOT = pathlib.Path.home().resolve()
GATE_DOCKERFILE = pathlib.Path(__file__).with_name("Dockerfile.gates")
GATE_DOCKERFILE_SHA256 = "4ce78042bd61b4ace0c505dde7ae41da01fd926cc92a6f6dd2e86f6825eaea15"
POSTGRES_IMAGE = "postgres@sha256:64154d0babcb1741988719e703419af0382b19953706149f9872fbd0f438efa8"
MAX_DOCKER_OUTPUT_BYTES = 1024 * 1024
CONTAINER_WAIT_SECONDS = 90
POSTGRES_READY_SECONDS = 90
GATE_MEMORY_LIMIT = "14g"
GATE_SCRATCH_SIZE = "3g"
GATE_TARGET_SIZE = "8g"
GATE_BUILD_JOBS = "1"
GATE_CHILD_OOM_EXIT = 247
GATE_PROFILE_DEV_DEBUG = "0"
GATE_PROFILE_TEST_DEBUG = "0"
MINIMUM_DAEMON_MEMORY_BYTES = 17 * 1024 * 1024 * 1024
MAX_BOOTSTRAP_BYTES = 4 * 1024 * 1024 * 1024
MAX_BOOTSTRAP_ENTRIES = 500000
MINIMUM_BOOTSTRAP_FREE_BYTES = 8 * 1024 * 1024 * 1024
TWILIGHT_GIT_URL = "https://github.com/twilight-rs/twilight.git"
TWILIGHT_GIT_REV = "b4ce13b727e7731b917576ad977300ab6926bb6b"
TWILIGHT_SOURCE_KEY = f"git+{TWILIGHT_GIT_URL}?rev={TWILIGHT_GIT_REV}"
ALLOWED_CARGO_LOCK_SOURCES = (
    "registry+https://github.com/rust-lang/crates.io-index",
    f"{TWILIGHT_SOURCE_KEY}#{TWILIGHT_GIT_REV}",
)
CARGO_LOCK_VALIDATOR = (
    "import pathlib,sys,tomllib\n"
    "allowed=set(sys.argv[3:])\n"
    "observed=[]\n"
    "for raw in sys.argv[1:3]:\n"
    " p=pathlib.Path(raw)\n"
    " if not p.is_file() or p.is_symlink() or p.stat().st_size>8388608: sys.exit(20)\n"
    " with p.open('rb') as handle: value=tomllib.load(handle)\n"
    " packages=value.get('package')\n"
    " if not isinstance(packages,list): sys.exit(21)\n"
    " for package in packages:\n"
    "  if not isinstance(package,dict): sys.exit(22)\n"
    "  source=package.get('source')\n"
    "  if source is not None:\n"
    "   if not isinstance(source,str) or source not in allowed: sys.exit(23)\n"
    "   observed.append(source)\n"
    "if not observed: sys.exit(24)\n"
)
REGISTRY_VENDOR_MATERIALIZER = (
    "import hashlib,json,os,pathlib,re,shutil,stat,sys,tomllib\n"
    "lock_path,source_parent,destination=map(pathlib.Path,sys.argv[1:])\n"
    "with lock_path.open('rb') as handle: lock=tomllib.load(handle)\n"
    "packages=[value for value in lock.get('package',[]) "
    "if str(value.get('source','')).startswith('registry+')]\n"
    "roots=[value for value in source_parent.iterdir() "
    "if value.is_dir() and not value.is_symlink()]\n"
    "if not packages or not roots or any(destination.iterdir()): sys.exit(30)\n"
    "for package in packages:\n"
    " name=package.get('name')\n"
    " version=package.get('version')\n"
    " checksum=package.get('checksum')\n"
    " if not isinstance(name,str) or re.fullmatch(r'[A-Za-z0-9_-]+',name) is None: sys.exit(31)\n"
    " if not isinstance(version,str) or re.fullmatch(r'[A-Za-z0-9.+_-]+',version) is None: sys.exit(31)\n"
    " if not isinstance(checksum,str) or re.fullmatch(r'[0-9a-f]{64}',checksum) is None: sys.exit(31)\n"
    " matches=[root/f'{name}-{version}' for root in roots "
    "if (root/f'{name}-{version}').is_dir() and not (root/f'{name}-{version}').is_symlink()]\n"
    " if len(matches)!=1: sys.exit(32)\n"
    " source=matches[0]\n"
    " target=destination/f'{name}-{version}'\n"
    " target.mkdir(mode=0o700)\n"
    " files={}\n"
    " for current,directories,names in os.walk(source,topdown=True,followlinks=False):\n"
    "  current_path=pathlib.Path(current)\n"
    "  relative=current_path.relative_to(source)\n"
    "  for directory in directories:\n"
    "   item=current_path/directory\n"
    "   metadata=item.lstat()\n"
    "   if not stat.S_ISDIR(metadata.st_mode) or item.is_symlink(): sys.exit(33)\n"
    "   (target/relative/directory).mkdir(mode=0o700)\n"
    "  for filename in names:\n"
    "   if filename=='.cargo-ok': continue\n"
    "   item=current_path/filename\n"
    "   metadata=item.lstat()\n"
    "   if not stat.S_ISREG(metadata.st_mode) or item.is_symlink(): sys.exit(34)\n"
    "   output=target/relative/filename\n"
    "   shutil.copyfile(item,output,follow_symlinks=False)\n"
    "   output.chmod(0o600)\n"
    "   relative_name=output.relative_to(target).as_posix()\n"
    "   files[relative_name]=hashlib.sha256(output.read_bytes()).hexdigest()\n"
    " payload={'files':dict(sorted(files.items())),'package':checksum}\n"
    " checksum_path=target/'.cargo-checksum.json'\n"
    " checksum_path.write_text(json.dumps(payload,sort_keys=True,separators=(',',':')),encoding='utf-8')\n"
    " checksum_path.chmod(0o600)\n"
)
RUNNER_POLICY = {
    "schema_version": 1,
    "common": {
        "capabilities": "none",
        "new_privileges": False,
        "read_only_root": True,
        "docker_socket_mounted": False,
        "host_network": False,
        "non_root": True,
        "pids_limit": 2048,
        "memory_limit": GATE_MEMORY_LIMIT,
        "memory_swap_limit": GATE_MEMORY_LIMIT,
        "memory_swappiness": 0,
        "minimum_daemon_memory_bytes": MINIMUM_DAEMON_MEMORY_BYTES,
        "cpu_limit": "4",
        "cargo_build_jobs": GATE_BUILD_JOBS,
        "cargo_profile_dev_debug": GATE_PROFILE_DEV_DEBUG,
        "cargo_profile_test_debug": GATE_PROFILE_TEST_DEBUG,
        "writable_mounts": [
            f"/scratch:tmpfs:{GATE_SCRATCH_SIZE}",
            "/tmp:tmpfs:512m",
            "/private/tmp:tmpfs:512m",
            "/run:tmpfs:16m",
        ],
        "cargo_target": f"owned-gate-attempt-disposable-tmpfs-volume:{GATE_TARGET_SIZE}",
    },
    "ordinary": {
        "gates": [[1, 8], [11, 16]],
        "network": "none",
        "read_only_mounts": [
            "/workspace",
            "/vendor",
            "/node_modules",
            "/bootstrap-bin",
            "/git",
        ],
    },
    "offline_install": {
        "gate": 9,
        "network": "none",
        "source": "isolated-package-projection",
        "candidate_worktree_mounted": False,
        "workspace_writable": False,
        "npm_cache": "sealed-bootstrap-copy-to-bounded-tmpfs",
        "audit": False,
    },
    "audit": {
        "gate": 10,
        "network": "bridge",
        "command": "npm --prefix eval/design-harness audit --audit-level=high",
        "read_only_mounts": ["/workspace"],
        "registry": "https://registry.npmjs.org/",
    },
    "postgres": {
        "gates": [17, 29],
        "server_network": "none",
        "gate_network": "container:postgres",
        "published_ports": False,
        "host_mounts": False,
        "storage": "tmpfs",
    },
    "bootstrap": {
        "cargo_network": "bridge",
        "cargo_lock_sources": list(ALLOWED_CARGO_LOCK_SOURCES),
        "cargo_lock_validation_network": "none",
        "npm_network": "bridge",
        "npm_registry": "https://registry.npmjs.org/",
        "maximum_host_staging_bytes": MAX_BOOTSTRAP_BYTES,
        "maximum_host_staging_entries": MAX_BOOTSTRAP_ENTRIES,
        "minimum_host_free_bytes": MINIMUM_BOOTSTRAP_FREE_BYTES,
    },
}


class GateContainerError(Exception):
    pass


def fail(code):
    raise GateContainerError(code)


def sha256_bytes(value):
    return hashlib.sha256(value).hexdigest()


def runner_policy_sha256():
    payload = json.dumps(
        RUNNER_POLICY,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
    return sha256_bytes(payload)


def runner_implementation_sha256():
    value = regular_bytes(
        pathlib.Path(__file__),
        "gate_container_implementation",
        256 * 1024,
    )
    return sha256_bytes(value)


def canonical_directory(path, label, mode=None):
    candidate = pathlib.Path(path)
    if not candidate.is_absolute() or os.path.realpath(candidate) != str(candidate):
        fail(f"{label}_path_invalid")
    try:
        metadata = candidate.lstat()
    except OSError as error:
        fail(f"{label}_unavailable:{error.__class__.__name__}")
    if (
        not stat.S_ISDIR(metadata.st_mode)
        or candidate.is_symlink()
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) & 0o022
        or mode is not None
        and stat.S_IMODE(metadata.st_mode) != mode
    ):
        fail(f"{label}_invalid")
    return candidate


def regular_bytes(path, label, maximum):
    candidate = pathlib.Path(path)
    try:
        metadata = candidate.lstat()
    except OSError as error:
        fail(f"{label}_unavailable:{error.__class__.__name__}")
    if (
        not stat.S_ISREG(metadata.st_mode)
        or candidate.is_symlink()
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) & 0o022
        or not 0 < metadata.st_size <= maximum
    ):
        fail(f"{label}_invalid")
    try:
        value = candidate.read_bytes()
    except OSError as error:
        fail(f"{label}_unavailable:{error.__class__.__name__}")
    if len(value) != metadata.st_size:
        fail(f"{label}_changed")
    return value


def docker_environment():
    try:
        executable = pathlib.Path(os.path.realpath(DOCKER_EXECUTABLE))
        executable_metadata = executable.lstat()
        socket_metadata = DOCKER_SOCKET.lstat()
    except OSError as error:
        fail(f"gate_container_runtime_unavailable:{error.__class__.__name__}")
    if (
        not stat.S_ISREG(executable_metadata.st_mode)
        or executable.is_symlink()
        or executable_metadata.st_uid != os.getuid()
        or stat.S_IMODE(executable_metadata.st_mode) & 0o022
        or not stat.S_IMODE(executable_metadata.st_mode) & 0o111
        or not stat.S_ISSOCK(socket_metadata.st_mode)
        or DOCKER_SOCKET.is_symlink()
        or socket_metadata.st_uid != os.getuid()
        or stat.S_IMODE(socket_metadata.st_mode) != 0o600
    ):
        fail("gate_container_runtime_identity_invalid")
    return executable, {
        "DOCKER_HOST": f"unix://{DOCKER_SOCKET}",
        "HOME": "/var/empty",
        "LANG": "C",
        "LC_ALL": "C",
        "PATH": "/usr/bin:/bin",
    }


def docker_run(arguments, label, timeout=CONTAINER_WAIT_SECONDS, input_bytes=None):
    executable, environment = docker_environment()
    try:
        result = subprocess.run(
            [str(executable), *arguments],
            env=environment,
            stdin=None if input_bytes is not None else subprocess.DEVNULL,
            input=input_bytes,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        fail(f"{label}_unavailable:{error.__class__.__name__}")
    if len(result.stdout) > MAX_DOCKER_OUTPUT_BYTES or len(result.stderr) > MAX_DOCKER_OUTPUT_BYTES:
        fail(f"{label}_output_invalid")
    return result


def docker_success(arguments, label, timeout=CONTAINER_WAIT_SECONDS, input_bytes=None):
    result = docker_run(arguments, label, timeout, input_bytes)
    if result.returncode != 0:
        fail(f"{label}_failed:{result.returncode}")
    return result.stdout


def inspect_object(kind, identity):
    result = docker_run(
        [kind, "inspect", "--format", "{{json .}}", identity],
        f"gate_container_{kind}_inspect",
    )
    if result.returncode != 0:
        return None
    try:
        return json.loads(result.stdout)
    except (UnicodeDecodeError, json.JSONDecodeError):
        fail(f"gate_container_{kind}_inspect_invalid")


def gate_image_identity():
    dockerfile = regular_bytes(GATE_DOCKERFILE, "gate_dockerfile", 64 * 1024)
    digest = sha256_bytes(dockerfile)
    if digest != GATE_DOCKERFILE_SHA256:
        fail("gate_dockerfile_digest_mismatch")
    tag = f"starring-d3-gates:{digest[:20]}"
    observed = inspect_object("image", tag)
    if observed is None:
        docker_success(
            [
                "build",
                "--network",
                "default",
                "--label",
                f"co.starring.d3.dockerfile-sha256={digest}",
                "--tag",
                tag,
                "-",
            ],
            "gate_container_image_build",
            3600,
            dockerfile,
        )
        observed = inspect_object("image", tag)
    if not isinstance(observed, dict):
        fail("gate_container_image_missing")
    image_id = observed.get("Id")
    labels = observed.get("Config", {}).get("Labels", {})
    if (
        not isinstance(image_id, str)
        or not image_id.startswith("sha256:")
        or len(image_id) != 71
        or observed.get("Os") != "linux"
        or observed.get("Architecture") != "arm64"
        or labels.get("co.starring.d3.dockerfile-sha256") != digest
    ):
        fail("gate_container_image_identity_invalid")
    versions = docker_success(
        [
            "run",
            "--rm",
            "--network",
            "none",
            "--read-only",
            "--cap-drop",
            "ALL",
            "--security-opt",
            "no-new-privileges",
            image_id,
            "/bin/sh",
            "-c",
            "rustc --version; cargo --version; rustfmt --version; cargo clippy --version; node --version; npm --version",
        ],
        "gate_container_toolchain_probe",
        120,
    )
    try:
        lines = versions.decode("utf-8").strip().splitlines()
    except UnicodeDecodeError:
        fail("gate_container_toolchain_invalid")
    if (
        len(lines) != 6
        or not lines[0].startswith("rustc 1.97.0 ")
        or not lines[1].startswith("cargo 1.97.0 ")
        or not lines[2].startswith("rustfmt 1.")
        or not lines[3].startswith("clippy 0.1.97 ")
        or lines[4] != "v26.5.0"
        or not lines[5].startswith("11.")
    ):
        fail("gate_container_toolchain_invalid")
    daemon_memory_raw = docker_success(
        ["info", "--format", "{{json .MemTotal}}"],
        "gate_container_daemon_memory",
        120,
    )
    try:
        daemon_memory_bytes = json.loads(daemon_memory_raw)
    except (UnicodeDecodeError, json.JSONDecodeError):
        fail("gate_container_daemon_memory_invalid")
    if (
        type(daemon_memory_bytes) is not int
        or daemon_memory_bytes < MINIMUM_DAEMON_MEMORY_BYTES
    ):
        fail("gate_container_daemon_memory_insufficient")
    return {
        "schema_version": 1,
        "kind": "starring.d3.gate-container-runtime.v1",
        "dockerfile_sha256": digest,
        "image_id": image_id,
        "postgres_image": POSTGRES_IMAGE,
        "runner_policy_sha256": runner_policy_sha256(),
        "runner_implementation_sha256": runner_implementation_sha256(),
        "daemon_memory_bytes": daemon_memory_bytes,
        "tool_versions": lines,
    }


def container_labels(root, index, attempt, role):
    owner = sha256_bytes(str(root).encode("utf-8"))
    return {
        "co.starring.d3.owner": owner,
        "co.starring.d3.gate": str(index),
        "co.starring.d3.attempt": str(attempt),
        "co.starring.d3.role": role,
    }


def container_name(root, index, attempt, role):
    owner = sha256_bytes(str(root).encode("utf-8"))[:12]
    suffix = "pg" if role == "postgres" else "gate"
    return f"starring-d3-{owner}-g{index:02d}-a{attempt}-{suffix}"


def gate_cache_identity(root, index, attempt):
    owner = sha256_bytes(str(root).encode("utf-8"))
    return (
        f"starring-d3-{owner[:12]}-g{index:02d}-a{attempt}-cargo-target",
        {
            "co.starring.d3.owner": owner,
            "co.starring.d3.gate": str(index),
            "co.starring.d3.attempt": str(attempt),
            "co.starring.d3.role": "cargo-target",
        },
    )


def require_volume_labels(value, expected):
    labels = value.get("Labels", {}) if isinstance(value, dict) else {}
    options = value.get("Options", {}) if isinstance(value, dict) else {}
    expected_options = {
        "device": "tmpfs",
        "o": (
            f"size={GATE_TARGET_SIZE},uid={os.getuid()},gid={os.getgid()},mode=0700"
        ),
        "type": "tmpfs",
    }
    if (
        any(labels.get(name) != expected_value for name, expected_value in expected.items())
        or options != expected_options
    ):
        fail("gate_container_volume_owner_mismatch")


def ensure_gate_cache(root, index, attempt):
    name, labels = gate_cache_identity(root, index, attempt)
    observed = inspect_object("volume", name)
    if observed is None:
        arguments = [
            "volume",
            "create",
            "--driver",
            "local",
            "--opt",
            "type=tmpfs",
            "--opt",
            "device=tmpfs",
            "--opt",
            (
                f"o=size={GATE_TARGET_SIZE},uid={os.getuid()},"
                f"gid={os.getgid()},mode=0700"
            ),
        ]
        for key, value in sorted(labels.items()):
            arguments.extend(("--label", f"{key}={value}"))
        arguments.append(name)
        docker_success(arguments, "gate_container_volume_create")
        observed = inspect_object("volume", name)
    require_volume_labels(observed, labels)
    return name


def remove_gate_cache(root, index, attempt):
    name, labels = gate_cache_identity(root, index, attempt)
    observed = inspect_object("volume", name)
    if observed is None:
        return
    require_volume_labels(observed, labels)
    docker_success(
        ["volume", "rm", "--force", name],
        "gate_container_volume_remove",
        300,
    )
    if inspect_object("volume", name) is not None:
        fail("gate_container_volume_remove_incomplete")


def cleanup_gate_attempt(root, index, attempt, containers):
    first_error = None
    try:
        remove_owned_containers(containers)
    except GateContainerError as error:
        first_error = error
    try:
        remove_gate_cache(root, index, attempt)
    except GateContainerError as error:
        if first_error is None:
            first_error = error
    if first_error is not None:
        raise first_error


def require_container_labels(value, expected):
    labels = value.get("Config", {}).get("Labels", {}) if isinstance(value, dict) else {}
    if any(labels.get(name) != expected_value for name, expected_value in expected.items()):
        fail("gate_container_owner_mismatch")


def remove_owned_container(name, labels):
    observed = inspect_object("container", name)
    if observed is None:
        return
    require_container_labels(observed, labels)
    docker_success(
        ["container", "rm", "--force", "--volumes", name],
        "gate_container_remove",
        120,
    )
    if inspect_object("container", name) is not None:
        fail("gate_container_remove_incomplete")


def remove_owned_containers(containers):
    first_error = None
    for name, labels in containers:
        try:
            remove_owned_container(name, labels)
        except GateContainerError as error:
            if first_error is None:
                first_error = error
    if first_error is not None:
        raise first_error


def mount_argument(source, destination, read_only):
    path = pathlib.Path(source)
    if (
        not path.is_absolute()
        or os.path.realpath(path) != str(path)
        or path != SHARED_ROOT
        and SHARED_ROOT not in path.parents
    ):
        fail("gate_container_mount_invalid")
    value = f"type=bind,src={path},dst={destination}"
    return f"{value},readonly" if read_only else value


def common_container_arguments(name, labels, image_id, user, network):
    arguments = [
        "container",
        "create",
        "--name",
        name,
        "--network",
        network,
        "--read-only",
        "--log-driver",
        "none",
        "--init",
        "--cap-drop",
        "ALL",
        "--security-opt",
        "no-new-privileges",
        "--pids-limit",
        "2048",
        "--memory",
        GATE_MEMORY_LIMIT,
        "--memory-swap",
        GATE_MEMORY_LIMIT,
        "--memory-swappiness",
        "0",
        "--cpus",
        "4",
        "--user",
        user,
        "--tmpfs",
        f"/tmp:rw,noexec,nosuid,size=512m,uid={os.getuid()},gid={os.getgid()},mode=0700",
        "--tmpfs",
        f"/private/tmp:rw,noexec,nosuid,size=512m,uid={os.getuid()},gid={os.getgid()},mode=0700",
        "--tmpfs",
        f"/run:rw,noexec,nosuid,size=16m,uid={os.getuid()},gid={os.getgid()},mode=0700",
        "--tmpfs",
        (
            f"/scratch:rw,noexec,nosuid,size={GATE_SCRATCH_SIZE},"
            f"uid={os.getuid()},gid={os.getgid()},mode=0700"
        ),
    ]
    for key, value in sorted(labels.items()):
        arguments.extend(("--label", f"{key}={value}"))
    arguments.append(image_id)
    return arguments


def install_node_dependencies(root, staging, image_id):
    canonical_directory(root, "gate_container_state_root", 0o700)
    canonical_directory(staging, "gate_container_bootstrap_staging", 0o700)
    owner = sha256_bytes(str(root).encode("utf-8"))
    name = f"starring-d3-{owner[:12]}-bootstrap"
    labels = {
        "co.starring.d3.owner": owner,
        "co.starring.d3.gate": "0",
        "co.starring.d3.attempt": "1",
        "co.starring.d3.role": "bootstrap",
    }
    remove_owned_container(name, labels)
    arguments = common_container_arguments(
        name,
        labels,
        image_id,
        f"{os.getuid()}:{os.getgid()}",
        "bridge",
    )[:-1]
    arguments.extend(
        (
            "--mount",
            mount_argument(staging, "/stage", False),
            "--env",
            "HOME=/stage/home",
            "--env",
            "NPM_CONFIG_BIN_LINKS=false",
            "--env",
            "NPM_CONFIG_CACHE=/stage/npm-cache",
            "--env",
            "NPM_CONFIG_FUND=false",
            "--env",
            "NPM_CONFIG_INSTALL_LINKS=false",
            "--env",
            "NPM_CONFIG_REGISTRY=https://registry.npmjs.org/",
            "--env",
            "NPM_CONFIG_UPDATE_NOTIFIER=false",
            image_id,
            "/usr/local/bin/npm",
            "ci",
            "--ignore-scripts",
            "--bin-links=false",
            "--install-links=false",
            "--prefix",
            "/stage/node-stage",
        )
    )
    try:
        docker_success(arguments, "gate_bootstrap_container_create")
        code = start_attached_container(name, labels, 3600, staging)
        if code != 0:
            fail(f"gate_bootstrap_npm_failed:{code}")
    finally:
        remove_owned_container(name, labels)


def validate_cargo_lock_sources(root, worktree, image_id):
    canonical_directory(root, "gate_container_state_root", 0o700)
    canonical_directory(worktree, "gate_container_worktree")
    owner = sha256_bytes(str(root).encode("utf-8"))
    name = f"starring-d3-{owner[:12]}-cargo-locks"
    labels = {
        "co.starring.d3.owner": owner,
        "co.starring.d3.gate": "0",
        "co.starring.d3.attempt": "1",
        "co.starring.d3.role": "cargo-locks",
    }
    remove_owned_container(name, labels)
    arguments = common_container_arguments(
        name,
        labels,
        image_id,
        f"{os.getuid()}:{os.getgid()}",
        "none",
    )[:-1]
    arguments.extend(
        (
            "--mount",
            mount_argument(worktree, "/workspace", True),
            image_id,
            "/usr/bin/python3",
            "-c",
            CARGO_LOCK_VALIDATOR,
            "/workspace/Cargo.lock",
            "/workspace/tools/d2-certification-transport/Cargo.lock",
            *ALLOWED_CARGO_LOCK_SOURCES,
        )
    )
    try:
        docker_success(arguments, "gate_cargo_lock_validator_create")
        code = start_attached_container(name, labels, 120)
        if code != 0:
            fail(f"gate_cargo_lock_source_invalid:{code}")
    finally:
        remove_owned_container(name, labels)


def run_bootstrap_cargo_container(root, worktree, staging, image_id, role, network, command):
    canonical_directory(root, "gate_container_state_root", 0o700)
    canonical_directory(worktree, "gate_container_worktree")
    canonical_directory(staging, "gate_container_bootstrap_staging", 0o700)
    owner = sha256_bytes(str(root).encode("utf-8"))
    name = f"starring-d3-{owner[:12]}-{role}"
    labels = {
        "co.starring.d3.owner": owner,
        "co.starring.d3.gate": "0",
        "co.starring.d3.attempt": "1",
        "co.starring.d3.role": role,
    }
    remove_owned_container(name, labels)
    arguments = common_container_arguments(
        name,
        labels,
        image_id,
        f"{os.getuid()}:{os.getgid()}",
        network,
    )[:-1]
    arguments.extend(
        (
            "--workdir",
            "/workspace",
            "--mount",
            mount_argument(worktree, "/workspace", True),
            "--mount",
            mount_argument(staging, "/stage", False),
            "--env",
            "CARGO_HOME=/stage/cargo-home",
            "--env",
            "CARGO_INCREMENTAL=0",
            "--env",
            "GIT_CONFIG_NOSYSTEM=1",
            "--env",
            "GIT_TERMINAL_PROMPT=0",
            "--env",
            "HOME=/stage/home",
            "--env",
            "LANG=C",
            "--env",
            "LC_ALL=C",
            "--env",
            "PATH=/usr/local/cargo/bin:/usr/local/bin:/usr/bin:/bin",
            "--env",
            "RUSTUP_HOME=/usr/local/rustup",
            "--env",
            "RUSTUP_TOOLCHAIN=1.97.0",
            "--env",
            "TMPDIR=/stage/tmp",
            image_id,
            "/bin/sh",
            "-c",
            command,
        )
    )
    try:
        docker_success(arguments, f"gate_{role}_create")
        code = start_attached_container(name, labels, 3600, staging)
        if code != 0:
            fail(f"gate_{role}_failed:{code}")
    finally:
        remove_owned_container(name, labels)


def fetch_cargo_vendor(root, worktree, staging, image_id):
    run_bootstrap_cargo_container(
        root,
        worktree,
        staging,
        image_id,
        "cargo-bootstrap",
        "bridge",
        (
            "umask 077 && mkdir -p /stage/vendor/workspace "
            "/stage/vendor/transport && "
            "cargo fetch --locked --manifest-path /workspace/Cargo.toml && "
            "cargo vendor --locked --manifest-path "
            "/workspace/tools/d2-certification-transport/Cargo.toml "
            "/stage/vendor/transport > /stage/transport-cargo-vendor-config.txt"
        ),
    )


def materialize_workspace_vendor(root, worktree, staging, image_id):
    command = " ".join(
        (
            "/usr/bin/python3",
            "-c",
            shlex.quote(REGISTRY_VENDOR_MATERIALIZER),
            "/workspace/Cargo.lock",
            "/stage/cargo-home/registry/src",
            "/stage/vendor/workspace",
        )
    )
    run_bootstrap_cargo_container(
        root,
        worktree,
        staging,
        image_id,
        "cargo-materialize",
        "none",
        command,
    )


def verify_cargo_vendor(root, worktree, staging, image_id):
    run_bootstrap_cargo_container(
        root,
        worktree,
        staging,
        image_id,
        "cargo-verify",
        "none",
        (
            "cargo metadata --config /stage/workspace-cargo-config.toml "
            "--locked --offline --format-version=1 "
            "--manifest-path /workspace/Cargo.toml >/dev/null && "
            "cargo metadata --config /stage/transport-staging-cargo-config.toml "
            "--locked --offline --format-version=1 --manifest-path "
            "/workspace/tools/d2-certification-transport/Cargo.toml >/dev/null"
        ),
    )


def validate_bind_roundtrip(root, image_id):
    canonical_directory(root, "gate_container_state_root", 0o700)
    if root != SHARED_ROOT and SHARED_ROOT not in root.parents:
        fail("gate_container_state_root_unshared")
    probe = root / f".gate-container-bind-probe-{secrets.token_hex(8)}"
    try:
        probe.mkdir(mode=0o700)
    except OSError as error:
        fail(f"gate_container_bind_probe_unavailable:{error.__class__.__name__}")
    canonical_directory(probe, "gate_container_bind_probe", 0o700)
    owner = sha256_bytes(str(root).encode("utf-8"))
    name = f"starring-d3-{owner[:12]}-bind"
    labels = {
        "co.starring.d3.owner": owner,
        "co.starring.d3.gate": "0",
        "co.starring.d3.attempt": "1",
        "co.starring.d3.role": "bind",
    }
    remove_owned_container(name, labels)
    arguments = common_container_arguments(
        name,
        labels,
        image_id,
        f"{os.getuid()}:{os.getgid()}",
        "none",
    )[:-1]
    arguments.extend(
        (
            "--mount",
            mount_argument(root, "/state", True),
            "--mount",
            mount_argument(probe, "/write", False),
            image_id,
            "/usr/bin/python3",
            "-c",
            (
                "import os,pathlib,sys;"
                "sys.exit(20) if not pathlib.Path('/state/state.json').is_file() else None;"
                "fd=os.open('/write/roundtrip',os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600);"
                "os.write(fd,b'starring-d3-bind-ok');os.fsync(fd);os.close(fd)"
            ),
        )
    )
    try:
        docker_success(arguments, "gate_container_bind_probe_create")
        code = start_attached_container(name, labels, 120)
        if code != 0:
            fail(f"gate_container_bind_probe_failed:{code}")
        output = probe / "roundtrip"
        flags = os.O_RDONLY
        if hasattr(os, "O_NOFOLLOW"):
            flags |= os.O_NOFOLLOW
        try:
            before = output.lstat()
            descriptor = os.open(output, flags)
        except OSError as error:
            fail(f"gate_container_bind_probe_unavailable:{error.__class__.__name__}")
        try:
            observed = os.fstat(descriptor)
            value = os.read(descriptor, 64)
            after = os.fstat(descriptor)
        finally:
            os.close(descriptor)
        if (
            not stat.S_ISREG(before.st_mode)
            or output.is_symlink()
            or before.st_uid != os.getuid()
            or before.st_nlink != 1
            or stat.S_IMODE(before.st_mode) != 0o600
            or (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns)
            != (observed.st_dev, observed.st_ino, observed.st_size, observed.st_mtime_ns)
            or (observed.st_dev, observed.st_ino, observed.st_size, observed.st_mtime_ns)
            != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns)
            or value != b"starring-d3-bind-ok"
        ):
            fail("gate_container_bind_probe_invalid")
    finally:
        try:
            remove_owned_container(name, labels)
        finally:
            discard_bind_probe(root, probe)


def discard_bind_probe(root, probe):
    if probe.parent != root or not probe.name.startswith(".gate-container-bind-probe-"):
        fail("gate_container_bind_probe_path_invalid")
    flags = os.O_RDONLY
    if hasattr(os, "O_DIRECTORY"):
        flags |= os.O_DIRECTORY
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        before = probe.lstat()
        descriptor = os.open(probe, flags)
    except OSError as error:
        fail(f"gate_container_bind_probe_cleanup_unavailable:{error.__class__.__name__}")
    try:
        observed = os.fstat(descriptor)
        if (
            not stat.S_ISDIR(before.st_mode)
            or probe.is_symlink()
            or before.st_uid != os.getuid()
            or (before.st_dev, before.st_ino) != (observed.st_dev, observed.st_ino)
        ):
            fail("gate_container_bind_probe_cleanup_invalid")
        for name in os.listdir(descriptor):
            metadata = os.stat(name, dir_fd=descriptor, follow_symlinks=False)
            if (
                name != "roundtrip"
                or not stat.S_ISREG(metadata.st_mode)
                or stat.S_ISLNK(metadata.st_mode)
                or metadata.st_uid != os.getuid()
                or metadata.st_nlink != 1
            ):
                fail("gate_container_bind_probe_cleanup_invalid")
            os.unlink(name, dir_fd=descriptor)
        os.fsync(descriptor)
    finally:
        os.close(descriptor)
    try:
        probe.rmdir()
    except OSError as error:
        fail(f"gate_container_bind_probe_cleanup_failed:{error.__class__.__name__}")
    descriptor = os.open(root, os.O_RDONLY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def bounded_tree_usage(root):
    stack = [pathlib.Path(root)]
    entries = 0
    total_bytes = 0
    while stack:
        current = stack.pop()
        try:
            iterator = os.scandir(current)
        except FileNotFoundError:
            continue
        except OSError as error:
            fail(f"gate_bootstrap_monitor_unavailable:{error.__class__.__name__}")
        with iterator:
            for entry in iterator:
                try:
                    metadata = entry.stat(follow_symlinks=False)
                except FileNotFoundError:
                    continue
                except OSError as error:
                    fail(
                        f"gate_bootstrap_monitor_unavailable:{error.__class__.__name__}"
                    )
                entries += 1
                if entries > MAX_BOOTSTRAP_ENTRIES:
                    fail("gate_bootstrap_staging_limit_exceeded")
                if stat.S_ISDIR(metadata.st_mode) and not stat.S_ISLNK(
                    metadata.st_mode
                ):
                    stack.append(pathlib.Path(entry.path))
                else:
                    total_bytes += metadata.st_size
                    if total_bytes > MAX_BOOTSTRAP_BYTES:
                        fail("gate_bootstrap_staging_limit_exceeded")
    return entries, total_bytes


def require_bootstrap_capacity(root):
    canonical_directory(root, "gate_container_state_root", 0o700)
    try:
        free = shutil.disk_usage(root).free
    except OSError as error:
        fail(f"gate_bootstrap_capacity_unavailable:{error.__class__.__name__}")
    if free < MINIMUM_BOOTSTRAP_FREE_BYTES:
        fail("gate_bootstrap_capacity_insufficient")
    return free


def start_attached_container(name, labels, timeout, monitored_root=None):
    executable, environment = docker_environment()
    try:
        process = subprocess.Popen(
            [str(executable), "container", "start", "--attach", name],
            env=environment,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
    except OSError as error:
        fail(f"gate_container_start_unavailable:{error.__class__.__name__}")
    try:
        deadline = time.monotonic() + timeout
        while process.poll() is None:
            if monitored_root is not None:
                bounded_tree_usage(monitored_root)
                require_bootstrap_capacity(pathlib.Path(monitored_root).parent)
            if time.monotonic() >= deadline:
                fail("gate_container_timeout")
            time.sleep(0.1)
        if monitored_root is not None:
            bounded_tree_usage(monitored_root)
            require_bootstrap_capacity(pathlib.Path(monitored_root).parent)
        observed = inspect_object("container", name)
        require_container_labels(observed, labels)
        state = observed.get("State", {})
        exit_code = state.get("ExitCode")
        if state.get("Running") is not False or type(exit_code) is not int:
            fail("gate_container_exit_invalid")
        if state.get("OOMKilled") is True:
            fail("gate_container_oom")
        if exit_code == GATE_CHILD_OOM_EXIT:
            fail("gate_container_child_oom")
        return exit_code
    except BaseException:
        remove_owned_container(name, labels)
        if process.poll() is None:
            process.kill()
            process.wait(timeout=CONTAINER_WAIT_SECONDS)
        raise


def postgres_database_name(database_url):
    try:
        parsed = urllib.parse.urlsplit(database_url)
        database = urllib.parse.unquote(parsed.path[1:], errors="strict")
    except (UnicodeDecodeError, ValueError):
        fail("gate_container_database_invalid")
    if not database or not all(character.islower() or character.isdigit() or character == "_" for character in database):
        fail("gate_container_database_invalid")
    return database


def start_postgres(root, index, attempt, database_url):
    name = container_name(root, index, attempt, "postgres")
    labels = container_labels(root, index, attempt, "postgres")
    remove_owned_container(name, labels)
    database = postgres_database_name(database_url)
    password = secrets.token_hex(32)
    arguments = [
        "container",
        "create",
        "--name",
        name,
        "--network",
        "none",
        "--read-only",
        "--log-driver",
        "none",
        "--security-opt",
        "no-new-privileges",
        "--pids-limit",
        "1024",
        "--memory",
        "2g",
        "--cpus",
        "2",
        "--tmpfs",
        "/var/lib/postgresql/data:rw,noexec,nosuid,size=1536m",
        "--tmpfs",
        "/var/run/postgresql:rw,noexec,nosuid,size=32m",
        "--env",
        f"POSTGRES_DB={database}",
        "--env",
        f"POSTGRES_PASSWORD={password}",
        "--env",
        "POSTGRES_USER=postgres",
    ]
    for key, value in sorted(labels.items()):
        arguments.extend(("--label", f"{key}={value}"))
    arguments.append(POSTGRES_IMAGE)
    docker_success(arguments, "gate_postgres_create")
    docker_success(["container", "start", name], "gate_postgres_start")
    deadline = time.monotonic() + POSTGRES_READY_SECONDS
    while time.monotonic() < deadline:
        result = docker_run(
            [
                "container",
                "exec",
                "--env",
                f"PGPASSWORD={password}",
                name,
                "pg_isready",
                "--host",
                "127.0.0.1",
                "--username",
                "postgres",
                "--dbname",
                database,
            ],
            "gate_postgres_ready",
            10,
        )
        if result.returncode == 0:
            return name, labels, f"postgres://postgres:{password}@127.0.0.1:5432/{database}"
        time.sleep(0.25)
    remove_owned_container(name, labels)
    fail("gate_postgres_not_ready")


def fixed_gate_command(index, command):
    if index == 9:
        selected = (
            "umask 077 && mkdir -p /scratch/package /scratch/npm-cache && "
            "cp -R /npm-cache/. /scratch/npm-cache/ && "
            "cp /workspace/eval/design-harness/package.json "
            "/workspace/eval/design-harness/package-lock.json /scratch/package/ && "
            "npm ci --ignore-scripts --bin-links=false --install-links=false "
            "--offline --prefix /scratch/package"
        )
    elif index == 10:
        selected = "npm --prefix eval/design-harness audit --audit-level=high"
    else:
        selected = (
            "umask 077 && mkdir -p /scratch/cargo-home && "
            "cp /gate-cargo-config.toml /scratch/cargo-home/config.toml && "
            "chmod 0400 /scratch/cargo-home/config.toml && "
            f"{command}"
        )
    return (
        "gate_oom_before=$(/usr/bin/sed -n 's/^oom_kill //p' "
        "/sys/fs/cgroup/memory.events) || exit 246; "
        "case \"$gate_oom_before\" in ''|*[!0-9]*) exit 246;; esac; "
        f"( {selected} ); gate_status=$?; "
        "gate_oom_after=$(/usr/bin/sed -n 's/^oom_kill //p' "
        "/sys/fs/cgroup/memory.events) || exit 246; "
        "case \"$gate_oom_after\" in ''|*[!0-9]*) exit 246;; esac; "
        f"if [ \"$gate_oom_after\" -gt \"$gate_oom_before\" ]; then exit {GATE_CHILD_OOM_EXIT}; fi; "
        "exit \"$gate_status\""
    )


def create_gate_container(
    root,
    source,
    bootstrap,
    image_id,
    index,
    attempt,
    command,
    network,
    database_url,
):
    name = container_name(root, index, attempt, "gate")
    labels = container_labels(root, index, attempt, "gate")
    remove_owned_container(name, labels)
    cache = None
    if index not in (9, 10):
        cache = ensure_gate_cache(root, index, attempt)
    arguments = common_container_arguments(
        name,
        labels,
        image_id,
        f"{os.getuid()}:{os.getgid()}",
        network,
    )[:-1]
    arguments.extend(
        (
            "--workdir",
            "/workspace",
            "--mount",
            mount_argument(source, "/workspace", True),
        )
    )
    if index == 9:
        arguments.extend(
            (
                "--mount",
                mount_argument(bootstrap / "npm-cache", "/npm-cache", True),
            )
        )
    elif index != 10:
        arguments.extend(
            (
                "--mount",
                f"type=volume,src={cache},dst=/scratch/target",
                "--mount",
                mount_argument(
                    bootstrap
                    / (
                        "transport-cargo-config.toml"
                        if 14 <= index <= 16
                        else "cargo-config.toml"
                    ),
                    "/gate-cargo-config.toml",
                    True,
                ),
                "--mount",
                mount_argument(bootstrap / "vendor", "/vendor", True),
                "--mount",
                mount_argument(bootstrap / "node-stage" / "node_modules", "/node_modules", True),
                "--mount",
                mount_argument(bootstrap / "bin", "/bootstrap-bin", True),
                "--mount",
                mount_argument(bootstrap / "git", "/git", True),
            )
        )
    environment = {
        "CARGO_BUILD_JOBS": GATE_BUILD_JOBS,
        "CARGO_HOME": "/scratch/cargo-home",
        "CARGO_INCREMENTAL": "0",
        "CARGO_NET_OFFLINE": "true",
        "CARGO_PROFILE_DEV_DEBUG": GATE_PROFILE_DEV_DEBUG,
        "CARGO_PROFILE_TEST_DEBUG": GATE_PROFILE_TEST_DEBUG,
        "CARGO_TARGET_DIR": "/scratch/target",
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_OPTIONAL_LOCKS": "0",
        "GIT_TERMINAL_PROMPT": "0",
        "HOME": "/scratch/home",
        "LANG": "C",
        "LC_ALL": "C",
        "NODE_PATH": "/node_modules",
        "NPM_CONFIG_BIN_LINKS": "false",
        "NPM_CONFIG_CACHE": "/scratch/npm-cache",
        "NPM_CONFIG_AUDIT": "true" if index == 10 else "false",
        "NPM_CONFIG_FUND": "false",
        "NPM_CONFIG_INSTALL_LINKS": "false",
        "NPM_CONFIG_OFFLINE": "false" if index == 10 else "true",
        "NPM_CONFIG_REGISTRY": "https://registry.npmjs.org/",
        "NPM_CONFIG_UPDATE_NOTIFIER": "false",
        "PATH": "/bootstrap-bin:/usr/local/cargo/bin:/usr/local/bin:/usr/bin:/bin",
        "PYTHONDONTWRITEBYTECODE": "1",
        "RUSTUP_HOME": "/usr/local/rustup",
        "RUSTUP_TOOLCHAIN": "1.97.0",
        "SHELL": "/bin/sh",
        "STARRING_D2_TEST_RUNTIME_PARENT": "/scratch/tmp/d2-runtime",
        "TMPDIR": "/scratch/tmp",
        "USER": "starring",
        "XDG_CACHE_HOME": "/scratch/xdg-cache",
        "XDG_CONFIG_HOME": "/scratch/xdg-config",
    }
    if database_url is not None:
        environment["STARRING_TEST_DATABASE_URL"] = database_url
    for key, value in sorted(environment.items()):
        arguments.extend(("--env", f"{key}={value}"))
    arguments.extend(
        (
            image_id,
            "/bin/sh",
            "-c",
            fixed_gate_command(index, command),
        )
    )
    docker_success(arguments, "gate_container_create")
    return name, labels


def run_gate(root, source, bootstrap, runtime, index, attempt, command, timeout, database_url):
    canonical_directory(root, "gate_container_state_root", 0o700)
    canonical_directory(source, "gate_container_source")
    canonical_directory(bootstrap, "gate_container_bootstrap", 0o555)
    image_id = runtime.get("image_id") if isinstance(runtime, dict) else None
    if not isinstance(image_id, str) or inspect_object("image", image_id) is None:
        fail("gate_container_image_unavailable")
    gate_name = container_name(root, index, attempt, "gate")
    gate_labels = container_labels(root, index, attempt, "gate")
    postgres_name = container_name(root, index, attempt, "postgres")
    postgres_labels = container_labels(root, index, attempt, "postgres")
    owned = (
        (gate_name, gate_labels),
        (postgres_name, postgres_labels),
    )
    cleanup_gate_attempt(root, index, attempt, owned)
    try:
        selected_database_url = None
        network = "bridge" if index == 10 else "none"
        if index > 16:
            postgres = start_postgres(root, index, attempt, database_url)
            network = f"container:{postgres_name}"
            selected_database_url = postgres[2]
        create_gate_container(
            root,
            source,
            bootstrap,
            image_id,
            index,
            attempt,
            command,
            network,
            selected_database_url,
        )
        return start_attached_container(gate_name, gate_labels, timeout)
    finally:
        cleanup_gate_attempt(root, index, attempt, owned)
