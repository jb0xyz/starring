import os
import pathlib
import secrets
import stat
import subprocess

from d3_candidate_io import (
    CandidateBundleError,
    absolute_path,
    canonical_json,
    copy_snapshot,
    directory_identity,
    fail,
    file_identity,
    fsync_directory,
    normalized_file_identity,
    read_json,
    remove_tree_descriptor,
    rename_exclusive,
    require_directory,
    seal_record,
    sha256_bytes,
    source_tree_digest,
    system_file_identity,
    unlink_owned_regular,
    verify_directory_identity,
    verify_sealed_record,
    write_new_file,
    write_new_file_atomic,
)


SCHEMA_VERSION = 1
MAX_GIT_TREE_BYTES = 16 * 1024 * 1024
SAFE_ENVIRONMENT_NAMES = (
    "CARGO_HOME",
    "HOME",
    "LANG",
    "LOGNAME",
    "RUSTUP_HOME",
    "SHELL",
    "TMPDIR",
    "USER",
    "XDG_CACHE_HOME",
)
FIXED_EXECUTABLE_PATH = "/usr/bin:/bin:/usr/sbin:/sbin"
FIXED_DEVELOPER_DIRECTORY = pathlib.Path("/Library/Developer/CommandLineTools")
FIXED_RUSTUP = pathlib.Path("/opt/homebrew/opt/rustup/libexec/bin/rustup")
FIXED_XCODE_SELECT = pathlib.Path("/usr/bin/xcode-select")
FIXED_XCRUN = pathlib.Path("/usr/bin/xcrun")
FIXED_GIT = pathlib.Path("/usr/bin/git")
FIXED_RUST_TARGET = "aarch64-apple-darwin"
FIXED_RUST_LINKER_ENVIRONMENT = "CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER"
ISOLATED_BUILD_DIRECTORY_NAMES = (
    "cargo_home",
    "home",
    "xdg_cache_home",
    "xdg_config_home",
    "tmpdir",
)
CODEX_WORKER_SOURCE_FILES = (
    "admission-registry.mjs",
    "codex-runner.mjs",
    "metrics-log.mjs",
    "protocol.mjs",
    "request-timeline.mjs",
    "scheduler.mjs",
    "worker.mjs",
)
D2_TOOLCHAIN_SOURCE_FILES = (
    "d2_certification.py",
    "d2_evidence.py",
    "d2_finalization.py",
    "d2_legacy_substrate_recovery.py",
    "d2_orchestrator_composition.py",
    "d2_orchestrator_contract.py",
    "d2_orchestrator_platform.py",
    "d2_preflight_evidence.py",
    "d2_drained_runtime_restart.py",
    "d2_live_runtime_restart.py",
    "d2_run.py",
    "d2_source_contract.py",
    "d2_worker_evidence.py",
    "isolated_orchestrator.py",
    "product_driver.js",
)
CERTIFICATION_TRANSPORT_SOURCE_FILES = (
    ".gitignore",
    "Cargo.lock",
    "Cargo.toml",
    "src/config.rs",
    "src/control.rs",
    "src/gateway.rs",
    "src/http_proxy.rs",
    "src/lib.rs",
    "src/main.rs",
    "src/state.rs",
)
REPO_CANDIDATE_ARTIFACTS = (
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
WORKSPACE_RELEASE_COMMANDS = (
    (
        "cargo",
        "build",
        "--frozen",
        "--release",
        "--target-dir",
        "{workspace_target}",
        "-p",
        "starring-api",
        "--bin",
        "starring-api",
    ),
    (
        "cargo",
        "build",
        "--frozen",
        "--release",
        "--target-dir",
        "{workspace_target}",
        "-p",
        "starring-runtime",
        "--bin",
        "starring-runtime",
    ),
    (
        "cargo",
        "build",
        "--frozen",
        "--release",
        "--target-dir",
        "{workspace_target}",
        "-p",
        "starring-db-bootstrap",
        "--bin",
        "starring-d2-db-bootstrap",
    ),
    (
        "cargo",
        "build",
        "--frozen",
        "--release",
        "--target-dir",
        "{workspace_target}",
        "-p",
        "starring-staging-provisioner",
        "--bin",
        "starring-d2-sealed-provisioner",
    ),
)
TRANSPORT_RELEASE_COMMAND = (
    "cargo",
    "build",
    "--frozen",
    "--release",
    "--manifest-path",
    "tools/d2-certification-transport/Cargo.toml",
    "--target-dir",
    "{transport_target}",
)


def schema_version_valid(value):
    return type(value) is int and value == SCHEMA_VERSION


def strict_positive_integer(value):
    return type(value) is int and value > 0


def nonce_valid(value):
    return (
        isinstance(value, str)
        and len(value) == 32
        and all(character in "0123456789abcdef" for character in value)
    )


def source_tree_record(root, files, label):
    return {
        "root": str(root),
        "root_identity": directory_identity(root, f"{label}_root", stat.S_IMODE(root.stat().st_mode)),
        "files": list(files),
        "sha256": source_tree_digest(root, files, label),
    }


def exact_source_trees(worktree):
    validate_git_tree_entries(worktree)
    roots = {
        "codex_worker": (worktree / "tools" / "codex-worker", CODEX_WORKER_SOURCE_FILES),
        "d2_toolchain": (
            worktree / "tools" / "d2-certification",
            D2_TOOLCHAIN_SOURCE_FILES,
        ),
        "certification_transport": (
            worktree / "tools" / "d2-certification-transport",
            CERTIFICATION_TRANSPORT_SOURCE_FILES,
        ),
    }
    return {
        name: source_tree_record(root, files, f"candidate_source_{name}")
        for name, (root, files) in roots.items()
    }


def validate_git_tree_entries(worktree):
    try:
        result = subprocess.run(
            [
                str(FIXED_GIT),
                "-C",
                str(worktree),
                "ls-tree",
                "-r",
                "-z",
                "--full-tree",
                "HEAD",
            ],
            cwd=worktree,
            env=sanitized_environment(
                {"DEVELOPER_DIR": str(FIXED_DEVELOPER_DIRECTORY)}
            ),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        fail(f"candidate_source_git_tree_unavailable:{error.__class__.__name__}")
    if result.returncode != 0 or len(result.stdout) > MAX_GIT_TREE_BYTES:
        fail("candidate_source_git_tree_invalid")
    entries = result.stdout.split(b"\x00")
    if not entries or entries[-1] != b"":
        fail("candidate_source_git_tree_invalid")
    observed = set()
    for raw in entries[:-1]:
        try:
            identity, encoded_path = raw.split(b"\t", 1)
            mode, kind, object_id = identity.split(b" ")
            path = encoded_path.decode("utf-8")
        except (ValueError, UnicodeDecodeError):
            fail("candidate_source_git_tree_invalid")
        if (
            mode not in {b"100644", b"100755"}
            or kind != b"blob"
            or len(object_id) != 40
            or any(character not in b"0123456789abcdef" for character in object_id)
            or not path
            or path.startswith("/")
            or ".." in pathlib.PurePosixPath(path).parts
            or path in observed
        ):
            fail("candidate_source_git_entry_forbidden")
        observed.add(path)


def cargo_lockfile_record(manifest, lockfile):
    manifest_identity = file_identity(manifest, "candidate_build_manifest")
    identity = file_identity(lockfile, "candidate_build_lockfile")
    return {
        "manifest_path": str(manifest),
        "manifest_sha256": manifest_identity["sha256"],
        "manifest_size": manifest_identity["size"],
        "lockfile_path": str(lockfile),
        "lockfile_sha256": identity["sha256"],
        "lockfile_size": identity["size"],
    }


def validate_recipe_lockfiles(recipe):
    observed = [
        cargo_lockfile_record(
            pathlib.Path(value["manifest_path"]),
            pathlib.Path(value["lockfile_path"]),
        )
        for value in recipe["lockfiles"]
    ]
    if observed != recipe["lockfiles"]:
        fail("candidate_build_lockfile_drift")


def candidate_build_recipe(state, root, worktree):
    build_root = root / "candidate-build"
    workspace_target = build_root / "workspace-target"
    transport_target = build_root / "transport-target"
    commands = tuple(
        tuple(
            str(workspace_target) if value == "{workspace_target}" else value
            for value in command
        )
        for command in WORKSPACE_RELEASE_COMMANDS
    ) + (
        tuple(
            str(transport_target) if value == "{transport_target}" else value
            for value in TRANSPORT_RELEASE_COMMAND
        ),
    )
    lockfiles = (
        cargo_lockfile_record(worktree / "Cargo.toml", worktree / "Cargo.lock"),
        cargo_lockfile_record(
            worktree / "tools" / "d2-certification-transport" / "Cargo.toml",
            worktree / "tools" / "d2-certification-transport" / "Cargo.lock",
        ),
    )
    isolated_environment = {
        name.upper(): str(build_root / name.replace("_", "-"))
        for name in ISOLATED_BUILD_DIRECTORY_NAMES
    }
    return {
        "schema_version": SCHEMA_VERSION,
        "kind": "starring.d3.candidate-build-recipe.v1",
        "cwd": str(worktree),
        "build_root": str(build_root),
        "commands": [list(command) for command in commands],
        "fetch_commands": [
            [
                "cargo",
                "fetch",
                "--locked",
                "--manifest-path",
                value["manifest_path"],
            ]
            for value in lockfiles
        ],
        "lockfiles": list(lockfiles),
        "isolated_environment": isolated_environment,
        "environment": {
            "AR": "{sealed_tool:ar}",
            "CARGO_INCREMENTAL": "0",
            FIXED_RUST_LINKER_ENVIRONMENT: "{sealed_tool:clang}",
            "CC": "{sealed_tool:clang}",
            "CXX": "{sealed_tool:clang}",
            "DEVELOPER_DIR": str(FIXED_DEVELOPER_DIRECTORY),
            "GIT_TERMINAL_PROMPT": "0",
            "LC_ALL": "C",
            "LD": "{sealed_tool:ld}",
            "PATH": FIXED_EXECUTABLE_PATH,
            "RUSTC": "{sealed_tool:rustc}",
            "STARRING_RUNTIME_BUILD_REVISION": state["merge_commit"],
        },
        "inherited_environment_names": list(SAFE_ENVIRONMENT_NAMES),
        "artifacts": [
            {
                "candidate": candidate,
                "source": str(build_root / source),
                "destination": destination,
            }
            for candidate, source, destination in REPO_CANDIDATE_ARTIFACTS
        ],
        "worker_source_root": str(worktree / "tools" / "codex-worker"),
        "worker_files": list(CODEX_WORKER_SOURCE_FILES),
    }


def build_directory_paths(recipe):
    build_root = absolute_path(recipe["build_root"], "candidate_build_root")
    paths = {"build_root": build_root}
    for name in ISOLATED_BUILD_DIRECTORY_NAMES:
        environment_name = name.upper()
        path = absolute_path(
            recipe["isolated_environment"][environment_name],
            f"candidate_build_{name}",
        )
        if path.parent != build_root:
            fail("candidate_build_directory_path_invalid")
        paths[name] = path
    if len(set(paths.values())) != len(paths):
        fail("candidate_build_directory_path_invalid")
    return paths


def initialize_candidate_build_directories(recipe):
    paths = build_directory_paths(recipe)
    build_root = paths["build_root"]
    try:
        build_root.mkdir(mode=0o700)
        fsync_directory(build_root.parent)
        for name in ISOLATED_BUILD_DIRECTORY_NAMES:
            paths[name].mkdir(mode=0o700)
            fsync_directory(build_root)
    except FileExistsError:
        require_directory(build_root, "candidate_build_root", 0o700)
        try:
            inventory = {path.name for path in build_root.iterdir()}
        except OSError as error:
            fail(f"candidate_build_inventory_unavailable:{error.__class__.__name__}")
        expected_inventory = {
            paths[name].name for name in ISOLATED_BUILD_DIRECTORY_NAMES
        }
        if not inventory.issubset(expected_inventory):
            fail("candidate_build_preintent_inventory_invalid")
        for name in ISOLATED_BUILD_DIRECTORY_NAMES:
            if paths[name].name not in inventory:
                paths[name].mkdir(mode=0o700)
                fsync_directory(build_root)
            require_directory(paths[name], f"candidate_build_{name}", 0o700)
            try:
                if any(paths[name].iterdir()):
                    fail("candidate_build_preintent_inventory_invalid")
            except OSError as error:
                fail(
                    f"candidate_build_preintent_inventory_unavailable:{error.__class__.__name__}"
                )
    return {
        name: directory_identity(path, f"candidate_build_{name}", 0o700)
        for name, path in paths.items()
    }


def validate_build_directories(value, recipe, live):
    expected_paths = build_directory_paths(recipe)
    if not isinstance(value, dict) or set(value) != set(expected_paths):
        fail("candidate_build_directories_invalid")
    for name, expected_path in expected_paths.items():
        identity = value[name]
        if (
            not isinstance(identity, dict)
            or set(identity) != {"path", "mode", "uid", "device", "inode"}
            or identity["path"] != str(expected_path)
            or any(
                type(identity[field]) is not int
                for field in ("mode", "uid", "device", "inode")
            )
            or identity["mode"] != 0o700
            or identity["uid"] != os.getuid()
        ):
            fail("candidate_build_directories_invalid")
        if live:
            verify_directory_identity(
                expected_path, identity, f"candidate_build_{name}"
            )
    return value


def sanitized_environment(extra=None):
    environment = {
        name: os.environ[name]
        for name in SAFE_ENVIRONMENT_NAMES
        if name in os.environ
    }
    environment.update(
        {
            "GIT_TERMINAL_PROMPT": "0",
            "LC_ALL": "C",
            "CARGO_INCREMENTAL": "0",
            "PATH": FIXED_EXECUTABLE_PATH,
        }
    )
    if extra is not None:
        environment.update(extra)
    return environment


def resolve_rustup():
    path = absolute_path(
        os.path.realpath(FIXED_RUSTUP), "candidate_build_rustup"
    )
    file_identity(path, "candidate_build_rustup")
    return path


def resolve_rustup_tool(rustup, name, worktree):
    try:
        result = subprocess.run(
            [str(rustup), "which", name],
            cwd=worktree,
            env=sanitized_environment(),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        fail(f"candidate_build_{name}_selection_unavailable:{error.__class__.__name__}")
    if result.returncode != 0 or not result.stdout or len(result.stdout) > 4096:
        fail(f"candidate_build_{name}_selection_invalid")
    try:
        raw = result.stdout.decode("utf-8").strip()
    except UnicodeDecodeError:
        fail(f"candidate_build_{name}_selection_invalid")
    return absolute_path(os.path.realpath(raw), f"candidate_build_{name}")


def require_system_directory_chain(path):
    current = path
    chain = []
    while True:
        chain.append(current)
        if current.parent == current:
            break
        current = current.parent
    for directory in reversed(chain):
        try:
            metadata = directory.lstat()
        except OSError as error:
            fail(
                f"candidate_build_developer_directory_unavailable:{error.__class__.__name__}"
            )
        if (
            not stat.S_ISDIR(metadata.st_mode)
            or directory.is_symlink()
            or metadata.st_uid != 0
            or stat.S_IMODE(metadata.st_mode) & 0o022
        ):
            fail("candidate_build_developer_directory_identity_invalid")


def resolve_native_toolchain(worktree):
    try:
        selected = subprocess.run(
            [str(FIXED_XCODE_SELECT), "-p"],
            cwd=worktree,
            env=sanitized_environment(),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        fail(
            f"candidate_build_developer_directory_unavailable:{error.__class__.__name__}"
        )
    if (
        selected.returncode != 0
        or selected.stdout.decode("utf-8", errors="replace").strip()
        != str(FIXED_DEVELOPER_DIRECTORY)
    ):
        fail("candidate_build_developer_directory_invalid")
    require_system_directory_chain(FIXED_DEVELOPER_DIRECTORY / "usr" / "bin")
    expected = {
        "clang": FIXED_DEVELOPER_DIRECTORY / "usr" / "bin" / "clang",
        "ar": FIXED_DEVELOPER_DIRECTORY / "usr" / "bin" / "ar",
        "ld": FIXED_DEVELOPER_DIRECTORY / "usr" / "bin" / "ld",
    }
    resolved = {}
    for name, path in expected.items():
        try:
            result = subprocess.run(
                [str(FIXED_XCRUN), "--find", name],
                cwd=worktree,
                env=sanitized_environment(
                    {"DEVELOPER_DIR": str(FIXED_DEVELOPER_DIRECTORY)}
                ),
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=30,
                check=False,
            )
        except (OSError, subprocess.TimeoutExpired) as error:
            fail(f"candidate_build_{name}_unavailable:{error.__class__.__name__}")
        if result.returncode != 0 or len(result.stdout) > 4096:
            fail(f"candidate_build_{name}_selection_invalid")
        try:
            selected_path = pathlib.Path(result.stdout.decode("utf-8").strip())
        except UnicodeDecodeError:
            fail(f"candidate_build_{name}_selection_invalid")
        if selected_path != path:
            fail(f"candidate_build_{name}_selection_invalid")
        canonical = absolute_path(os.path.realpath(path), f"candidate_build_{name}")
        system_file_identity(canonical, f"candidate_build_{name}")
        resolved[name] = canonical
    return resolved


def tool_identity(
    path,
    name,
    worktree,
    version_arguments=("--version", "--verbose"),
    allowed=(0,),
    system_owned=False,
):
    identity = (
        system_file_identity(path, f"candidate_build_{name}")
        if system_owned
        else file_identity(path, f"candidate_build_{name}")
    )
    try:
        result = subprocess.run(
            [str(path), *version_arguments],
            cwd=worktree,
            env=sanitized_environment(),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        fail(f"candidate_build_{name}_version_unavailable:{error.__class__.__name__}")
    output = result.stdout + result.stderr
    if result.returncode not in allowed or not output or len(output) > 65536:
        fail(f"candidate_build_{name}_version_invalid")
    try:
        version = output.decode("utf-8").strip()
    except UnicodeDecodeError:
        fail(f"candidate_build_{name}_version_invalid")
    if not version or "\x00" in version:
        fail(f"candidate_build_{name}_version_invalid")
    return {"name": name, **identity, "version": version}


def rust_target(version):
    hosts = [line[6:] for line in version.splitlines() if line.startswith("host: ")]
    if hosts != [FIXED_RUST_TARGET]:
        fail("candidate_build_rust_target_invalid")
    return hosts[0]


def toolchain_environment(tools, directories):
    by_name = {value["name"]: value for value in tools}
    environment = {
        "AR": by_name["ar"]["path"],
        "CC": by_name["clang"]["path"],
        "CXX": by_name["clang"]["path"],
        "DEVELOPER_DIR": str(FIXED_DEVELOPER_DIRECTORY),
        "LD": by_name["ld"]["path"],
        "RUSTC": by_name["rustc"]["path"],
        FIXED_RUST_LINKER_ENVIRONMENT: by_name["clang"]["path"],
    }
    environment.update(
        {
            name.upper(): directories[name]["path"]
            for name in ISOLATED_BUILD_DIRECTORY_NAMES
        }
    )
    return environment


def validate_candidate_toolchain(value):
    verify_sealed_record(value, "candidate_build_toolchain")
    required = {
        "schema_version",
        "kind",
        "rust_target",
        "developer_directory",
        "directories",
        "environment",
        "tools",
        "record_sha256",
    }
    if (
        not isinstance(value, dict)
        or set(value) != required
        or not schema_version_valid(value["schema_version"])
        or value["kind"] != "starring.d3.candidate-build-toolchain.v1"
        or value["rust_target"] != FIXED_RUST_TARGET
        or value["developer_directory"] != str(FIXED_DEVELOPER_DIRECTORY)
    ):
        fail("candidate_build_toolchain_invalid")
    tools = value["tools"]
    names = ["rustup", "cargo", "rustc", "clang", "ar", "ld"]
    if (
        not isinstance(tools, list)
        or [item.get("name") for item in tools if isinstance(item, dict)] != names
    ):
        fail("candidate_build_toolchain_invalid")
    by_name = {}
    for item in tools:
        if not isinstance(item, dict) or set(item) != {
            "name",
            "path",
            "sha256",
            "size",
            "mode",
            "uid",
            "device",
            "inode",
            "links",
            "version",
        }:
            fail("candidate_build_toolchain_invalid")
        identity = dict(item)
        name = identity.pop("name")
        version = identity.pop("version")
        if not isinstance(version, str) or not version or "\x00" in version:
            fail("candidate_build_toolchain_invalid")
        validate_file_identity_shape(identity, "candidate_build_toolchain")
        by_name[name] = item
    native_paths = {
        "clang": str(FIXED_DEVELOPER_DIRECTORY / "usr" / "bin" / "clang"),
        "ar": str(FIXED_DEVELOPER_DIRECTORY / "usr" / "bin" / "ar"),
        "ld": str(FIXED_DEVELOPER_DIRECTORY / "usr" / "bin" / "ld"),
    }
    if any(
        by_name[name]["path"] != path
        or by_name[name]["uid"] != 0
        or by_name[name]["mode"] & 0o022
        for name, path in native_paths.items()
    ) or any(
        by_name[name]["uid"] != os.getuid() or by_name[name]["mode"] & 0o022
        for name in ("rustup", "cargo", "rustc")
    ):
        fail("candidate_build_toolchain_invalid")
    rust_target(by_name["rustc"]["version"])
    directories = value["directories"]
    if not isinstance(directories, dict):
        fail("candidate_build_toolchain_invalid")
    environment_paths = {
        name: item.get("path") if isinstance(item, dict) else None
        for name, item in directories.items()
    }
    if set(environment_paths) != {"build_root", *ISOLATED_BUILD_DIRECTORY_NAMES}:
        fail("candidate_build_toolchain_invalid")
    for item in directories.values():
        if (
            not isinstance(item, dict)
            or set(item) != {"path", "mode", "uid", "device", "inode"}
            or not isinstance(item["path"], str)
            or any(
                type(item[field]) is not int
                for field in ("mode", "uid", "device", "inode")
            )
            or item["mode"] != 0o700
            or item["uid"] != os.getuid()
        ):
            fail("candidate_build_toolchain_invalid")
    if value["environment"] != toolchain_environment(tools, directories):
        fail("candidate_build_toolchain_environment_invalid")
    return value


def resolve_candidate_toolchain(worktree, directories):
    rustup = resolve_rustup()
    cargo = resolve_rustup_tool(rustup, "cargo", worktree)
    rustc = resolve_rustup_tool(rustup, "rustc", worktree)
    native = resolve_native_toolchain(worktree)
    tools = [
        tool_identity(rustup, "rustup", worktree, ("--version",)),
        tool_identity(cargo, "cargo", worktree),
        tool_identity(rustc, "rustc", worktree),
        tool_identity(
            native["clang"],
            "clang",
            worktree,
            ("--version",),
            system_owned=True,
        ),
        tool_identity(
            native["ar"],
            "ar",
            worktree,
            (),
            allowed=(1,),
            system_owned=True,
        ),
        tool_identity(
            native["ld"],
            "ld",
            worktree,
            ("-v",),
            system_owned=True,
        ),
    ]
    value = seal_record(
        {
            "schema_version": SCHEMA_VERSION,
            "kind": "starring.d3.candidate-build-toolchain.v1",
            "rust_target": rust_target(tools[2]["version"]),
            "developer_directory": str(FIXED_DEVELOPER_DIRECTORY),
            "directories": directories,
            "environment": toolchain_environment(tools, directories),
            "tools": tools,
        }
    )
    return validate_candidate_toolchain(value)


def require_cargo_configuration_absent(worktree):
    cargo_home = pathlib.Path(
        os.environ.get("CARGO_HOME", str(pathlib.Path.home() / ".cargo"))
    ).expanduser()
    if not cargo_home.is_absolute():
        fail("candidate_build_cargo_home_invalid")
    roots = [cargo_home]
    roots.extend(directory / ".cargo" for directory in (worktree, *worktree.parents))
    observed_roots = set()
    for root in roots:
        normalized = os.path.normpath(str(root))
        if normalized in observed_roots:
            continue
        observed_roots.add(normalized)
        for name in ("config", "config.toml"):
            try:
                (root / name).lstat()
            except FileNotFoundError:
                continue
            except OSError as error:
                fail(f"candidate_build_cargo_config_unavailable:{error.__class__.__name__}")
            fail("candidate_build_cargo_config_forbidden")


def run_build_command(command, cargo, build_environment, worktree, revision):
    try:
        result = subprocess.run(
            [str(cargo), *command[1:]],
            cwd=worktree,
            env=sanitized_environment(
                {
                    **build_environment,
                    "CARGO_NET_OFFLINE": "true",
                    "STARRING_RUNTIME_BUILD_REVISION": revision,
                }
            ),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=7200,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        fail(f"candidate_build_unavailable:{error.__class__.__name__}")
    if result.returncode != 0:
        fail(f"candidate_build_failed:{result.returncode}")


def run_fetch_command(command, cargo, build_environment, worktree, revision):
    try:
        result = subprocess.run(
            [str(cargo), *command[1:]],
            cwd=worktree,
            env=sanitized_environment(
                {
                    **build_environment,
                    "STARRING_RUNTIME_BUILD_REVISION": revision,
                }
            ),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=7200,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        fail(f"candidate_fetch_unavailable:{error.__class__.__name__}")
    if result.returncode != 0:
        fail(f"candidate_fetch_failed:{result.returncode}")


def require_isolated_cargo_configuration_absent(toolchain):
    cargo_home = pathlib.Path(toolchain["environment"]["CARGO_HOME"])
    home = pathlib.Path(toolchain["environment"]["HOME"])
    forbidden = (
        cargo_home / "config",
        cargo_home / "config.toml",
        cargo_home / "credentials",
        cargo_home / "credentials.toml",
        home / ".cargo" / "config",
        home / ".cargo" / "config.toml",
        home / ".cargo" / "credentials",
        home / ".cargo" / "credentials.toml",
    )
    for path in forbidden:
        try:
            path.lstat()
        except FileNotFoundError:
            continue
        except OSError as error:
            fail(
                f"candidate_build_isolated_config_unavailable:{error.__class__.__name__}"
            )
        fail("candidate_build_isolated_config_forbidden")


def execute_candidate_build(state, worktree, root, recipe, toolchain):
    require_cargo_configuration_absent(worktree)
    validate_candidate_toolchain(toolchain)
    validate_build_directories(toolchain["directories"], recipe, True)
    if resolve_candidate_toolchain(worktree, toolchain["directories"]) != toolchain:
        fail("candidate_build_toolchain_drift")
    build_root = absolute_path(recipe["build_root"], "candidate_build_root")
    if build_root.parent != root:
        fail("candidate_build_root_invalid")
    require_directory(build_root, "candidate_build_root", 0o700)
    tools = toolchain["tools"]
    cargo = pathlib.Path(tools[1]["path"])
    build_environment = toolchain["environment"]
    require_isolated_cargo_configuration_absent(toolchain)
    validate_recipe_lockfiles(recipe)
    for command in recipe["fetch_commands"]:
        run_fetch_command(
            command,
            cargo,
            build_environment,
            worktree,
            state["merge_commit"],
        )
    require_isolated_cargo_configuration_absent(toolchain)
    validate_recipe_lockfiles(recipe)
    for command in recipe["commands"]:
        run_build_command(
            command,
            cargo,
            build_environment,
            worktree,
            state["merge_commit"],
        )
    require_isolated_cargo_configuration_absent(toolchain)
    validate_recipe_lockfiles(recipe)
    if resolve_candidate_toolchain(worktree, toolchain["directories"]) != toolchain:
        fail("candidate_build_toolchain_changed")
    return tools


def candidate_bundle_intent(
    state, gate_chain, recipe, source_trees, toolchain, nonce
):
    return seal_record(
        {
            "schema_version": SCHEMA_VERSION,
            "kind": "starring.d3.candidate-bundle-intent.v1",
            "merge_commit": state["merge_commit"],
            "merge_tree": state["merge_tree"],
            "gate_evidence_chain_head_sha256": gate_chain,
            "recipe": recipe,
            "source_trees": source_trees,
            "toolchain": toolchain,
            "staging_nonce": nonce,
            "staging_path": str(recipe_root(recipe) / f".candidate-bundle-staging-{nonce}"),
        }
    )


def recipe_root(recipe):
    return pathlib.Path(recipe["build_root"]).parent


def validate_intent(value, state, gate_chain, recipe, source_trees):
    verify_sealed_record(value, "candidate_bundle_intent")
    nonce = value.get("staging_nonce") if isinstance(value, dict) else None
    toolchain = value.get("toolchain") if isinstance(value, dict) else None
    validate_candidate_toolchain(toolchain)
    validate_build_directories(toolchain["directories"], recipe, False)
    if (
        not schema_version_valid(value.get("schema_version"))
        or not nonce_valid(nonce)
        or canonical_json(value)
        != canonical_json(
            candidate_bundle_intent(
                state, gate_chain, recipe, source_trees, toolchain, nonce
            )
        )
    ):
        fail("candidate_bundle_intent_mismatch")
    return nonce


def intent_temporary_paths(root):
    prefix = ".candidate-bundle-intent.json.tmp-"
    try:
        return tuple(
            path
            for path in root.iterdir()
            if path.name.startswith(prefix)
        )
    except OSError as error:
        fail(f"candidate_bundle_intent_temporary_unavailable:{error.__class__.__name__}")


def recover_intent_temporary(root, state, gate_chain, recipe, source_trees):
    intent_path = root / "candidate-bundle-intent.json"
    temporaries = intent_temporary_paths(root)
    if len(temporaries) > 1:
        fail("candidate_bundle_intent_temporary_ambiguous")
    if not temporaries:
        return
    temporary = temporaries[0]
    nonce = temporary.name.rsplit("-", 1)[-1]
    if not nonce_valid(nonce):
        fail("candidate_bundle_intent_temporary_name_invalid")
    try:
        value = read_json(temporary, "candidate_bundle_intent_temporary", 0o600)
        validate_intent(value, state, gate_chain, recipe, source_trees)
        valid = value["staging_nonce"] == nonce
    except CandidateBundleError:
        valid = False
    if intent_path.exists():
        existing = read_json(intent_path, "candidate_bundle_intent", 0o600)
        validate_intent(existing, state, gate_chain, recipe, source_trees)
        if not valid or existing != value:
            fail("candidate_bundle_intent_temporary_drift")
        unlink_owned_regular(temporary, 0o600)
        return
    if not valid:
        unlink_owned_regular(temporary, 0o600)
        return
    parent = os.open(root, os.O_RDONLY)
    try:
        rename_exclusive(parent, temporary.name, intent_path.name)
        os.fsync(parent)
    finally:
        os.close(parent)


def publication_identity(intent, staging, final_root):
    metadata = require_directory(staging, "candidate_bundle_staging", 0o700)
    return seal_record(
        {
            "schema_version": SCHEMA_VERSION,
            "kind": "starring.d3.candidate-bundle-publication.v1",
            "intent_record_sha256": intent["record_sha256"],
            "staging_nonce": intent["staging_nonce"],
            "staging_path": str(staging),
            "final_path": str(final_root),
            "uid": metadata.st_uid,
            "device": metadata.st_dev,
            "inode": metadata.st_ino,
            "creation_mode": 0o700,
            "sealed_mode": 0o555,
        }
    )


def validate_publication_identity(path, intent, directory, final_root, mode):
    value = read_json(path, "candidate_bundle_publication", 0o400)
    verify_sealed_record(value, "candidate_bundle_publication")
    required = {
        "schema_version",
        "kind",
        "intent_record_sha256",
        "staging_nonce",
        "staging_path",
        "final_path",
        "uid",
        "device",
        "inode",
        "creation_mode",
        "sealed_mode",
        "record_sha256",
    }
    metadata = require_directory(directory, "candidate_bundle_publication_root", mode)
    if (
        set(value) != required
        or not schema_version_valid(value["schema_version"])
        or value["kind"] != "starring.d3.candidate-bundle-publication.v1"
        or value["intent_record_sha256"] != intent["record_sha256"]
        or value["staging_nonce"] != intent["staging_nonce"]
        or value["staging_path"] != intent["staging_path"]
        or value["final_path"] != str(final_root)
        or value["uid"] != metadata.st_uid
        or value["device"] != metadata.st_dev
        or value["inode"] != metadata.st_ino
        or value["creation_mode"] != 0o700
        or value["sealed_mode"] != 0o555
    ):
        fail("candidate_bundle_publication_identity_mismatch")
    return value


def publication_temporary_path(directory, intent):
    return directory / f".publication.json.tmp-{intent['staging_nonce']}"


def recover_publication_temporary(directory, intent, final_root):
    publication = directory / "publication.json"
    temporary = publication_temporary_path(directory, intent)
    if not temporary.exists():
        return
    try:
        value = validate_publication_identity(
            temporary, intent, directory, final_root, 0o700
        )
        valid = value["staging_nonce"] == intent["staging_nonce"]
    except CandidateBundleError:
        valid = False
    if publication.exists():
        existing = validate_publication_identity(
            publication, intent, directory, final_root, 0o700
        )
        if not valid or existing != value:
            fail("candidate_bundle_publication_temporary_drift")
        unlink_owned_regular(temporary, 0o400)
        return
    if not valid:
        unlink_owned_regular(temporary, 0o400)
        return
    parent = os.open(directory, os.O_RDONLY)
    try:
        rename_exclusive(parent, temporary.name, publication.name)
        os.fsync(parent)
    finally:
        os.close(parent)


def staging_paths(root):
    try:
        return tuple(
            path
            for path in root.iterdir()
            if path.name.startswith(".candidate-bundle-staging-")
        )
    except OSError as error:
        fail(f"candidate_bundle_staging_unavailable:{error.__class__.__name__}")


def discard_journal_value(root, staging, intent):
    metadata = require_directory(staging, "candidate_bundle_staging", 0o700)
    return seal_record(
        {
            "schema_version": SCHEMA_VERSION,
            "kind": "starring.d3.candidate-bundle-discard.v1",
            "intent_record_sha256": intent["record_sha256"],
            "staging_nonce": intent["staging_nonce"],
            "staging_path": str(staging),
            "discard_path": str(
                root / f".candidate-bundle-discard-{intent['staging_nonce']}"
            ),
            "uid": metadata.st_uid,
            "device": metadata.st_dev,
            "inode": metadata.st_ino,
            "mode": 0o700,
        }
    )


def validate_discard_journal(value, root, intent):
    verify_sealed_record(value, "candidate_bundle_discard")
    required = {
        "schema_version",
        "kind",
        "intent_record_sha256",
        "staging_nonce",
        "staging_path",
        "discard_path",
        "uid",
        "device",
        "inode",
        "mode",
        "record_sha256",
    }
    expected_staging = absolute_path(intent["staging_path"], "candidate_bundle_staging")
    expected_discard = root / f".candidate-bundle-discard-{intent['staging_nonce']}"
    if (
        set(value) != required
        or not schema_version_valid(value["schema_version"])
        or value["kind"] != "starring.d3.candidate-bundle-discard.v1"
        or value["intent_record_sha256"] != intent["record_sha256"]
        or value["staging_nonce"] != intent["staging_nonce"]
        or value["staging_path"] != str(expected_staging)
        or value["discard_path"] != str(expected_discard)
        or any(type(value[name]) is not int for name in ("uid", "device", "inode", "mode"))
        or value["mode"] != 0o700
    ):
        fail("candidate_bundle_discard_journal_invalid")
    return value


def discard_journal_temporary(root, intent):
    return root / f".candidate-bundle-discard.json.tmp-{intent['staging_nonce']}"


def recover_discard_journal_temporary(root, intent):
    journal = root / "candidate-bundle-discard.json"
    temporary = discard_journal_temporary(root, intent)
    if not temporary.exists():
        return
    try:
        value = read_json(temporary, "candidate_bundle_discard_temporary", 0o600)
        validate_discard_journal(value, root, intent)
        valid = True
    except CandidateBundleError:
        valid = False
    if journal.exists():
        existing = read_json(journal, "candidate_bundle_discard", 0o600)
        validate_discard_journal(existing, root, intent)
        if not valid or existing != value:
            fail("candidate_bundle_discard_temporary_drift")
        unlink_owned_regular(temporary, 0o600)
        return
    if not valid:
        unlink_owned_regular(temporary, 0o600)
        return
    parent = os.open(root, os.O_RDONLY)
    try:
        rename_exclusive(parent, temporary.name, journal.name)
        os.fsync(parent)
    finally:
        os.close(parent)


def finish_discard(root, discard, journal_path, journal):
    flags = os.O_RDONLY
    if hasattr(os, "O_DIRECTORY"):
        flags |= os.O_DIRECTORY
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    descriptor = os.open(discard, flags)
    try:
        observed = os.fstat(descriptor)
        if (
            observed.st_dev,
            observed.st_ino,
            observed.st_uid,
            stat.S_IMODE(observed.st_mode),
        ) != (
            journal["device"],
            journal["inode"],
            journal["uid"],
            journal["mode"],
        ):
            fail("candidate_bundle_discard_identity_drift")
        remove_tree_descriptor(descriptor)
    finally:
        os.close(descriptor)
    os.rmdir(discard)
    fsync_directory(root)
    unlink_owned_regular(journal_path, 0o600)


def recover_discard_residue(root, intent):
    recover_discard_journal_temporary(root, intent)
    journal_path = root / "candidate-bundle-discard.json"
    discard = root / f".candidate-bundle-discard-{intent['staging_nonce']}"
    if not journal_path.exists():
        if discard.exists():
            fail("candidate_bundle_discard_unjournaled")
        return
    journal = read_json(journal_path, "candidate_bundle_discard", 0o600)
    validate_discard_journal(journal, root, intent)
    if discard.exists():
        finish_discard(root, discard, journal_path, journal)
        return
    staging = absolute_path(intent["staging_path"], "candidate_bundle_staging")
    if staging.exists():
        metadata = require_directory(staging, "candidate_bundle_staging", 0o700)
        if (
            metadata.st_dev,
            metadata.st_ino,
            metadata.st_uid,
        ) != (journal["device"], journal["inode"], journal["uid"]):
            fail("candidate_bundle_discard_identity_drift")
    unlink_owned_regular(journal_path, 0o600)


def discard_staging(root, staging, intent):
    require_directory(staging, "candidate_bundle_staging", 0o700)
    validate_publication_identity(
        staging / "publication.json", intent, staging, root / "candidate-bundle", 0o700
    )
    recover_discard_residue(root, intent)
    journal_path = root / "candidate-bundle-discard.json"
    journal = discard_journal_value(root, staging, intent)
    write_new_file_atomic(
        journal_path,
        discard_journal_temporary(root, intent).name,
        (canonical_json(journal) + "\n").encode("utf-8"),
        0o600,
    )
    discard_name = f".candidate-bundle-discard-{intent['staging_nonce']}"
    parent = os.open(root, os.O_RDONLY)
    try:
        rename_exclusive(parent, staging.name, discard_name)
        os.fsync(parent)
    finally:
        os.close(parent)
    discard = root / discard_name
    finish_discard(root, discard, journal_path, journal)


def verify_staging_bundle(
    root, staging, state, worktree, gate_chain, recipe, source_trees, intent
):
    final_root = root / "candidate-bundle"
    validate_publication_identity(
        staging / "publication.json", intent, staging, final_root, 0o555
    )
    try:
        inventory = {path.name for path in staging.iterdir()}
    except OSError as error:
        fail(f"candidate_bundle_staging_inventory_unavailable:{error.__class__.__name__}")
    if inventory != expected_root_inventory():
        fail("candidate_bundle_staging_inventory_invalid")
    record = read_json(staging / "bundle.json", "candidate_bundle_staging_record", 0o400)
    verify_sealed_record(record, "candidate_bundle_staging_record")
    if (
        record.get("merge_commit") != state["merge_commit"]
        or record.get("merge_tree") != state["merge_tree"]
        or record.get("gate_evidence_chain_head_sha256") != gate_chain
        or record.get("recipe") != recipe
        or record.get("source_trees") != source_trees
    ):
        fail("candidate_bundle_staging_record_mismatch")
    bundle_identity = record.get("bundle_root")
    if not isinstance(bundle_identity, dict) or bundle_identity.get("creation_mode") != 0o700:
        fail("candidate_bundle_staging_root_invalid")
    observed_root = directory_identity(staging, "candidate_bundle_staging", 0o555)
    observed_root["path"] = str(final_root)
    expected_root = dict(bundle_identity)
    expected_root.pop("creation_mode")
    if observed_root != expected_root:
        fail("candidate_bundle_staging_root_mismatch")
    artifacts = record.get("artifacts")
    if not isinstance(artifacts, list):
        fail("candidate_bundle_staging_artifacts_invalid")
    by_candidate = {
        value.get("candidate"): value for value in artifacts if isinstance(value, dict)
    }
    for candidate, _source, destination in REPO_CANDIDATE_ARTIFACTS:
        value = by_candidate.get(candidate)
        if not isinstance(value, dict) or set(value) != {"candidate", "source", "artifact"}:
            fail("candidate_bundle_staging_artifacts_invalid")
        if normalized_file_identity(
            staging / destination,
            final_root / destination,
            f"candidate_bundle_staging_{candidate}",
            0o555,
        ) != value["artifact"]:
            fail("candidate_bundle_staging_artifact_mismatch")
    worker = record.get("worker")
    worker_staging = staging / "codex-worker"
    if not isinstance(worker, dict):
        fail("candidate_bundle_staging_worker_invalid")
    observed_worker = directory_identity(
        worker_staging, "candidate_bundle_staging_worker", 0o555
    )
    observed_worker["path"] = str(final_root / "codex-worker")
    if observed_worker != worker.get("root_identity"):
        fail("candidate_bundle_staging_worker_mismatch")
    files = worker.get("files")
    if not isinstance(files, list) or len(files) != len(CODEX_WORKER_SOURCE_FILES):
        fail("candidate_bundle_staging_worker_invalid")
    by_name = {value.get("name"): value for value in files if isinstance(value, dict)}
    if set(by_name) != set(CODEX_WORKER_SOURCE_FILES):
        fail("candidate_bundle_staging_worker_invalid")
    for name in CODEX_WORKER_SOURCE_FILES:
        if normalized_file_identity(
            worker_staging / name,
            final_root / "codex-worker" / name,
            f"candidate_bundle_staging_worker_{name}",
            0o444,
        ) != by_name[name]["artifact"]:
            fail("candidate_bundle_staging_worker_mismatch")
    if source_tree_digest(
        worker_staging, CODEX_WORKER_SOURCE_FILES, "candidate_bundle_staging_worker"
    ) != worker.get("sha256"):
        fail("candidate_bundle_staging_worker_mismatch")
    return record


def recover_candidate_staging(
    root, state, worktree, gate_chain, recipe, source_trees, intent
):
    recover_discard_residue(root, intent)
    observed = staging_paths(root)
    expected = absolute_path(intent["staging_path"], "candidate_bundle_staging")
    foreign = tuple(path for path in observed if path != expected)
    if foreign:
        fail("candidate_bundle_foreign_staging_present")
    if not expected.exists():
        return
    metadata = require_directory(expected, "candidate_bundle_staging")
    publication = expected / "publication.json"
    if stat.S_IMODE(metadata.st_mode) == 0o700:
        recover_publication_temporary(
            expected, intent, root / "candidate-bundle"
        )
    if not publication.exists():
        try:
            empty = not any(expected.iterdir())
        except OSError as error:
            fail(f"candidate_bundle_staging_unavailable:{error.__class__.__name__}")
        if not empty or stat.S_IMODE(metadata.st_mode) != 0o700:
            fail("candidate_bundle_staging_identity_absent")
        expected.rmdir()
        fsync_directory(root)
        return
    mode = stat.S_IMODE(metadata.st_mode)
    if mode == 0o700:
        discard_staging(root, expected, intent)
        return
    if mode != 0o555:
        fail("candidate_bundle_staging_mode_invalid")
    verify_staging_bundle(
        root, expected, state, worktree, gate_chain, recipe, source_trees, intent
    )
    parent = os.open(root, os.O_RDONLY)
    try:
        rename_exclusive(parent, expected.name, "candidate-bundle")
        os.fsync(parent)
    finally:
        os.close(parent)


def create_candidate_bundle(
    state, root, worktree, gate_chain, recipe, tools, source_trees, intent
):
    final_root = root / "candidate-bundle"
    staging = absolute_path(intent["staging_path"], "candidate_bundle_staging")
    staging.mkdir(mode=0o700)
    fsync_directory(root)
    write_new_file_atomic(
        staging / "publication.json",
        publication_temporary_path(staging, intent).name,
        (canonical_json(publication_identity(intent, staging, final_root)) + "\n").encode(
            "utf-8"
        ),
        0o400,
    )
    worker_staging = staging / "codex-worker"
    worker_staging.mkdir(mode=0o700)
    fsync_directory(staging)
    artifacts = []
    for specification in recipe["artifacts"]:
        candidate = specification["candidate"]
        source = absolute_path(specification["source"], f"candidate_source_{candidate}")
        destination = staging / specification["destination"]
        source_identity, artifact_identity = copy_snapshot(
            source, destination, 0o555, f"candidate_{candidate}"
        )
        artifact_identity["path"] = str(final_root / specification["destination"])
        artifacts.append(
            {
                "candidate": candidate,
                "source": source_identity,
                "artifact": artifact_identity,
            }
        )
    fsync_directory(staging)
    worker_files = []
    worker_source_root = absolute_path(
        recipe["worker_source_root"], "candidate_worker_source_root"
    )
    for name in CODEX_WORKER_SOURCE_FILES:
        source_identity, artifact_identity = copy_snapshot(
            worker_source_root / name,
            worker_staging / name,
            0o444,
            f"candidate_worker_{name}",
        )
        artifact_identity["path"] = str(final_root / "codex-worker" / name)
        worker_files.append(
            {"name": name, "source": source_identity, "artifact": artifact_identity}
        )
    fsync_directory(worker_staging)
    worker_digest = source_tree_digest(
        worker_staging, CODEX_WORKER_SOURCE_FILES, "candidate_worker_bundle"
    )
    worker_staging.chmod(0o555)
    worker_directory = directory_identity(
        worker_staging, "candidate_bundle_worker", 0o555
    )
    worker_directory["path"] = str(final_root / "codex-worker")
    root_before_seal = directory_identity(staging, "candidate_bundle_staging", 0o700)
    state_root = directory_identity(root, "candidate_bundle_state_root", 0o700)
    record = seal_record(
        {
            "schema_version": SCHEMA_VERSION,
            "kind": "starring.d3.sealed-candidate-bundle.v1",
            "merge_commit": state["merge_commit"],
            "merge_tree": state["merge_tree"],
            "gate_evidence_chain_head_sha256": gate_chain,
            "recipe": recipe,
            "tools": tools,
            "state_root": state_root,
            "bundle_root": {
                **root_before_seal,
                "path": str(final_root),
                "creation_mode": root_before_seal["mode"],
                "mode": 0o555,
            },
            "source_trees": source_trees,
            "artifacts": artifacts,
            "worker": {
                "root": str(final_root / "codex-worker"),
                "root_identity": worker_directory,
                "sha256": worker_digest,
                "files": worker_files,
            },
        }
    )
    record_path = staging / "bundle.json"
    write_new_file(
        record_path, (canonical_json(record) + "\n").encode("utf-8"), 0o400
    )
    staging.chmod(0o555)
    fsync_directory(worker_staging)
    fsync_directory(staging)
    parent = os.open(root, os.O_RDONLY)
    try:
        rename_exclusive(parent, staging.name, final_root.name)
        os.fsync(parent)
    finally:
        os.close(parent)
    return record


def expected_root_inventory():
    return {"bundle.json", "publication.json", "codex-worker"} | {
        destination for _candidate, _source, destination in REPO_CANDIDATE_ARTIFACTS
    }


def validate_file_identity_shape(value, label):
    if not isinstance(value, dict) or set(value) != {
        "path",
        "sha256",
        "size",
        "mode",
        "uid",
        "device",
        "inode",
        "links",
    }:
        fail(f"{label}_fields_invalid")
    if (
        not isinstance(value["path"], str)
        or not value["path"]
        or len(value["path"].encode("utf-8")) > 4096
        or "\x00" in value["path"]
        or not isinstance(value["sha256"], str)
        or len(value["sha256"]) != 64
        or any(character not in "0123456789abcdef" for character in value["sha256"])
        or not strict_positive_integer(value["size"])
        or type(value["mode"]) is not int
        or type(value["uid"]) is not int
        or type(value["device"]) is not int
        or type(value["inode"]) is not int
        or type(value["links"]) is not int
        or value["links"] != 1
    ):
        fail(f"{label}_identity_invalid")
    absolute_path(value["path"], label)


def validate_source_tree_record(value, expected, label):
    if not isinstance(value, dict) or set(value) != {
        "root",
        "root_identity",
        "files",
        "sha256",
    }:
        fail(f"{label}_fields_invalid")
    if value != expected:
        fail(f"{label}_identity_mismatch")


def load_candidate_bundle(root, state, gate_chain):
    worktree = pathlib.Path(state["worktree_path"])
    recipe = candidate_build_recipe(state, root, worktree)
    source_trees = exact_source_trees(worktree)
    intent = read_json(root / "candidate-bundle-intent.json", "candidate_bundle_intent", 0o600)
    validate_intent(intent, state, gate_chain, recipe, source_trees)
    if staging_paths(root):
        fail("candidate_bundle_staging_incomplete")
    bundle_root = root / "candidate-bundle"
    require_directory(bundle_root, "candidate_bundle_root", 0o555)
    try:
        inventory = {path.name for path in bundle_root.iterdir()}
    except OSError as error:
        fail(f"candidate_bundle_inventory_unavailable:{error.__class__.__name__}")
    if inventory != expected_root_inventory():
        fail("candidate_bundle_inventory_invalid")
    validate_publication_identity(
        bundle_root / "publication.json",
        intent,
        bundle_root,
        bundle_root,
        0o555,
    )
    record_path = bundle_root / "bundle.json"
    record = read_json(record_path, "candidate_bundle", 0o400)
    verify_sealed_record(record, "candidate_bundle")
    required = {
        "schema_version",
        "kind",
        "merge_commit",
        "merge_tree",
        "gate_evidence_chain_head_sha256",
        "recipe",
        "tools",
        "state_root",
        "bundle_root",
        "source_trees",
        "artifacts",
        "worker",
        "record_sha256",
    }
    if (
        set(record) != required
        or not schema_version_valid(record["schema_version"])
        or record["kind"] != "starring.d3.sealed-candidate-bundle.v1"
        or record["merge_commit"] != state["merge_commit"]
        or record["merge_tree"] != state["merge_tree"]
        or record["gate_evidence_chain_head_sha256"] != gate_chain
        or canonical_json(record["recipe"]) != canonical_json(recipe)
    ):
        fail("candidate_bundle_identity_invalid")
    verify_directory_identity(root, record["state_root"], "candidate_bundle_state_root")
    bundle_identity = record["bundle_root"]
    if not isinstance(bundle_identity, dict) or bundle_identity.get("creation_mode") != 0o700:
        fail("candidate_bundle_root_fields_invalid")
    sealed_identity = dict(bundle_identity)
    sealed_identity.pop("creation_mode")
    verify_directory_identity(bundle_root, sealed_identity, "candidate_bundle_root")
    if record["source_trees"] != source_trees:
        fail("candidate_bundle_source_trees_mismatch")
    tools = record["tools"]
    if not isinstance(tools, list) or [
        value.get("name") for value in tools if isinstance(value, dict)
    ] != ["rustup", "cargo", "rustc", "clang", "ar", "ld"]:
        fail("candidate_bundle_tools_invalid")
    if tools != intent["toolchain"]["tools"]:
        fail("candidate_bundle_toolchain_mismatch")
    for value in tools:
        if not isinstance(value, dict) or "version" not in value or "name" not in value:
            fail("candidate_bundle_tool_fields_invalid")
        identity = dict(value)
        identity.pop("name")
        version = identity.pop("version")
        if not isinstance(version, str) or not version:
            fail("candidate_bundle_tool_identity_invalid")
        validate_file_identity_shape(identity, "candidate_bundle_tool")
        absolute_path(identity["path"], "candidate_bundle_tool")
    artifacts = record["artifacts"]
    if not isinstance(artifacts, list) or len(artifacts) != len(REPO_CANDIDATE_ARTIFACTS):
        fail("candidate_bundle_artifacts_invalid")
    by_candidate = {
        value.get("candidate"): value for value in artifacts if isinstance(value, dict)
    }
    if set(by_candidate) != {value[0] for value in REPO_CANDIDATE_ARTIFACTS}:
        fail("candidate_bundle_artifacts_invalid")
    for candidate, source_relative, destination in REPO_CANDIDATE_ARTIFACTS:
        value = by_candidate[candidate]
        if set(value) != {"candidate", "source", "artifact"}:
            fail("candidate_bundle_artifact_fields_invalid")
        validate_file_identity_shape(value["source"], "candidate_bundle_source")
        validate_file_identity_shape(value["artifact"], "candidate_bundle_artifact")
        if value["source"]["path"] != str(root / "candidate-build" / source_relative):
            fail("candidate_bundle_source_path_mismatch")
        artifact_path = bundle_root / destination
        if value["artifact"]["path"] != str(artifact_path):
            fail("candidate_bundle_artifact_path_mismatch")
        file_identity(
            artifact_path,
            f"candidate_bundle_artifact_{candidate}",
            expected=value["artifact"],
            expected_mode=0o555,
        )
        if value["source"]["sha256"] != value["artifact"]["sha256"]:
            fail("candidate_bundle_artifact_digest_mismatch")
    worker = record["worker"]
    if not isinstance(worker, dict) or set(worker) != {
        "root",
        "root_identity",
        "sha256",
        "files",
    }:
        fail("candidate_bundle_worker_fields_invalid")
    worker_root = bundle_root / "codex-worker"
    if worker["root"] != str(worker_root):
        fail("candidate_bundle_worker_root_mismatch")
    verify_directory_identity(
        worker_root, worker["root_identity"], "candidate_bundle_worker_root"
    )
    try:
        inventory = {path.name for path in worker_root.iterdir()}
    except OSError as error:
        fail(f"candidate_bundle_worker_inventory_unavailable:{error.__class__.__name__}")
    if inventory != set(CODEX_WORKER_SOURCE_FILES):
        fail("candidate_bundle_worker_inventory_invalid")
    files = worker["files"]
    if not isinstance(files, list) or [value.get("name") for value in files if isinstance(value, dict)] != list(CODEX_WORKER_SOURCE_FILES):
        fail("candidate_bundle_worker_files_invalid")
    for value in files:
        if set(value) != {"name", "source", "artifact"}:
            fail("candidate_bundle_worker_file_fields_invalid")
        name = value["name"]
        validate_file_identity_shape(value["source"], "candidate_bundle_worker_source")
        validate_file_identity_shape(value["artifact"], "candidate_bundle_worker_artifact")
        if value["source"]["path"] != str(worktree / "tools" / "codex-worker" / name):
            fail("candidate_bundle_worker_source_path_mismatch")
        artifact_path = worker_root / name
        if value["artifact"]["path"] != str(artifact_path):
            fail("candidate_bundle_worker_artifact_path_mismatch")
        file_identity(
            artifact_path,
            f"candidate_bundle_worker_{name}",
            expected=value["artifact"],
            expected_mode=0o444,
        )
        if value["source"]["sha256"] != value["artifact"]["sha256"]:
            fail("candidate_bundle_worker_digest_mismatch")
    if source_tree_digest(
        worker_root, CODEX_WORKER_SOURCE_FILES, "candidate_bundle_worker"
    ) != worker["sha256"] or worker["sha256"] != source_trees["codex_worker"]["sha256"]:
        fail("candidate_bundle_worker_tree_mismatch")
    file_identity(record_path, "candidate_bundle_record", expected_mode=0o400)
    return record


def ensure_candidate_bundle(state_path, state, worktree, gate_chain, revalidate):
    root = state_path.parent
    root_identity = directory_identity(root, "candidate_bundle_state_root", 0o700)
    recipe = candidate_build_recipe(state, root, worktree)
    source_trees = exact_source_trees(worktree)
    intent_path = root / "candidate-bundle-intent.json"
    bundle_root = root / "candidate-bundle"
    recover_intent_temporary(root, state, gate_chain, recipe, source_trees)
    if bundle_root.exists():
        return load_candidate_bundle(root, state, gate_chain), "exact_replay"
    if intent_path.exists():
        existing = read_json(intent_path, "candidate_bundle_intent", 0o600)
        validate_intent(existing, state, gate_chain, recipe, source_trees)
        intent = existing
    else:
        directories = initialize_candidate_build_directories(recipe)
        toolchain = resolve_candidate_toolchain(worktree, directories)
        intent = candidate_bundle_intent(
            state,
            gate_chain,
            recipe,
            source_trees,
            toolchain,
            secrets.token_hex(16),
        )
        write_new_file_atomic(
            intent_path,
            f".candidate-bundle-intent.json.tmp-{intent['staging_nonce']}",
            (canonical_json(intent) + "\n").encode("utf-8"),
            0o600,
        )
    recover_candidate_staging(root, state, worktree, gate_chain, recipe, source_trees, intent)
    if bundle_root.exists():
        return load_candidate_bundle(root, state, gate_chain), "exact_replay"
    validate_build_directories(intent["toolchain"]["directories"], recipe, True)
    current_toolchain = resolve_candidate_toolchain(
        worktree, intent["toolchain"]["directories"]
    )
    if current_toolchain != intent["toolchain"]:
        fail("candidate_build_toolchain_drift")
    tools = execute_candidate_build(
        state, worktree, root, recipe, intent["toolchain"]
    )
    if directory_identity(root, "candidate_bundle_state_root", 0o700) != root_identity:
        fail("candidate_bundle_state_root_changed")
    revalidate()
    after_sources = exact_source_trees(worktree)
    if after_sources != source_trees:
        fail("candidate_bundle_source_changed")
    if directory_identity(root, "candidate_bundle_state_root", 0o700) != root_identity:
        fail("candidate_bundle_state_root_changed")
    create_candidate_bundle(
        state, root, worktree, gate_chain, recipe, tools, source_trees, intent
    )
    return load_candidate_bundle(root, state, gate_chain), "created"


def record_file_identity(root):
    return file_identity(
        root / "candidate-bundle" / "bundle.json",
        "candidate_bundle_record",
        expected_mode=0o400,
    )


def validate_d2_manifest_binding(manifest, state, worktree, root, record):
    if not isinstance(manifest, dict):
        fail("d2_manifest_invalid")
    candidates = manifest.get("candidates")
    source_trees = manifest.get("source_trees")
    if not isinstance(candidates, dict) or not isinstance(source_trees, dict):
        fail("d2_manifest_candidate_binding_invalid")
    artifacts = {value["candidate"]: value["artifact"] for value in record["artifacts"]}
    for candidate, _source, _destination in REPO_CANDIDATE_ARTIFACTS:
        expected = {
            "path": artifacts[candidate]["path"],
            "sha256": artifacts[candidate]["sha256"],
        }
        if candidates.get(candidate) != expected:
            fail(f"d2_candidate_{candidate}_bundle_mismatch")
    worker_file = next(
        value["artifact"]
        for value in record["worker"]["files"]
        if value["name"] == "worker.mjs"
    )
    if candidates.get("codex_worker") != {
        "path": worker_file["path"],
        "sha256": worker_file["sha256"],
    }:
        fail("d2_candidate_codex_worker_bundle_mismatch")
    expected_source_trees = {
        "codex_worker": {
            "root": record["worker"]["root"],
            "files": list(CODEX_WORKER_SOURCE_FILES),
            "sha256": record["worker"]["sha256"],
        },
        "d2_toolchain": {
            "root": str(worktree / "tools" / "d2-certification"),
            "files": list(D2_TOOLCHAIN_SOURCE_FILES),
            "sha256": record["source_trees"]["d2_toolchain"]["sha256"],
        },
        "certification_transport": {
            "root": str(worktree / "tools" / "d2-certification-transport"),
            "files": list(CERTIFICATION_TRANSPORT_SOURCE_FILES),
            "sha256": record["source_trees"]["certification_transport"]["sha256"],
        },
    }
    if source_trees != expected_source_trees:
        fail("d2_manifest_source_tree_bundle_mismatch")
    if pathlib.Path(state["worktree_path"]) != worktree:
        fail("candidate_bundle_worktree_mismatch")
    return record_file_identity(root)
