import argparse
import datetime
import fcntl
import hashlib
import json
import os
import pathlib
import re
import secrets
import shlex
import shutil
import stat
import subprocess
import sys
import time
import urllib.parse

import d3_candidate_bundle as candidate_bundle
import d3_candidate_io as candidate_io
import d3_gate_container as gate_container
from d3_candidate_bundle import (
    CandidateBundleError,
    ensure_candidate_bundle,
    load_candidate_bundle,
    record_file_identity,
    validate_d2_manifest_binding,
)


SCHEMA_VERSION = 1
MAX_JSON_BYTES = 1024 * 1024
MAX_PROCESS_OUTPUT_BYTES = 1024 * 1024
D2_RUN_MAX_ENTRIES = 10000
D2_RUN_MAX_BYTES = 64 * 1024 * 1024
SHA_PATTERN = re.compile(r"^[0-9a-f]{40}$")
DIGEST_PATTERN = re.compile(r"^[0-9a-f]{64}$")
NAME_PATTERN = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._/-]{0,191}$")
REPOSITORY_PATTERN = re.compile(r"^[A-Za-z0-9_.-]{1,100}/[A-Za-z0-9_.-]{1,100}$")
REPOSITORY_COMPONENT_PATTERN = re.compile(r"^[A-Za-z0-9_.-]{1,100}$")
POSTGRES_DATABASE_PATTERN = re.compile(r"^starring_(?:test|d3)(?:_[a-z0-9_]{1,48})?$")
GITHUB_SCP_PATTERN = re.compile(r"^git@github\.com:(?P<path>[^:]+)$")
UTC_PATTERN = re.compile(
    r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]{1,6})?Z$"
)
ZERO_DIGEST = "0" * 64
D2_STEP_CODES = (
    "isolated_target_created",
    "prior_guild_ownership_absent",
    "candidate_processes_started",
    "oauth_authenticated",
    "one_shot_authoring_submitted",
    "encrypted_preview_ready",
    "product_decisions_applied",
    "runtime_live",
    "create_and_join_executed",
    "duplicate_interaction_suppressed",
    "runtime_restarted_with_canonical_process_identity",
    "route_and_instance_reconstructed",
    "indeterminate_effect_reconciled",
    "target_replaced",
    "gateway_disconnect_failed_closed",
    "test_resources_torn_down",
    "total_absence_confirmed",
)
D2_CERTIFICATION_CLASS = "commercial_human_v1"
D2_HUMAN_BOUNDARIES = (
    "create_disposable_discord_guild",
    "complete_discord_oauth",
    "confirm_product_preview",
    "execute_real_discord_interactions",
    "confirm_replacement_preview",
    "delete_disposable_discord_guild",
)
D2A_TAINT_NAME = "d2a-taint.json"
REQUIRED_ACTIONS_WORKFLOW = {
    "name": "CI",
    "path": ".github/workflows/ci.yml",
    "state": "active",
}
REQUIRED_ACTIONS_JOBS = ("checks", "postgres")
REQUIRED_GATE_COMMANDS = (
    "python3 tools/ci/scan_tracked_secrets.py",
    "cargo fmt --all -- --check",
    "cargo build --locked --workspace --all-targets",
    "cargo test --locked --workspace",
    "cargo clippy --locked --workspace --all-targets -- -D warnings",
    "cargo build --locked -p interaction-smoke --features unsafe-dev-activation",
    "npm --prefix tools/codex-worker run check",
    "npm --prefix tools/codex-worker test",
    "npm --prefix eval/codex-worker-slo run check",
    "npm --prefix eval/design-harness ci",
    "npm --prefix eval/design-harness run audit",
    "npm --prefix eval/design-harness run check",
    "python3 -m unittest discover -s tools/d2-certification -p 'test_*.py'",
    "python3 -m unittest discover -s tools/d3-certification -p 'test_*.py'",
    "node --test tools/d2-certification/product_driver.test.mjs",
    "cargo fmt --manifest-path tools/d2-certification-transport/Cargo.toml -- --check",
    "cargo test --locked --manifest-path tools/d2-certification-transport/Cargo.toml",
    "cargo clippy --locked --manifest-path tools/d2-certification-transport/Cargo.toml --all-targets -- -D warnings",
    "python3 -m unittest discover -s tools/d2-maintenance -p 'test_*.py'",
    "node --test tools/d2-maintenance/headless_product_runner.test.mjs",
    "cargo fmt --manifest-path tools/d2-maintenance/session-issuer/Cargo.toml -- --check",
    "cargo test --locked --manifest-path tools/d2-maintenance/session-issuer/Cargo.toml",
    "cargo clippy --locked --manifest-path tools/d2-maintenance/session-issuer/Cargo.toml --all-targets -- -D warnings",
    "cargo test --locked -p automation-ruleset-postgres -- --ignored --test-threads=1",
    "cargo test --locked -p automation-instance-postgres -- --ignored --test-threads=1",
    "cargo test --locked -p automation-panel-installation-postgres -- --ignored --test-threads=1",
    "cargo test --locked -p automation-ruleset-activation-postgres -- --ignored --test-threads=1",
    "cargo test --locked -p authoring-promotion-postgres -- --ignored --test-threads=1",
    "cargo test --locked -p authoring-application-postgres -- --ignored --test-threads=1",
    "cargo test --locked -p automation-ruleset-dispatch -- --ignored --test-threads=1",
    "cargo test --locked -p automation-ruleset-readiness -- --ignored --test-threads=1",
    "cargo test --locked -p automation-runtime-convergence-postgres -- --ignored --test-threads=1",
    "cargo test --locked -p automation-runtime-execution-postgres --test postgres_security -- --ignored --test-threads=1",
    "cargo test --locked -p automation-runtime-serving-postgres -- --ignored --test-threads=1",
    "cargo test --locked -p automation-runtime-interaction-postgres -- --ignored --test-threads=1",
    "cargo test --locked -p automation-runtime-panel-postgres -- --ignored --test-threads=1",
)
GATE_BOOTSTRAP_WORK_DIRECTORIES = (
    "cargo-home",
    "home",
    "target",
    "tmp",
    "xdg-cache",
    "xdg-config",
)
GATE_BOOTSTRAP_DIRECTORIES = (
    "bin",
    "git",
    "node-stage",
    "npm-cache",
    "vendor",
)
GATE_BOOTSTRAP_FILES = (
    "cargo-config.toml",
    "issuer-cargo-config.toml",
    "native-cargo-config.toml",
    "native-transport-cargo-config.toml",
    "transport-cargo-config.toml",
)
GATE_BOOTSTRAP_TEMPORARY_PATHS = (
    ("node-stage/package-lock.json", 0o400),
    ("node-stage/package.json", 0o400),
    ("issuer-cargo-vendor-config.txt", 0o600),
    ("issuer-staging-cargo-config.toml", 0o600),
    ("transport-cargo-vendor-config.txt", 0o600),
    ("transport-staging-cargo-config.toml", 0o600),
    ("workspace-cargo-config.toml", 0o600),
)
SAFE_ENVIRONMENT_NAMES = (
    "CARGO_HOME",
    "DEVELOPER_DIR",
    "HOME",
    "LANG",
    "LOGNAME",
    "PATH",
    "RUSTUP_HOME",
    "SHELL",
    "TMPDIR",
    "USER",
    "XDG_CACHE_HOME",
    "XDG_CONFIG_HOME",
)
FORBIDDEN_COMMAND_PATTERNS = (
    re.compile(r"(?:^|[;&|()\s])(?:env|printenv|set|export)(?:\s|$)"),
    re.compile(r"(?:echo|printf)\s+[^\n]*(?:\$\{|\$[A-Za-z_])"),
    re.compile(r"(?:token|secret|password|cookie)\s*=", re.IGNORECASE),
    re.compile(r"(?:Bearer|Bot)\s+[A-Za-z0-9._~-]+", re.IGNORECASE),
    re.compile(r"postgres(?:ql)?://[^\s]+", re.IGNORECASE),
    re.compile(r"\bcf(?:at|ut)_[A-Za-z0-9_-]+\b"),
    re.compile(r"security\s+find-generic-password"),
    re.compile(r"(?:/proc/[^\s]*/environ|\.env(?:\s|$))"),
)
FORBIDDEN_GIT_TRANSPORT_PATTERN = (
    r"^(url\..*\.(insteadof|pushinsteadof)|"
    r"remote\.origin\.(pushurl|uploadpack|receivepack|proxy)|"
    r"core\.(sshcommand|gitproxy)|"
    r"http(\..*)?\.(proxy|sslverify|sslcainfo|sslcapath|sslcert|sslkey|"
    r"curloptresolve|followredirects))$"
)


class D3Error(Exception):
    pass


def fail(code):
    raise D3Error(code)


def canonical_json(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def sha256_bytes(value):
    return hashlib.sha256(value).hexdigest()


def utc_now():
    return (
        datetime.datetime.now(datetime.timezone.utc)
        .isoformat(timespec="microseconds")
        .replace("+00:00", "Z")
    )


def strict_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            fail("json_duplicate_key")
        result[key] = value
    return result


def load_json_bytes(raw, label):
    if not raw or len(raw) > MAX_JSON_BYTES:
        fail(f"{label}_size_invalid")
    try:
        return json.loads(raw, object_pairs_hook=strict_object)
    except D3Error:
        raise
    except (UnicodeDecodeError, json.JSONDecodeError):
        fail(f"{label}_json_invalid")


def load_json_file(path, label, mode=0o600):
    raw = read_owned_bytes(path, label, mode, MAX_JSON_BYTES)
    return load_json_bytes(raw, label)


def load_small_ascii(path, label):
    try:
        return read_owned_bytes(path, label, 0o600, 128).decode("ascii").strip()
    except UnicodeDecodeError:
        fail(f"{label}_encoding_invalid")


def validate_sha(value, label):
    if not isinstance(value, str) or not SHA_PATTERN.fullmatch(value):
        fail(f"{label}_invalid")
    return value


def validate_digest(value, label):
    if not isinstance(value, str) or not DIGEST_PATTERN.fullmatch(value):
        fail(f"{label}_invalid")
    return value


def validate_name(value, label):
    if not isinstance(value, str) or not NAME_PATTERN.fullmatch(value) or ".." in value:
        fail(f"{label}_invalid")
    return value


def validate_timestamp(value, label):
    if not isinstance(value, str) or not UTC_PATTERN.fullmatch(value):
        fail(f"{label}_invalid")
    try:
        parsed = datetime.datetime.fromisoformat(value[:-1] + "+00:00")
    except ValueError:
        fail(f"{label}_invalid")
    if parsed.tzinfo != datetime.timezone.utc:
        fail(f"{label}_invalid")
    return value


def valid_schema_version(value):
    return type(value) is int and value == SCHEMA_VERSION


def absolute_path(raw, label):
    path = pathlib.Path(raw)
    if not path.is_absolute() or os.path.realpath(path) != str(path):
        fail(f"{label}_path_invalid")
    return path


def require_directory(path, label, mode=None):
    try:
        metadata = path.lstat()
    except OSError as error:
        fail(f"{label}_unavailable:{error.__class__.__name__}")
    if not stat.S_ISDIR(metadata.st_mode) or path.is_symlink():
        fail(f"{label}_not_directory")
    if metadata.st_uid != os.getuid():
        fail(f"{label}_owner_invalid")
    if mode is not None and stat.S_IMODE(metadata.st_mode) != mode:
        fail(f"{label}_mode_invalid")
    return metadata


def require_regular(path, label, mode=None, allow_empty=False):
    try:
        metadata = path.lstat()
    except OSError as error:
        fail(f"{label}_unavailable:{error.__class__.__name__}")
    if not stat.S_ISREG(metadata.st_mode) or path.is_symlink():
        fail(f"{label}_not_regular")
    if metadata.st_uid != os.getuid():
        fail(f"{label}_owner_invalid")
    if mode is not None and stat.S_IMODE(metadata.st_mode) != mode:
        fail(f"{label}_mode_invalid")
    if not allow_empty and metadata.st_size == 0:
        fail(f"{label}_empty")
    return metadata


def read_owned_bytes(path, label, mode, maximum, allow_empty=False):
    flags = os.O_RDONLY
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        fail(f"{label}_unavailable:{error.__class__.__name__}")
    try:
        metadata = os.fstat(descriptor)
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_uid != os.getuid()
            or stat.S_IMODE(metadata.st_mode) != mode
        ):
            fail(f"{label}_ownership_invalid")
        if metadata.st_size > maximum:
            fail(f"{label}_size_invalid")
        raw = bytearray()
        while len(raw) <= maximum:
            chunk = os.read(descriptor, min(65536, maximum + 1 - len(raw)))
            if not chunk:
                break
            raw.extend(chunk)
        if len(raw) > maximum:
            fail(f"{label}_size_invalid")
        if not allow_empty and not raw:
            fail(f"{label}_empty")
        return bytes(raw)
    finally:
        os.close(descriptor)


def fsync_directory(path):
    descriptor = os.open(path, os.O_RDONLY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def write_all(descriptor, payload):
    remaining = memoryview(payload)
    while remaining:
        written = os.write(descriptor, remaining)
        if written <= 0:
            fail("evidence_write_failed")
        remaining = remaining[written:]


def write_new_file(path, value):
    payload = value if isinstance(value, bytes) else value.encode("utf-8")
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    descriptor = os.open(path, flags, 0o600)
    try:
        write_all(descriptor, payload)
        os.fsync(descriptor)
    finally:
        os.close(descriptor)
    fsync_directory(path.parent)


def write_new_json(path, value):
    write_new_file(path, canonical_json(value) + "\n")


def seal_record(value):
    record = dict(value)
    record["record_sha256"] = sha256_bytes(canonical_json(record).encode("utf-8"))
    return record


def verify_sealed_record(value, label):
    if not isinstance(value, dict):
        fail(f"{label}_invalid")
    digest = validate_digest(value.get("record_sha256"), f"{label}_record")
    payload = dict(value)
    payload.pop("record_sha256")
    if sha256_bytes(canonical_json(payload).encode("utf-8")) != digest:
        fail(f"{label}_record_mismatch")
    return digest


class StateLock:
    def __init__(self, root):
        self.path = root / ".lock"
        self.handle = None

    def __enter__(self):
        flags = os.O_RDWR | os.O_CREAT
        if hasattr(os, "O_NOFOLLOW"):
            flags |= os.O_NOFOLLOW
        descriptor = os.open(self.path, flags, 0o600)
        self.handle = os.fdopen(descriptor, "r+b")
        metadata = os.fstat(descriptor)
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_uid != os.getuid()
            or stat.S_IMODE(metadata.st_mode) != 0o600
        ):
            self.handle.close()
            fail("state_lock_invalid")
        fcntl.flock(descriptor, fcntl.LOCK_EX)
        return self

    def __exit__(self, kind, value, traceback):
        if self.handle is not None:
            self.handle.close()


def sanitized_environment(extra=None):
    environment = {
        name: os.environ[name]
        for name in SAFE_ENVIRONMENT_NAMES
        if name in os.environ
    }
    environment["GIT_TERMINAL_PROMPT"] = "0"
    environment["LC_ALL"] = "C"
    environment["CARGO_INCREMENTAL"] = "0"
    if extra is not None:
        if not isinstance(extra, dict) or any(
            not isinstance(name, str) or not isinstance(value, str)
            for name, value in extra.items()
        ):
            fail("process_environment_invalid")
        environment.update(extra)
    return environment


def run_process(
    argv,
    cwd,
    label,
    allowed=(0,),
    timeout=None,
    discard=False,
    postgres_database_url=None,
):
    environment = sanitized_environment()
    if postgres_database_url is not None:
        environment["STARRING_TEST_DATABASE_URL"] = postgres_database_url
    try:
        result = subprocess.run(
            argv,
            cwd=cwd,
            env=environment,
            stdout=subprocess.DEVNULL if discard else subprocess.PIPE,
            stderr=subprocess.DEVNULL if discard else subprocess.PIPE,
            timeout=timeout,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        fail(f"{label}_unavailable:{error.__class__.__name__}")
    if result.returncode not in allowed:
        fail(f"{label}_failed:{result.returncode}")
    if discard:
        return result.returncode, b""
    output = result.stdout
    if len(output) > MAX_PROCESS_OUTPUT_BYTES:
        fail(f"{label}_output_too_large")
    return result.returncode, output


def git(repo, arguments, label, allowed=(0,)):
    return run_process(["git", "-C", str(repo), *arguments], repo, label, allowed)[1]


def git_text(repo, arguments, label):
    try:
        return git(repo, arguments, label).decode("ascii").strip()
    except UnicodeDecodeError:
        fail(f"{label}_output_invalid")


def github_repository_path(path):
    if path.startswith("/"):
        path = path[1:]
    if path.endswith(".git"):
        path = path[:-4]
    parts = path.split("/")
    if (
        len(parts) != 2
        or not all(REPOSITORY_COMPONENT_PATTERN.fullmatch(part) for part in parts)
    ):
        fail("remote_url_invalid")
    return "/".join(parts)


def canonical_github_repository(value):
    if not value or any(character.isspace() for character in value):
        fail("remote_url_invalid")
    scp = GITHUB_SCP_PATTERN.fullmatch(value)
    if scp is not None:
        repository = github_repository_path(scp.group("path"))
        if value not in (
            f"git@github.com:{repository}",
            f"git@github.com:{repository}.git",
        ):
            fail("remote_url_invalid")
        return repository
    try:
        parsed = urllib.parse.urlsplit(value)
        port = parsed.port
    except ValueError:
        fail("remote_url_invalid")
    if parsed.scheme == "https":
        if parsed.username is not None or parsed.password is not None:
            fail("remote_url_credentials_forbidden")
        if (
            parsed.hostname != "github.com"
            or port is not None
            or parsed.query
            or parsed.fragment
        ):
            fail("remote_url_invalid")
        repository = github_repository_path(parsed.path)
        if value not in (
            f"https://github.com/{repository}",
            f"https://github.com/{repository}.git",
        ):
            fail("remote_url_invalid")
        return repository
    if parsed.scheme == "ssh":
        if parsed.password is not None:
            fail("remote_url_credentials_forbidden")
        if (
            parsed.username != "git"
            or parsed.hostname != "github.com"
            or port is not None
            or parsed.query
            or parsed.fragment
        ):
            fail("remote_url_invalid")
        repository = github_repository_path(parsed.path)
        if value not in (
            f"ssh://git@github.com/{repository}",
            f"ssh://git@github.com/{repository}.git",
        ):
            fail("remote_url_invalid")
        return repository
    fail("remote_url_invalid")


def github_repository_from_remote(repo, remote):
    try:
        raw = git(
            repo,
            ["config", "--get-all", f"remote.{remote}.url"],
            "remote_url",
            allowed=(0, 1),
        ).decode("ascii")
    except UnicodeDecodeError:
        fail("remote_url_invalid")
    values = raw.splitlines()
    if len(values) != 1:
        fail("remote_url_invalid")
    return canonical_github_repository(values[0])


def validate_release_transport(repo, remote):
    if remote != "origin":
        fail("release_ref_invalid")
    status, _ = run_process(
        [
            "git",
            "-C",
            str(repo),
            "config",
            "--get-regexp",
            FORBIDDEN_GIT_TRANSPORT_PATTERN,
        ],
        repo,
        "git_transport_overrides",
        allowed=(0, 1),
        discard=True,
    )
    if status == 0:
        fail("git_transport_override_forbidden")


def git_object(repo, sha, expected, label):
    observed = git_text(repo, ["cat-file", "-t", sha], f"{label}_type")
    if observed != expected:
        fail(f"{label}_type_invalid")


def commit_tree(repo, sha, label):
    git_object(repo, sha, "commit", label)
    return validate_sha(git_text(repo, ["show", "-s", "--format=%T", sha], f"{label}_tree"), f"{label}_tree")


def commit_parents(repo, sha, label):
    raw = git_text(repo, ["show", "-s", "--format=%P", sha], f"{label}_parents")
    parents = raw.split()
    for index, parent in enumerate(parents):
        validate_sha(parent, f"{label}_parent_{index}")
    return parents


def git_file_identity(repo, commit, path, label):
    validate_sha(commit, f"{label}_commit")
    if path != REQUIRED_ACTIONS_WORKFLOW["path"]:
        fail(f"{label}_path_invalid")
    raw = git(repo, ["ls-tree", "-z", commit, "--", path], f"{label}_entry")
    entries = raw.split(b"\0")
    if len(entries) != 2 or entries[1] != b"":
        fail(f"{label}_entry_invalid")
    try:
        metadata, observed_path = entries[0].split(b"\t", 1)
        mode, kind, object_sha = metadata.decode("ascii").split(" ")
        observed_path = observed_path.decode("utf-8")
    except (UnicodeDecodeError, ValueError):
        fail(f"{label}_entry_invalid")
    if mode != "100644" or kind != "blob" or observed_path != path:
        fail(f"{label}_entry_invalid")
    validate_sha(object_sha, f"{label}_blob")
    content = git(repo, ["cat-file", "blob", object_sha], f"{label}_content")
    return {
        "path": path,
        "git_blob_sha": object_sha,
        "sha256": sha256_bytes(content),
    }


def validate_gate(command):
    if not isinstance(command, str) or not command or len(command.encode("utf-8")) > 8192:
        fail("gate_command_invalid")
    if "\x00" in command or "\n" in command or "\r" in command:
        fail("gate_command_invalid")
    try:
        parts = shlex.split(command)
    except ValueError:
        fail("gate_command_invalid")
    if not parts:
        fail("gate_command_invalid")
    if any(pattern.search(command) for pattern in FORBIDDEN_COMMAND_PATTERNS):
        fail("gate_command_sensitive")
    return command


def load_postgres_database_url(raw):
    if raw is None:
        fail("postgres_database_url_file_required")
    path = absolute_path(raw, "postgres_database_url")
    try:
        value = read_owned_bytes(
            path,
            "postgres_database_url",
            0o600,
            8192,
        ).decode("utf-8")
    except UnicodeDecodeError:
        fail("postgres_database_url_encoding_invalid")
    if value.endswith("\n"):
        value = value[:-1]
    if (
        not value
        or any(character.isspace() for character in value)
        or "\x00" in value
    ):
        fail("postgres_database_url_invalid")
    try:
        parsed = urllib.parse.urlsplit(value)
        port = parsed.port
        username = parsed.username
        password = parsed.password
        hostname = parsed.hostname
    except ValueError:
        fail("postgres_database_url_invalid")
    if (
        parsed.scheme not in ("postgres", "postgresql")
        or hostname not in ("127.0.0.1", "localhost", "::1")
        or port is None
        or not 1 <= port <= 65535
        or not username
        or password is None
        or not password
        or parsed.fragment
        or parsed.query
        or parsed.path.count("/") != 1
    ):
        fail("postgres_database_url_invalid")
    try:
        database = urllib.parse.unquote(parsed.path[1:], errors="strict")
    except (UnicodeDecodeError, ValueError):
        fail("postgres_database_url_invalid")
    if not POSTGRES_DATABASE_PATTERN.fullmatch(database):
        fail("postgres_database_url_invalid")
    return value


def gate_digests(commands):
    validated = tuple(validate_gate(command) for command in commands)
    if validated != REQUIRED_GATE_COMMANDS:
        fail("gate_manifest_mismatch")
    return [
        sha256_bytes(command.encode("utf-8"))
        for command in REQUIRED_GATE_COMMANDS
    ]


def state_name(pr_number, expected_head, expected_base):
    digest = sha256_bytes(f"{pr_number}:{expected_head}:{expected_base}".encode("ascii"))
    return f"d3-pr-{pr_number}-{digest[:12]}"


def create_state_root(output_root, name):
    path = output_root / name
    try:
        path.mkdir(mode=0o700)
        fsync_directory(output_root)
    except FileExistsError:
        require_directory(path, "state_root", 0o700)
    return path


def load_state(raw):
    manifest_path = absolute_path(raw, "state")
    manifest = load_json_file(manifest_path, "state", 0o600)
    expected_fields = {
        "schema_version",
        "kind",
        "pr_number",
        "repo_path",
        "remote",
        "github_repository",
        "base_ref",
        "expected_head",
        "expected_base",
        "merge_commit",
        "merge_tree",
        "merge_parents",
        "worktree_path",
        "gate_command_sha256",
        "created_at",
    }
    if not isinstance(manifest, dict) or set(manifest) != expected_fields:
        fail("state_fields_invalid")
    if (
        not valid_schema_version(manifest["schema_version"])
        or manifest["kind"] != "starring.d3.exact-tree-state.v1"
    ):
        fail("state_schema_invalid")
    if type(manifest["pr_number"]) is not int or manifest["pr_number"] <= 0:
        fail("state_pr_invalid")
    validate_name(manifest["remote"], "state_remote")
    if (
        not isinstance(manifest["github_repository"], str)
        or not REPOSITORY_PATTERN.fullmatch(manifest["github_repository"])
    ):
        fail("state_github_repository_invalid")
    validate_name(manifest["base_ref"], "state_base_ref")
    validate_sha(manifest["expected_head"], "state_expected_head")
    validate_sha(manifest["expected_base"], "state_expected_base")
    validate_sha(manifest["merge_commit"], "state_merge_commit")
    validate_sha(manifest["merge_tree"], "state_merge_tree")
    if manifest["merge_parents"] != [manifest["expected_base"], manifest["expected_head"]]:
        fail("state_parents_invalid")
    required_gate_digests = [
        sha256_bytes(command.encode("utf-8"))
        for command in REQUIRED_GATE_COMMANDS
    ]
    if manifest["gate_command_sha256"] != required_gate_digests:
        fail("state_gates_invalid")
    validate_timestamp(manifest["created_at"], "state_created_at")
    repo = absolute_path(manifest["repo_path"], "state_repo")
    worktree = absolute_path(manifest["worktree_path"], "state_worktree")
    require_directory(repo, "state_repo")
    if github_repository_from_remote(repo, manifest["remote"]) != manifest["github_repository"]:
        fail("state_github_repository_mismatch")
    validate_release_transport(repo, manifest["remote"])
    require_directory(manifest_path.parent, "state_root", 0o700)
    require_directory(worktree, "state_worktree", 0o700)
    digest_path = manifest_path.with_name("state.sha256")
    raw_digest = load_small_ascii(digest_path, "state_digest")
    validate_digest(raw_digest, "state_digest")
    observed_digest = sha256_bytes(canonical_json(manifest).encode("utf-8"))
    if raw_digest != observed_digest:
        fail("state_digest_mismatch")
    validate_worktree(manifest, repo, worktree)
    return manifest_path, manifest, raw_digest, repo, worktree


def state_lock_target(raw):
    path = absolute_path(raw, "state")
    require_directory(path.parent, "state_root", 0o700)
    return path, path.parent


def validate_worktree(manifest, repo, worktree):
    head = validate_sha(git_text(worktree, ["rev-parse", "HEAD"], "worktree_head"), "worktree_head")
    if head != manifest["merge_commit"]:
        fail("worktree_head_mismatch")
    symbolic_code, _ = run_process(
        ["git", "-C", str(worktree), "symbolic-ref", "-q", "HEAD"],
        worktree,
        "worktree_detached",
        allowed=(0, 1),
    )
    if symbolic_code == 0:
        fail("worktree_not_detached")
    tree = commit_tree(repo, head, "worktree_commit")
    if tree != manifest["merge_tree"]:
        fail("worktree_tree_mismatch")
    dirty = git(
        worktree,
        ["status", "--porcelain=v1", "--untracked-files=all"],
        "worktree_status",
    )
    if dirty:
        fail("worktree_tracked_changes")


def fetch_candidate(repo, remote, base_ref, pr_number, prefix):
    head_ref = f"refs/d3/pr-{pr_number}/{prefix}-head"
    merge_ref = f"refs/d3/pr-{pr_number}/{prefix}-merge"
    base_remote_ref = f"refs/remotes/{remote}/{base_ref}"
    git(
        repo,
        [
            "fetch",
            "--atomic",
            "--no-tags",
            "--force",
            remote,
            f"refs/heads/{base_ref}:{base_remote_ref}",
            f"refs/pull/{pr_number}/head:{head_ref}",
            f"refs/pull/{pr_number}/merge:{merge_ref}",
        ],
        f"{prefix}_fetch",
    )
    return (
        validate_sha(git_text(repo, ["rev-parse", head_ref], f"{prefix}_head"), f"{prefix}_head"),
        validate_sha(git_text(repo, ["rev-parse", base_remote_ref], f"{prefix}_base"), f"{prefix}_base"),
        validate_sha(git_text(repo, ["rev-parse", merge_ref], f"{prefix}_merge"), f"{prefix}_merge"),
    )


def fetch_main(repo, remote, base_ref):
    base_remote_ref = f"refs/remotes/{remote}/{base_ref}"
    git(
        repo,
        [
            "fetch",
            "--atomic",
            "--no-tags",
            "--force",
            remote,
            f"refs/heads/{base_ref}:{base_remote_ref}",
        ],
        "finalize_fetch",
    )
    return validate_sha(
        git_text(repo, ["rev-parse", base_remote_ref], "finalize_main"),
        "finalize_main",
    )


def command_prepare(arguments):
    repo = absolute_path(arguments.repo, "repo")
    output_root = absolute_path(arguments.output_root, "output_root")
    require_directory(repo, "repo")
    require_directory(output_root, "output_root", 0o700)
    remote = validate_name(arguments.remote, "remote")
    base_ref = validate_name(arguments.base_ref, "base_ref")
    if remote != "origin" or base_ref != "main":
        fail("release_ref_invalid")
    expected_head = validate_sha(arguments.expected_head, "expected_head")
    expected_base = validate_sha(arguments.expected_base, "expected_base")
    if arguments.pr_number <= 0:
        fail("pr_number_invalid")
    digests = gate_digests(arguments.gate)
    github_repository = github_repository_from_remote(repo, remote)
    validate_release_transport(repo, remote)
    name = state_name(arguments.pr_number, expected_head, expected_base)
    pending_root = output_root / name
    root_existed = os.path.lexists(pending_root)
    if not root_existed:
        try:
            gate_container.require_bootstrap_start_capacity(output_root)
        except gate_container.GateContainerError as error:
            fail(str(error))
    elif not (pending_root / "state.json").exists():
        try:
            gate_container.require_bootstrap_start_capacity(pending_root)
        except gate_container.GateContainerError as error:
            fail(str(error))
    root = create_state_root(output_root, name)
    with StateLock(root):
        state_path = root / "state.json"
        intent_path = root / "prepare-intent.json"
        intent = {
            "schema_version": SCHEMA_VERSION,
            "kind": "starring.d3.prepare-intent.v1",
            "pr_number": arguments.pr_number,
            "repo_path": str(repo),
            "remote": remote,
            "github_repository": github_repository,
            "base_ref": base_ref,
            "expected_head": expected_head,
            "expected_base": expected_base,
            "gate_command_sha256": digests,
        }
        if intent_path.exists():
            if load_json_file(intent_path, "prepare_intent", 0o600) != intent:
                fail("prepare_replay_mismatch")
        else:
            write_new_json(intent_path, intent)
        if state_path.exists():
            digest_path = root / "state.sha256"
            if not digest_path.exists():
                interrupted_state = load_json_file(state_path, "state", 0o600)
                if (
                    not isinstance(interrupted_state, dict)
                    or interrupted_state.get("pr_number") != arguments.pr_number
                    or interrupted_state.get("repo_path") != str(repo)
                    or interrupted_state.get("github_repository") != github_repository
                    or interrupted_state.get("expected_head") != expected_head
                    or interrupted_state.get("expected_base") != expected_base
                    or interrupted_state.get("gate_command_sha256") != digests
                ):
                    fail("prepare_interrupted_state_invalid")
                interrupted_digest = sha256_bytes(
                    canonical_json(interrupted_state).encode("utf-8")
                )
                write_new_file(digest_path, interrupted_digest + "\n")
            _, state, digest, _, _ = load_state(str(state_path))
            expected = {
                "pr_number": arguments.pr_number,
                "repo_path": str(repo),
                "remote": remote,
                "github_repository": github_repository,
                "base_ref": base_ref,
                "expected_head": expected_head,
                "expected_base": expected_base,
                "gate_command_sha256": digests,
            }
            if any(state[key] != value for key, value in expected.items()):
                fail("prepare_replay_mismatch")
            return {
                "state": str(state_path),
                "state_sha256": digest,
                "merge_commit": state["merge_commit"],
                "merge_tree": state["merge_tree"],
                "disposition": "exact_replay",
            }
        head, base, merge = fetch_candidate(repo, remote, base_ref, arguments.pr_number, "prepare")
        if head != expected_head:
            fail("pr_head_mismatch")
        if base != expected_base:
            fail("pr_base_mismatch")
        parents = commit_parents(repo, merge, "merge_commit")
        if parents != [expected_base, expected_head]:
            fail("merge_parents_mismatch")
        tree = commit_tree(repo, merge, "merge_commit")
        worktree = root / "worktree"
        if worktree.exists():
            metadata = require_directory(worktree, "worktree")
            if stat.S_IMODE(metadata.st_mode) & 0o022:
                fail("worktree_mode_invalid")
            worktree.chmod(0o700)
        else:
            git(repo, ["worktree", "add", "--detach", str(worktree), merge], "worktree_add")
            worktree.chmod(0o700)
            fsync_directory(root)
        state = {
            "schema_version": SCHEMA_VERSION,
            "kind": "starring.d3.exact-tree-state.v1",
            "pr_number": arguments.pr_number,
            "repo_path": str(repo),
            "remote": remote,
            "github_repository": github_repository,
            "base_ref": base_ref,
            "expected_head": expected_head,
            "expected_base": expected_base,
            "merge_commit": merge,
            "merge_tree": tree,
            "merge_parents": parents,
            "worktree_path": str(worktree),
            "gate_command_sha256": digests,
            "created_at": utc_now(),
        }
        validate_worktree(state, repo, worktree)
        digest = sha256_bytes(canonical_json(state).encode("utf-8"))
        write_new_json(state_path, state)
        write_new_file(root / "state.sha256", digest + "\n")
        return {
            "state": str(state_path),
            "state_sha256": digest,
            "merge_commit": merge,
            "merge_tree": tree,
            "disposition": "created",
        }


def evidence_record_hash(record):
    payload = dict(record)
    observed = payload.pop("record_sha256", None)
    expected = sha256_bytes(canonical_json(payload).encode("utf-8"))
    if observed != expected:
        fail("gate_evidence_chain_invalid")
    return observed


def load_gate_evidence(
    path,
    state,
    gate_runtime_sha256,
    gate_bootstrap_sha256,
):
    if not path.exists():
        return []
    raw_evidence = read_owned_bytes(
        path, "gate_evidence", 0o600, MAX_JSON_BYTES, allow_empty=True
    )
    records = []
    total = 0
    previous = ZERO_DIGEST
    for raw in raw_evidence.splitlines():
        total += len(raw)
        if total > MAX_JSON_BYTES:
            fail("gate_evidence_too_large")
        record = load_json_bytes(raw, "gate_evidence")
        required = {
            "schema_version",
            "kind",
            "merge_commit",
            "merge_tree",
            "gate_index",
            "command_sha256",
            "attempt",
            "gate_runtime_sha256",
            "gate_bootstrap_sha256",
            "observed_at",
            "previous_sha256",
            "record_sha256",
        }
        if not isinstance(record, dict) or not required.issubset(record):
            fail("gate_evidence_fields_invalid")
        if record["kind"] == "starring.d3.gate-started.v1":
            if set(record) != required:
                fail("gate_evidence_fields_invalid")
        elif record["kind"] == "starring.d3.gate-completed.v1":
            if set(record) != required | {"exit_code", "duration_ms"}:
                fail("gate_evidence_fields_invalid")
            if type(record["exit_code"]) is not int or record["exit_code"] < 0:
                fail("gate_evidence_exit_invalid")
            if type(record["duration_ms"]) is not int or record["duration_ms"] < 0:
                fail("gate_evidence_duration_invalid")
        else:
            fail("gate_evidence_kind_invalid")
        if (
            not valid_schema_version(record["schema_version"])
            or record["merge_commit"] != state["merge_commit"]
            or record["merge_tree"] != state["merge_tree"]
            or record["previous_sha256"] != previous
            or type(record["gate_index"]) is not int
            or not 1 <= record["gate_index"] <= len(state["gate_command_sha256"])
            or record["command_sha256"] != state["gate_command_sha256"][record["gate_index"] - 1]
            or type(record["attempt"]) is not int
            or record["attempt"] <= 0
            or record["gate_runtime_sha256"] != gate_runtime_sha256
            or record["gate_bootstrap_sha256"] != gate_bootstrap_sha256
        ):
            fail("gate_evidence_identity_invalid")
        validate_digest(record["gate_runtime_sha256"], "gate_evidence_runtime")
        validate_digest(record["gate_bootstrap_sha256"], "gate_evidence_bootstrap")
        validate_timestamp(record["observed_at"], "gate_evidence_observed_at")
        previous = evidence_record_hash(record)
        records.append(record)
    return records


def append_gate_evidence(
    path,
    state,
    records,
    value,
    gate_runtime_sha256,
    gate_bootstrap_sha256,
):
    record = {
        "schema_version": SCHEMA_VERSION,
        **value,
        "gate_runtime_sha256": validate_digest(
            gate_runtime_sha256,
            "gate_evidence_runtime",
        ),
        "gate_bootstrap_sha256": validate_digest(
            gate_bootstrap_sha256,
            "gate_evidence_bootstrap",
        ),
        "merge_commit": state["merge_commit"],
        "merge_tree": state["merge_tree"],
        "observed_at": utc_now(),
        "previous_sha256": ZERO_DIGEST if not records else records[-1]["record_sha256"],
    }
    record["record_sha256"] = sha256_bytes(canonical_json(record).encode("utf-8"))
    payload = (canonical_json(record) + "\n").encode("utf-8")
    flags = os.O_WRONLY | os.O_CREAT | os.O_APPEND
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    descriptor = os.open(path, flags, 0o600)
    try:
        metadata = os.fstat(descriptor)
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_uid != os.getuid()
            or stat.S_IMODE(metadata.st_mode) != 0o600
        ):
            fail("gate_evidence_file_invalid")
        write_all(descriptor, payload)
        os.fsync(descriptor)
    finally:
        os.close(descriptor)
    fsync_directory(path.parent)
    records.append(record)
    return record


def gate_status(records, count):
    attempts = {index: 0 for index in range(1, count + 1)}
    completed = {}
    open_attempts = {}
    successful = set()
    for record in records:
        index = record["gate_index"]
        key = (index, record["attempt"])
        if record["kind"] == "starring.d3.gate-started.v1":
            if (
                key in open_attempts
                or key in completed
                or any(open_index == index for open_index, _ in open_attempts)
                or record["attempt"] != attempts[index] + 1
                or any(lower not in successful for lower in range(1, index))
                or index in successful
            ):
                fail("gate_evidence_sequence_invalid")
            attempts[index] = record["attempt"]
            open_attempts[key] = record
        else:
            if key not in open_attempts:
                fail("gate_evidence_sequence_invalid")
            open_attempts.pop(key)
            completed[key] = record
            if record["exit_code"] == 0:
                successful.add(index)
            else:
                successful.discard(index)
    latest = {}
    for (index, attempt), record in completed.items():
        if attempt == attempts[index]:
            latest[index] = record
    return attempts, latest, open_attempts


def ensure_private_directory(path):
    try:
        path.mkdir(mode=0o700)
        fsync_directory(path.parent)
    except FileExistsError:
        pass
    except OSError as error:
        fail(f"gate_sandbox_directory_unavailable:{error.__class__.__name__}")
    require_directory(path, "gate_sandbox_directory", 0o700)
    return path


def ensure_gate_container_runtime(root):
    try:
        identity = gate_container.gate_image_identity()
        identity["gate_orchestration_sha256"] = gate_orchestration_sha256()
        gate_container.validate_bind_roundtrip(root, identity["image_id"])
    except gate_container.GateContainerError as error:
        fail(str(error))
    path = root / "gate-container-runtime.json"
    if path.exists():
        existing = load_gate_container_runtime(root)
        if existing != seal_record(identity):
            fail("gate_container_runtime_changed")
        return existing
    record = seal_record(identity)
    write_new_json(path, record)
    return record


def gate_orchestration_sha256():
    try:
        return candidate_bundle.file_identity(
            pathlib.Path(__file__),
            "gate_orchestration",
        )["sha256"]
    except CandidateBundleError as error:
        fail(str(error))


def load_gate_container_runtime(root):
    value = load_json_file(
        root / "gate-container-runtime.json",
        "gate_container_runtime",
        0o600,
    )
    required = {
        "schema_version",
        "kind",
        "dockerfile_sha256",
        "image_id",
        "postgres_image",
        "gate_orchestration_sha256",
        "runner_policy_sha256",
        "runner_implementation_sha256",
        "daemon_memory_bytes",
        "tool_versions",
        "record_sha256",
    }
    if (
        not isinstance(value, dict)
        or set(value) != required
        or not valid_schema_version(value.get("schema_version"))
        or value.get("kind") != "starring.d3.gate-container-runtime.v1"
        or not isinstance(value.get("dockerfile_sha256"), str)
        or not DIGEST_PATTERN.fullmatch(value["dockerfile_sha256"])
        or not isinstance(value.get("image_id"), str)
        or not re.fullmatch(r"sha256:[0-9a-f]{64}", value["image_id"])
        or value.get("postgres_image") != gate_container.POSTGRES_IMAGE
        or not isinstance(value.get("gate_orchestration_sha256"), str)
        or not DIGEST_PATTERN.fullmatch(value["gate_orchestration_sha256"])
        or value["gate_orchestration_sha256"] != gate_orchestration_sha256()
        or not isinstance(value.get("runner_policy_sha256"), str)
        or not DIGEST_PATTERN.fullmatch(value["runner_policy_sha256"])
        or value["runner_policy_sha256"] != gate_container.runner_policy_sha256()
        or not isinstance(value.get("runner_implementation_sha256"), str)
        or not DIGEST_PATTERN.fullmatch(value["runner_implementation_sha256"])
        or value["runner_implementation_sha256"]
        != gate_container.runner_implementation_sha256()
        or type(value.get("daemon_memory_bytes")) is not int
        or value["daemon_memory_bytes"] < gate_container.MINIMUM_DAEMON_MEMORY_BYTES
        or not isinstance(value.get("tool_versions"), list)
        or len(value["tool_versions"]) != 6
        or any(not isinstance(item, str) or not item for item in value["tool_versions"])
    ):
        fail("gate_container_runtime_invalid")
    verify_sealed_record(value, "gate_container_runtime")
    return value


def seal_read_only_tree(root):
    paths = sorted(root.rglob("*"), key=lambda path: len(path.parts), reverse=True)
    for path in paths:
        metadata = path.lstat()
        if path.is_symlink():
            fail("gate_bootstrap_symlink_forbidden")
        mode = stat.S_IMODE(metadata.st_mode) & ~0o222
        if stat.S_ISDIR(metadata.st_mode):
            mode |= 0o500
        elif stat.S_ISREG(metadata.st_mode):
            mode |= 0o400
        else:
            fail("gate_bootstrap_entry_invalid")
        path.chmod(mode)
    root.chmod(0o555)


def gate_bootstrap_tree_identity(root):
    require_directory(root, "gate_bootstrap", 0o555)
    digest = hashlib.sha256()
    entries = 0
    total_bytes = 0
    try:
        paths = sorted(root.rglob("*"), key=lambda path: str(path.relative_to(root)))
    except OSError as error:
        fail(f"gate_bootstrap_inventory_unavailable:{error.__class__.__name__}")
    for path in paths:
        relative = str(path.relative_to(root))
        try:
            before = path.lstat()
        except OSError as error:
            fail(f"gate_bootstrap_entry_unavailable:{error.__class__.__name__}")
        mode = stat.S_IMODE(before.st_mode)
        if (
            path.is_symlink()
            or before.st_uid != os.getuid()
            or mode & 0o222
            or "\x00" in relative
        ):
            fail("gate_bootstrap_entry_invalid")
        entries += 1
        if entries > 500000:
            fail("gate_bootstrap_inventory_too_large")
        if stat.S_ISDIR(before.st_mode):
            kind = "directory"
            size = 0
        elif stat.S_ISREG(before.st_mode) and before.st_nlink == 1:
            kind = "file"
            size = before.st_size
        else:
            fail("gate_bootstrap_entry_invalid")
        header = canonical_json(
            {
                "path": relative,
                "kind": kind,
                "mode": mode,
                "size": size,
            }
        ).encode("utf-8")
        digest.update(len(header).to_bytes(8, "big"))
        digest.update(header)
        if kind == "directory":
            continue
        total_bytes += size
        if total_bytes > 4 * 1024 * 1024 * 1024:
            fail("gate_bootstrap_inventory_too_large")
        flags = os.O_RDONLY
        if hasattr(os, "O_NOFOLLOW"):
            flags |= os.O_NOFOLLOW
        try:
            descriptor = os.open(path, flags)
        except OSError as error:
            fail(f"gate_bootstrap_entry_unavailable:{error.__class__.__name__}")
        observed = 0
        try:
            while True:
                chunk = os.read(descriptor, 1024 * 1024)
                if not chunk:
                    break
                observed += len(chunk)
                digest.update(chunk)
            after = os.fstat(descriptor)
        finally:
            os.close(descriptor)
        if (
            observed != size
            or before.st_dev != after.st_dev
            or before.st_ino != after.st_ino
            or before.st_mode != after.st_mode
            or before.st_size != after.st_size
            or before.st_mtime_ns != after.st_mtime_ns
            or before.st_ctime_ns != after.st_ctime_ns
        ):
            fail("gate_bootstrap_changed_during_read")
    return {
        "entries": entries,
        "total_bytes": total_bytes,
        "tree_sha256": digest.hexdigest(),
    }


def ensure_gate_bootstrap_record(root, bootstrap, runtime):
    identity = {
        "schema_version": SCHEMA_VERSION,
        "kind": "starring.d3.gate-bootstrap.v1",
        "gate_runtime_sha256": runtime["record_sha256"],
        **gate_bootstrap_tree_identity(bootstrap),
    }
    path = root / "gate-bootstrap.json"
    if path.exists():
        existing = load_json_file(path, "gate_bootstrap_record", 0o600)
        verify_sealed_record(existing, "gate_bootstrap_record")
        if existing != seal_record(identity):
            fail("gate_bootstrap_changed")
        return existing
    record = seal_record(identity)
    write_new_json(path, record)
    return record


def load_gate_bootstrap_record(root, runtime):
    record = load_json_file(
        root / "gate-bootstrap.json",
        "gate_bootstrap_record",
        0o600,
    )
    required = {
        "schema_version",
        "kind",
        "gate_runtime_sha256",
        "entries",
        "total_bytes",
        "tree_sha256",
        "record_sha256",
    }
    if (
        not isinstance(record, dict)
        or set(record) != required
        or not valid_schema_version(record.get("schema_version"))
        or record.get("kind") != "starring.d3.gate-bootstrap.v1"
        or record.get("gate_runtime_sha256") != runtime["record_sha256"]
        or type(record.get("entries")) is not int
        or record["entries"] <= 0
        or type(record.get("total_bytes")) is not int
        or record["total_bytes"] <= 0
        or not isinstance(record.get("tree_sha256"), str)
        or not DIGEST_PATTERN.fullmatch(record["tree_sha256"])
    ):
        fail("gate_bootstrap_record_invalid")
    verify_sealed_record(record, "gate_bootstrap_record")
    observed = gate_bootstrap_tree_identity(root / "gate-bootstrap")
    if any(record[key] != observed[key] for key in observed):
        fail("gate_bootstrap_changed")
    return record


def candidate_dependency_snapshot(root, runtime=None, bootstrap_record=None):
    selected_runtime = (
        load_gate_container_runtime(root) if runtime is None else runtime
    )
    selected_bootstrap = (
        load_gate_bootstrap_record(root, selected_runtime)
        if bootstrap_record is None
        else bootstrap_record
    )
    bootstrap = root / "gate-bootstrap"
    identity = {
        "schema_version": SCHEMA_VERSION,
        "kind": "starring.d3.candidate-dependency-snapshot.v1",
        "gate_runtime_sha256": selected_runtime["record_sha256"],
        "gate_bootstrap_sha256": selected_bootstrap["record_sha256"],
        "gate_bootstrap_tree_sha256": selected_bootstrap["tree_sha256"],
        "candidate_builder_implementation_sha256": (
            candidate_bundle.candidate_builder_implementation_sha256()
        ),
        "bootstrap_root": str(bootstrap),
        "workspace": {
            "vendor_root": str(bootstrap / "vendor" / "workspace"),
            "cargo_config": candidate_bundle.file_identity(
                bootstrap / "native-cargo-config.toml",
                "candidate_workspace_cargo_config",
                expected_mode=0o400,
            ),
        },
        "transport": {
            "vendor_root": str(bootstrap / "vendor" / "transport"),
            "cargo_config": candidate_bundle.file_identity(
                bootstrap / "native-transport-cargo-config.toml",
                "candidate_transport_cargo_config",
                expected_mode=0o400,
            ),
        },
    }
    try:
        return candidate_bundle.validate_dependency_snapshot(identity)
    except CandidateBundleError as error:
        fail(str(error))


def make_private_writable_tree(root):
    for path in root.rglob("*"):
        metadata = path.lstat()
        if path.is_symlink():
            fail("gate_runtime_symlink_forbidden")
        if stat.S_ISDIR(metadata.st_mode):
            path.chmod(0o700)
        elif stat.S_ISREG(metadata.st_mode):
            path.chmod(0o600)
        else:
            fail("gate_runtime_entry_invalid")
    root.chmod(0o700)


def normalize_vendor_configuration(raw, observed_vendor, selected_vendor):
    if len(raw) > 64 * 1024:
        fail("gate_vendor_configuration_invalid")
    try:
        value = raw.decode("utf-8")
    except UnicodeDecodeError:
        fail("gate_vendor_configuration_invalid")
    observed_vendor_value = str(observed_vendor)
    selected_vendor_value = str(selected_vendor)
    if any(
        character in selected_vendor_value
        for character in ('"', "\\", "\x00", "\n", "\r")
    ):
        fail("gate_vendor_configuration_invalid")
    sections = {}
    current = None
    output = []
    directory_count = 0
    for line in value.splitlines():
        if not line:
            output.append(line)
            continue
        if re.fullmatch(r'\[source\.(?:crates-io|vendored-sources|"[^"]+")\]', line):
            current = line
            if current in sections:
                fail("gate_vendor_configuration_invalid")
            sections[current] = []
            output.append(line)
            continue
        if current is None or not re.fullmatch(
            r'(?:replace-with|directory|git|rev|tag|branch) = "[^"\x00]+"',
            line,
        ):
            fail("gate_vendor_configuration_invalid")
        sections[current].append(line)
        if line.startswith("directory = "):
            directory_count += 1
            if line != f'directory = "{observed_vendor_value}"':
                fail("gate_vendor_configuration_invalid")
            line = f'directory = "{selected_vendor_value}"'
        output.append(line)
    if (
        directory_count != 1
        or "[source.crates-io]" not in sections
        or "[source.vendored-sources]" not in sections
        or sections["[source.vendored-sources]"]
        != [f'directory = "{observed_vendor_value}"']
        or any(
            'replace-with = "vendored-sources"' not in lines
            for section, lines in sections.items()
            if section != "[source.vendored-sources]"
        )
    ):
        fail("gate_vendor_configuration_invalid")
    return "\n".join(output).rstrip() + "\n\n[net]\noffline = true\n"


def composite_vendor_configuration(workspace_vendor, twilight_vendor):
    selected = (str(workspace_vendor), str(twilight_vendor))
    if any(
        not value
        or any(character in value for character in ('"', "\\", "\x00", "\n", "\r"))
        for value in selected
    ):
        fail("gate_vendor_configuration_invalid")
    return (
        '[source.crates-io]\nreplace-with = "workspace-vendored-sources"\n\n'
        f'[source."{gate_container.TWILIGHT_SOURCE_KEY}"]\n'
        f'git = "{gate_container.TWILIGHT_GIT_URL}"\n'
        f'rev = "{gate_container.TWILIGHT_GIT_REV}"\n'
        'replace-with = "twilight-vendored-sources"\n\n'
        '[source.workspace-vendored-sources]\n'
        f'directory = "{selected[0]}"\n\n'
        '[source.twilight-vendored-sources]\n'
        f'directory = "{selected[1]}"\n\n'
        '[net]\noffline = true\n'
    )


def validate_npm_lock_sources(path):
    try:
        raw = path.read_bytes()
    except OSError as error:
        fail(f"gate_npm_lock_unavailable:{error.__class__.__name__}")
    value = load_json_bytes(raw, "gate_npm_lock")
    packages = value.get("packages") if isinstance(value, dict) else None
    if not isinstance(packages, dict):
        fail("gate_npm_lock_invalid")
    for package in packages.values():
        if not isinstance(package, dict):
            fail("gate_npm_lock_invalid")
        resolved = package.get("resolved")
        if resolved is None:
            continue
        try:
            parsed = urllib.parse.urlsplit(resolved)
        except ValueError:
            fail("gate_npm_lock_source_invalid")
        if (
            parsed.scheme != "https"
            or parsed.hostname != "registry.npmjs.org"
            or parsed.username is not None
            or parsed.password is not None
            or parsed.fragment
        ):
            fail("gate_npm_lock_source_invalid")


def gate_git_wrapper(workspace="/workspace", projection="/git"):
    workspace = pathlib.Path(workspace)
    projection = pathlib.Path(projection)
    if not workspace.is_absolute() or not projection.is_absolute():
        fail("gate_git_wrapper_path_invalid")
    workspace_literal = shlex.quote(str(workspace))
    projection_literal = shlex.quote(str(projection))
    return (
        "#!/bin/sh\n"
        'if [ "$1" = "-C" ] && '
        f'[ "$(/usr/bin/readlink -f -- "$2")" = {workspace_literal} ]; then\n'
        "  shift 2\n"
        f"  exec /usr/bin/git --git-dir={projection_literal} "
        f"--work-tree={workspace_literal} \"$@\"\n"
        "fi\n"
        f'if [ "$(/usr/bin/readlink -f -- .)" = {workspace_literal} ]; then\n'
        f"  exec /usr/bin/git --git-dir={projection_literal} "
        f"--work-tree={workspace_literal} \"$@\"\n"
        "fi\n"
        'exec /usr/bin/git "$@"\n'
    )


def prepare_gate_git_projection(staging, worktree):
    projection = staging / "git"
    commands = (
        ["/usr/bin/git", "init", "--bare", str(projection)],
        [
            "/usr/bin/git",
            "--git-dir",
            str(projection),
            "fetch",
            "--no-tags",
            "--no-recurse-submodules",
            str(worktree),
            "HEAD",
        ],
        [
            "/usr/bin/git",
            "--git-dir",
            str(projection),
            "update-ref",
            "refs/heads/candidate",
            "FETCH_HEAD",
        ],
        [
            "/usr/bin/git",
            "--git-dir",
            str(projection),
            "symbolic-ref",
            "HEAD",
            "refs/heads/candidate",
        ],
        [
            "/usr/bin/git",
            "--git-dir",
            str(projection),
            "--work-tree",
            str(worktree),
            "read-tree",
            "HEAD",
        ],
    )
    for index, command in enumerate(commands):
        run_process(
            command,
            worktree,
            f"gate_git_projection_{index}",
            timeout=120,
            discard=True,
        )
    projected = run_process(
        [
            "/usr/bin/git",
            "--git-dir",
            str(projection),
            "--work-tree",
            str(worktree),
            "rev-parse",
            "HEAD",
        ],
        worktree,
        "gate_git_projection_identity",
        timeout=30,
    )[1]
    expected = git(worktree, ["rev-parse", "HEAD"], "gate_git_source_identity")
    if projected != expected:
        fail("gate_git_projection_identity_mismatch")
    status = run_process(
        [
            "/usr/bin/git",
            "--git-dir",
            str(projection),
            "--work-tree",
            str(worktree),
            "status",
            "--porcelain",
            "--untracked-files=normal",
        ],
        worktree,
        "gate_git_projection_status",
        timeout=120,
    )[1]
    if status:
        fail("gate_git_projection_dirty")


def discard_owned_directory(parent, path, label):
    if path.parent != parent:
        fail(f"{label}_path_invalid")
    flags = os.O_RDONLY
    if hasattr(os, "O_DIRECTORY"):
        flags |= os.O_DIRECTORY
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        before = path.lstat()
        descriptor = os.open(path, flags)
    except OSError as error:
        fail(f"{label}_unavailable:{error.__class__.__name__}")
    try:
        observed = os.fstat(descriptor)
        if (
            not stat.S_ISDIR(before.st_mode)
            or path.is_symlink()
            or before.st_uid != os.getuid()
            or (before.st_dev, before.st_ino)
            != (observed.st_dev, observed.st_ino)
        ):
            fail(f"{label}_identity_invalid")
        os.fchmod(descriptor, 0o700)
        candidate_io.remove_tree_descriptor(descriptor)
    except candidate_io.CandidateBundleError as error:
        fail(str(error))
    finally:
        os.close(descriptor)
    try:
        path.rmdir()
    except OSError as error:
        fail(f"{label}_cleanup_failed:{error.__class__.__name__}")
    fsync_directory(parent)


def discard_gate_bootstrap_staging(root, staging):
    if not os.path.lexists(staging):
        return
    if (
        staging.parent != root
        or staging.name != ".gate-bootstrap-staging"
        and re.fullmatch(r"\.gate-bootstrap-staging-[0-9a-f]{16}", staging.name)
        is None
    ):
        fail("gate_bootstrap_staging_path_invalid")
    discard_owned_directory(root, staging, "gate_bootstrap_staging")


def require_gate_bootstrap_layout(bootstrap):
    require_directory(bootstrap, "gate_bootstrap", 0o555)
    expected = {*GATE_BOOTSTRAP_DIRECTORIES, *GATE_BOOTSTRAP_FILES}
    try:
        inventory = {path.name for path in bootstrap.iterdir()}
    except OSError as error:
        fail(f"gate_bootstrap_inventory_unavailable:{error.__class__.__name__}")
    if inventory != expected:
        fail("gate_bootstrap_inventory_invalid")
    for name in GATE_BOOTSTRAP_DIRECTORIES:
        metadata = require_directory(bootstrap / name, f"gate_bootstrap_{name}")
        if stat.S_IMODE(metadata.st_mode) & 0o222:
            fail(f"gate_bootstrap_{name}_mode_invalid")
    for name in GATE_BOOTSTRAP_FILES:
        require_regular(bootstrap / name, f"gate_bootstrap_{name}", 0o400)
    expected_nested = {
        "bin": {"git", "promptfoo"},
        "node-stage": {"node_modules"},
        "vendor": {"issuer", "transport", "workspace"},
    }
    for name, expected_inventory in expected_nested.items():
        try:
            observed = {path.name for path in (bootstrap / name).iterdir()}
        except OSError as error:
            fail(
                f"gate_bootstrap_{name}_inventory_unavailable:"
                f"{error.__class__.__name__}"
            )
        if observed != expected_inventory:
            fail(f"gate_bootstrap_{name}_inventory_invalid")
    return bootstrap


def discard_gate_bootstrap_temporary_file(staging, relative, mode):
    path = staging / relative
    if staging not in path.parents or ".." in pathlib.PurePath(relative).parts:
        fail("gate_bootstrap_temporary_path_invalid")
    try:
        candidate_io.unlink_owned_regular(path, mode)
    except candidate_io.CandidateBundleError as error:
        fail(str(error))


def prepare_gate_bootstrap(root, worktree, runtime):
    bootstrap = root / "gate-bootstrap"
    if os.path.lexists(bootstrap):
        return require_gate_bootstrap_layout(bootstrap)
    try:
        gate_container.require_bootstrap_start_capacity(root)
    except gate_container.GateContainerError as error:
        fail(str(error))
    staging = root / ".gate-bootstrap-staging"
    try:
        stale = sorted(
            path
            for path in root.iterdir()
            if path.name == ".gate-bootstrap-staging"
            or re.fullmatch(
                r"\.gate-bootstrap-staging-[0-9a-f]{16}",
                path.name,
            )
        )
    except OSError as error:
        fail(f"gate_bootstrap_staging_inventory_unavailable:{error.__class__.__name__}")
    if len(stale) > 32:
        fail("gate_bootstrap_staging_inventory_invalid")
    for path in stale:
        discard_gate_bootstrap_staging(root, path)
    staging.mkdir(mode=0o700)
    fsync_directory(root)
    try:
        return build_gate_bootstrap(
            root,
            worktree,
            runtime,
            bootstrap,
            staging,
        )
    except BaseException:
        discard_gate_bootstrap_staging(root, staging)
        raise


def build_gate_bootstrap(root, worktree, runtime, bootstrap, staging):
    for name in GATE_BOOTSTRAP_WORK_DIRECTORIES:
        ensure_private_directory(staging / name)
    ensure_private_directory(staging / "npm-cache")
    node_stage = ensure_private_directory(staging / "node-stage")
    bin_directory = ensure_private_directory(staging / "bin")
    prepare_gate_git_projection(staging, worktree)
    try:
        gate_container.validate_cargo_lock_sources(
            root,
            worktree,
            runtime["image_id"],
        )
        gate_container.fetch_cargo_vendor(
            root,
            worktree,
            staging,
            runtime["image_id"],
        )
        gate_container.materialize_workspace_vendor(
            root,
            worktree,
            staging,
            runtime["image_id"],
        )
        transport_vendor_raw = read_owned_bytes(
            staging / "transport-cargo-vendor-config.txt",
            "gate_transport_vendor_configuration",
            0o600,
            64 * 1024,
        )
        issuer_vendor_raw = read_owned_bytes(
            staging / "issuer-cargo-vendor-config.txt",
            "gate_issuer_vendor_configuration",
            0o600,
            64 * 1024,
        )
        workspace_staging_config = composite_vendor_configuration(
            pathlib.Path("/stage/vendor/workspace"),
            pathlib.Path("/stage/vendor/transport"),
        )
        transport_staging_config = normalize_vendor_configuration(
            transport_vendor_raw,
            pathlib.Path("/stage/vendor/transport"),
            pathlib.Path("/stage/vendor/transport"),
        )
        issuer_staging_config = normalize_vendor_configuration(
            issuer_vendor_raw,
            pathlib.Path("/stage/vendor/issuer"),
            pathlib.Path("/stage/vendor/issuer"),
        )
        write_new_file(
            staging / "workspace-cargo-config.toml",
            workspace_staging_config,
        )
        write_new_file(
            staging / "transport-staging-cargo-config.toml",
            transport_staging_config,
        )
        write_new_file(
            staging / "issuer-staging-cargo-config.toml",
            issuer_staging_config,
        )
        gate_container.verify_cargo_vendor(
            root,
            worktree,
            staging,
            runtime["image_id"],
        )
        for name in ("package.json", "package-lock.json"):
            source = worktree / "eval" / "design-harness" / name
            destination = node_stage / name
            shutil.copyfile(source, destination)
            destination.chmod(0o400)
        validate_npm_lock_sources(node_stage / "package-lock.json")
        try:
            gate_container.install_node_dependencies(
                root,
                staging,
                runtime["image_id"],
            )
        except gate_container.GateContainerError as error:
            fail(str(error))
    except gate_container.GateContainerError as error:
        fail(str(error))
    cargo_config = composite_vendor_configuration(
        pathlib.Path("/vendor/workspace"),
        pathlib.Path("/vendor/transport"),
    )
    write_new_file(staging / "cargo-config.toml", cargo_config)
    transport_cargo_config = normalize_vendor_configuration(
        transport_vendor_raw,
        pathlib.Path("/stage/vendor/transport"),
        pathlib.Path("/vendor/transport"),
    )
    write_new_file(
        staging / "transport-cargo-config.toml",
        transport_cargo_config,
    )
    issuer_cargo_config = normalize_vendor_configuration(
        issuer_vendor_raw,
        pathlib.Path("/stage/vendor/issuer"),
        pathlib.Path("/vendor/issuer"),
    )
    write_new_file(
        staging / "issuer-cargo-config.toml",
        issuer_cargo_config,
    )
    native_cargo_config = composite_vendor_configuration(
        bootstrap / "vendor" / "workspace",
        bootstrap / "vendor" / "transport",
    )
    write_new_file(staging / "native-cargo-config.toml", native_cargo_config)
    native_transport_cargo_config = normalize_vendor_configuration(
        transport_vendor_raw,
        pathlib.Path("/stage/vendor/transport"),
        bootstrap / "vendor" / "transport",
    )
    write_new_file(
        staging / "native-transport-cargo-config.toml",
        native_transport_cargo_config,
    )
    promptfoo = (
        "#!/bin/sh\n"
        'exec /usr/local/bin/node '
        '/node_modules/promptfoo/dist/src/entrypoint.js "$@"\n'
    )
    promptfoo_path = bin_directory / "promptfoo"
    write_new_file(promptfoo_path, promptfoo)
    promptfoo_path.chmod(0o555)
    git_path = bin_directory / "git"
    write_new_file(git_path, gate_git_wrapper())
    git_path.chmod(0o555)
    for name in GATE_BOOTSTRAP_WORK_DIRECTORIES:
        discard_owned_directory(
            staging,
            staging / name,
            f"gate_bootstrap_work_{name}",
        )
    for relative, mode in GATE_BOOTSTRAP_TEMPORARY_PATHS:
        discard_gate_bootstrap_temporary_file(staging, relative, mode)
    seal_read_only_tree(staging)
    try:
        staging.rename(bootstrap)
    except OSError as error:
        fail(f"gate_bootstrap_publish_failed:{error.__class__.__name__}")
    fsync_directory(root)
    return require_gate_bootstrap_layout(bootstrap)


def initialize_gate_run(root, index, attempt, bootstrap, worktree):
    runs = ensure_private_directory(root / "gate-runs")
    path = runs / f"gate-{index:02d}-attempt-{attempt}-{secrets.token_hex(8)}"
    path.mkdir(mode=0o700)
    fsync_directory(runs)
    cwd = worktree
    if index in (10, 11):
        projection = ensure_private_directory(path / "worktree")
        package = ensure_private_directory(
            ensure_private_directory(projection / "eval") / "design-harness"
        )
        for name in ("package.json", "package-lock.json"):
            shutil.copyfile(
                worktree / "eval" / "design-harness" / name,
                package / name,
            )
            (package / name).chmod(0o400)
        cwd = projection
    return path, cwd


def remove_gate_run(root, path):
    runs = root / "gate-runs"
    require_directory(runs, "gate_runs", 0o700)
    if path.parent != runs or not path.name.startswith("gate-"):
        fail("gate_run_path_invalid")
    flags = os.O_RDONLY
    if hasattr(os, "O_DIRECTORY"):
        flags |= os.O_DIRECTORY
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        fail(f"gate_run_cleanup_unavailable:{error.__class__.__name__}")
    try:
        metadata = os.fstat(descriptor)
        if (
            not stat.S_ISDIR(metadata.st_mode)
            or metadata.st_uid != os.getuid()
            or stat.S_IMODE(metadata.st_mode) != 0o700
        ):
            fail("gate_run_cleanup_identity_invalid")
        candidate_io.remove_tree_descriptor(descriptor)
    except candidate_io.CandidateBundleError as error:
        fail(str(error))
    finally:
        os.close(descriptor)
    try:
        path.rmdir()
    except OSError as error:
        fail(f"gate_run_cleanup_failed:{error.__class__.__name__}")
    fsync_directory(runs)


def run_sandboxed_gate(
    root,
    worktree,
    bootstrap,
    runtime,
    index,
    attempt,
    command,
    timeout,
    database_url,
):
    run_root, cwd = initialize_gate_run(
        root, index, attempt, bootstrap, worktree
    )
    try:
        try:
            return_code = gate_container.run_gate(
                root,
                cwd,
                bootstrap,
                runtime,
                index,
                attempt,
                command,
                timeout,
                database_url,
            )
        except gate_container.GateContainerError as error:
            fail(str(error))
        return return_code
    finally:
        remove_gate_run(root, run_root)


def command_run_gates(arguments):
    digests = gate_digests(arguments.gate)
    state_argument, root = state_lock_target(arguments.state)
    with StateLock(root):
        state_path, state, _, repo, worktree = load_state(str(state_argument))
        if digests != state["gate_command_sha256"]:
            fail("gate_plan_mismatch")
        postgres_database_url = load_postgres_database_url(
            arguments.postgres_database_url_file
        )
        validate_worktree(state, repo, worktree)
        bootstrap_path = root / "gate-bootstrap"
        if os.path.lexists(bootstrap_path):
            require_gate_bootstrap_layout(bootstrap_path)
        else:
            try:
                gate_container.require_bootstrap_start_capacity(root)
            except gate_container.GateContainerError as error:
                fail(str(error))
        try:
            candidate_bundle.require_cargo_configuration_absent(worktree)
            runtime = ensure_gate_container_runtime(root)
            bootstrap = prepare_gate_bootstrap(root, worktree, runtime)
            bootstrap_record = ensure_gate_bootstrap_record(
                root,
                bootstrap,
                runtime,
            )
            dependency_snapshot = candidate_dependency_snapshot(
                root,
                runtime,
                bootstrap_record,
            )
        except CandidateBundleError as error:
            fail(str(error))
        evidence_path = root / "gate-evidence.jsonl"
        records = load_gate_evidence(
            evidence_path,
            state,
            runtime["record_sha256"],
            bootstrap_record["record_sha256"],
        )
        attempts, latest, open_attempts = gate_status(records, len(digests))
        for index, command in enumerate(arguments.gate, start=1):
            prior = latest.get(index)
            if prior is not None and prior["exit_code"] == 0:
                continue
            unfinished = [key for key in open_attempts if key[0] == index]
            if unfinished:
                attempt = max(key[1] for key in unfinished)
            else:
                attempt = attempts[index] + 1
                append_gate_evidence(
                    evidence_path,
                    state,
                    records,
                    {
                        "kind": "starring.d3.gate-started.v1",
                        "gate_index": index,
                        "command_sha256": digests[index - 1],
                        "attempt": attempt,
                    },
                    runtime["record_sha256"],
                    bootstrap_record["record_sha256"],
                )
            started = time.monotonic_ns()
            try:
                return_code = run_sandboxed_gate(
                    root,
                    worktree,
                    bootstrap,
                    runtime,
                    index,
                    attempt,
                    command,
                    arguments.timeout_seconds,
                    postgres_database_url,
                )
            except (D3Error, CandidateBundleError):
                return_code = 255
            duration_ms = max(0, (time.monotonic_ns() - started) // 1_000_000)
            completion = append_gate_evidence(
                evidence_path,
                state,
                records,
                {
                    "kind": "starring.d3.gate-completed.v1",
                    "gate_index": index,
                    "command_sha256": digests[index - 1],
                    "attempt": attempt,
                    "exit_code": return_code,
                    "duration_ms": duration_ms,
                },
                runtime["record_sha256"],
                bootstrap_record["record_sha256"],
            )
            validate_worktree(state, repo, worktree)
            if return_code != 0:
                fail(f"gate_failed:{index}:{return_code}")
            latest[index] = completion
        if ensure_gate_container_runtime(root) != runtime:
            fail("gate_container_runtime_changed")
        records = load_gate_evidence(
            evidence_path,
            state,
            runtime["record_sha256"],
            bootstrap_record["record_sha256"],
        )
        _, latest, open_attempts = gate_status(records, len(digests))
        if open_attempts or any(index not in latest or latest[index]["exit_code"] != 0 for index in range(1, len(digests) + 1)):
            fail("gate_set_incomplete")
        gate_chain = records[-1]["record_sha256"]

        def revalidate_candidate_inputs():
            observed_path, observed_state, _, observed_repo, observed_worktree = load_state(
                str(state_path)
            )
            if (
                observed_path != state_path
                or observed_state != state
                or observed_repo != repo
                or observed_worktree != worktree
                or require_gate_completion(root, state) != gate_chain
                or candidate_dependency_snapshot(root) != dependency_snapshot
            ):
                fail("candidate_build_state_changed")

        try:
            bundle, bundle_disposition = ensure_candidate_bundle(
                state_path,
                state,
                worktree,
                gate_chain,
                dependency_snapshot,
                revalidate_candidate_inputs,
            )
        except CandidateBundleError as error:
            fail(str(error))
        return {
            "merge_commit": state["merge_commit"],
            "merge_tree": state["merge_tree"],
            "gates": len(digests),
            "status": "passed",
            "evidence_chain_head_sha256": gate_chain,
            "candidate_bundle": str(root / "candidate-bundle" / "bundle.json"),
            "candidate_bundle_sha256": bundle["record_sha256"],
            "candidate_bundle_disposition": bundle_disposition,
        }


def load_d2_receipts(path, manifest, manifest_digest):
    raw_receipts = read_owned_bytes(
        path, "d2_receipts", 0o600, MAX_JSON_BYTES * 17, allow_empty=True
    )
    receipts = []
    previous = ZERO_DIGEST
    total = 0
    for raw in raw_receipts.splitlines():
        total += len(raw)
        if total > MAX_JSON_BYTES * 17:
            fail("d2_receipts_too_large")
        receipt = load_json_bytes(raw, "d2_receipt")
        fields = {
            "schema_version",
            "run_id",
            "manifest_sha256",
            "step",
            "code",
            "observed_at",
            "previous_sha256",
            "receipt_sha256",
            "evidence",
        }
        expected_step = len(receipts) + 1
        if (
            not isinstance(receipt, dict)
            or set(receipt) != fields
            or not valid_schema_version(receipt["schema_version"])
            or receipt["run_id"] != manifest.get("run_id")
            or receipt["manifest_sha256"] != manifest_digest
            or type(receipt["step"]) is not int
            or receipt["step"] != expected_step
            or expected_step > 17
            or receipt["code"] != D2_STEP_CODES[expected_step - 1]
            or receipt["previous_sha256"] != previous
            or not isinstance(receipt["evidence"], dict)
        ):
            fail("d2_receipt_sequence_invalid")
        validate_timestamp(receipt["observed_at"], "d2_receipt_observed_at")
        digest = validate_digest(receipt["receipt_sha256"], "d2_receipt_sha256")
        payload = dict(receipt)
        payload.pop("receipt_sha256")
        if sha256_bytes(canonical_json(payload).encode("utf-8")) != digest:
            fail("d2_receipt_chain_invalid")
        previous = digest
        receipts.append(receipt)
    if len(receipts) != 17:
        fail(f"d2_certification_incomplete:{len(receipts)}_of_17")
    return receipts


def d2_metadata_identity(metadata):
    return (
        metadata.st_dev,
        metadata.st_ino,
        metadata.st_mode,
        metadata.st_uid,
        metadata.st_nlink,
        metadata.st_size,
        metadata.st_mtime_ns,
        metadata.st_ctime_ns,
    )


def d2_run_tree_identity(root):
    digest = hashlib.sha256()
    entries = 0
    total_bytes = 0
    try:
        root_before = root.lstat()
    except OSError as error:
        fail(f"d2_run_tree_unavailable:{error.__class__.__name__}")
    if (
        not stat.S_ISDIR(root_before.st_mode)
        or root.is_symlink()
        or root_before.st_uid != os.getuid()
        or stat.S_IMODE(root_before.st_mode) & 0o022
    ):
        fail("d2_run_tree_entry_invalid")
    paths = []
    try:
        for path in root.rglob("*"):
            paths.append(path)
            if len(paths) > D2_RUN_MAX_ENTRIES:
                fail("d2_run_tree_too_large")
    except OSError as error:
        fail(f"d2_run_tree_unavailable:{error.__class__.__name__}")
    paths.sort(key=lambda path: str(path.relative_to(root)))
    metadata_fence = []
    for path in paths:
        relative = str(path.relative_to(root))
        try:
            relative.encode("utf-8")
        except UnicodeEncodeError:
            fail("d2_run_tree_entry_invalid")
        try:
            before = path.lstat()
        except OSError as error:
            fail(f"d2_run_tree_entry_unavailable:{error.__class__.__name__}")
        mode = stat.S_IMODE(before.st_mode)
        if (
            path.is_symlink()
            or before.st_uid != os.getuid()
            or mode & 0o022
            or not relative
            or "\x00" in relative
        ):
            fail("d2_run_tree_entry_invalid")
        entries += 1
        if entries > D2_RUN_MAX_ENTRIES:
            fail("d2_run_tree_too_large")
        if stat.S_ISDIR(before.st_mode):
            kind = "directory"
            size = 0
        elif stat.S_ISREG(before.st_mode):
            if before.st_nlink != 1:
                fail("d2_run_tree_entry_invalid")
            kind = "file"
            size = before.st_size
        else:
            fail("d2_run_tree_entry_invalid")
        if total_bytes + size > D2_RUN_MAX_BYTES:
            fail("d2_run_tree_too_large")
        header = canonical_json(
            {
                "path": relative,
                "kind": kind,
                "mode": mode,
                "uid": before.st_uid,
                "device": before.st_dev,
                "inode": before.st_ino,
                "size": size,
                "mtime_ns": before.st_mtime_ns,
                "ctime_ns": before.st_ctime_ns,
            }
        ).encode("utf-8")
        digest.update(len(header).to_bytes(8, "big"))
        digest.update(header)
        flags = os.O_RDONLY
        if kind == "directory" and hasattr(os, "O_DIRECTORY"):
            flags |= os.O_DIRECTORY
        if hasattr(os, "O_NOFOLLOW"):
            flags |= os.O_NOFOLLOW
        try:
            descriptor = os.open(path, flags)
        except OSError as error:
            fail(f"d2_run_tree_entry_unavailable:{error.__class__.__name__}")
        observed = 0
        try:
            if kind == "file":
                remaining = size
                while remaining:
                    chunk = os.read(descriptor, min(1024 * 1024, remaining))
                    if not chunk:
                        break
                    observed += len(chunk)
                    remaining -= len(chunk)
                    digest.update(chunk)
                if os.read(descriptor, 1):
                    fail("d2_run_tree_changed_during_read")
            after = os.fstat(descriptor)
        finally:
            os.close(descriptor)
        if (
            observed != size
            or d2_metadata_identity(before) != d2_metadata_identity(after)
        ):
            fail("d2_run_tree_changed_during_read")
        metadata_fence.append((path, d2_metadata_identity(after)))
        total_bytes += size
    for path, identity in metadata_fence:
        try:
            final = path.lstat()
        except OSError:
            fail("d2_run_tree_changed_during_read")
        if d2_metadata_identity(final) != identity:
            fail("d2_run_tree_changed_during_read")
    try:
        root_after = root.lstat()
    except OSError:
        fail("d2_run_tree_changed_during_read")
    if d2_metadata_identity(root_before) != d2_metadata_identity(root_after):
        fail("d2_run_tree_changed_during_read")
    return {
        "d2_run_entries": entries,
        "d2_run_total_bytes": total_bytes,
        "d2_run_tree_sha256": digest.hexdigest(),
    }


def capture_d2_run_identity(manifest_path, run_id, manifest_digest):
    manifest_path = absolute_path(manifest_path, "d2_manifest")
    if manifest_path.name != "manifest.json":
        fail("d2_manifest_path_invalid")
    run_directory = manifest_path.parent
    taint_path = run_directory / D2A_TAINT_NAME
    if os.path.lexists(taint_path):
        fail("d2a_run_not_release_eligible")
    directory_before = require_directory(
        run_directory, "d2_run_directory", 0o700
    )
    orchestrator_directory = run_directory / "orchestrator"
    orchestrator_before = require_directory(
        orchestrator_directory, "d2_orchestrator_directory", 0o700
    )
    manifest_before = require_regular(
        manifest_path, "d2_manifest", 0o600
    )
    retirement_path = (
        orchestrator_directory / "candidate-start-retirement.json"
    )
    abort_teardown_path = (
        orchestrator_directory / "discord-resource-teardown-abort.json"
    )
    if os.path.lexists(retirement_path) or os.path.lexists(
        abort_teardown_path
    ):
        fail("candidate_start_transition_retirement_required")
    tree_before = d2_run_tree_identity(run_directory)
    manifest = load_json_file(manifest_path, "d2_manifest", 0o600)
    if (
        not isinstance(manifest, dict)
        or manifest.get("run_id") != run_id
        or sha256_bytes(canonical_json(manifest).encode("utf-8"))
        != manifest_digest
    ):
        fail("d2_manifest_binding_drift")
    manifest_after = require_regular(manifest_path, "d2_manifest", 0o600)
    directory_after = require_directory(
        run_directory, "d2_run_directory", 0o700
    )
    orchestrator_after = require_directory(
        orchestrator_directory, "d2_orchestrator_directory", 0o700
    )
    tree_after = d2_run_tree_identity(run_directory)
    manifest_final = require_regular(manifest_path, "d2_manifest", 0o600)
    directory_final = require_directory(
        run_directory, "d2_run_directory", 0o700
    )
    orchestrator_final = require_directory(
        orchestrator_directory, "d2_orchestrator_directory", 0o700
    )
    tree_final = d2_run_tree_identity(run_directory)
    manifest_sealed = require_regular(manifest_path, "d2_manifest", 0o600)
    directory_sealed = require_directory(
        run_directory, "d2_run_directory", 0o700
    )
    orchestrator_sealed = require_directory(
        orchestrator_directory, "d2_orchestrator_directory", 0o700
    )
    if (
        d2_metadata_identity(manifest_before)
        != d2_metadata_identity(manifest_after)
        or d2_metadata_identity(manifest_after)
        != d2_metadata_identity(manifest_final)
        or d2_metadata_identity(manifest_final)
        != d2_metadata_identity(manifest_sealed)
        or d2_metadata_identity(directory_before)
        != d2_metadata_identity(directory_after)
        or d2_metadata_identity(directory_after)
        != d2_metadata_identity(directory_final)
        or d2_metadata_identity(directory_final)
        != d2_metadata_identity(directory_sealed)
        or d2_metadata_identity(orchestrator_before)
        != d2_metadata_identity(orchestrator_after)
        or d2_metadata_identity(orchestrator_after)
        != d2_metadata_identity(orchestrator_final)
        or d2_metadata_identity(orchestrator_final)
        != d2_metadata_identity(orchestrator_sealed)
        or tree_before != tree_after
        or tree_after != tree_final
        or os.path.lexists(taint_path)
        or os.path.lexists(retirement_path)
        or os.path.lexists(abort_teardown_path)
    ):
        fail("d2_run_identity_changed")
    return {
        "d2_manifest_path": str(manifest_path),
        "d2_run_directory_device": directory_sealed.st_dev,
        "d2_run_directory_inode": directory_sealed.st_ino,
        "d2_orchestrator_directory_device": orchestrator_sealed.st_dev,
        "d2_orchestrator_directory_inode": orchestrator_sealed.st_ino,
        **tree_final,
    }


def require_active_d2_binding(binding):
    raw_path = binding.get("d2_manifest_path")
    if not isinstance(raw_path, str):
        fail("d2_binding_run_identity_invalid")
    path = absolute_path(raw_path, "d2_manifest")
    if (
        type(binding.get("d2_run_directory_device")) is not int
        or binding["d2_run_directory_device"] < 0
        or type(binding.get("d2_run_directory_inode")) is not int
        or binding["d2_run_directory_inode"] <= 0
        or type(binding.get("d2_orchestrator_directory_device")) is not int
        or binding["d2_orchestrator_directory_device"] < 0
        or type(binding.get("d2_orchestrator_directory_inode")) is not int
        or binding["d2_orchestrator_directory_inode"] <= 0
        or type(binding.get("d2_run_entries")) is not int
        or binding["d2_run_entries"] <= 0
        or binding["d2_run_entries"] > D2_RUN_MAX_ENTRIES
        or type(binding.get("d2_run_total_bytes")) is not int
        or binding["d2_run_total_bytes"] <= 0
        or binding["d2_run_total_bytes"] > D2_RUN_MAX_BYTES
        or not isinstance(binding.get("d2_run_tree_sha256"), str)
        or not DIGEST_PATTERN.fullmatch(binding["d2_run_tree_sha256"])
    ):
        fail("d2_binding_run_identity_invalid")
    observed = capture_d2_run_identity(
        path,
        binding.get("run_id"),
        binding.get("manifest_sha256"),
    )
    if any(binding.get(key) != value for key, value in observed.items()):
        fail("d2_binding_run_identity_changed")
    return observed


def command_bind_d2(arguments):
    d2_manifest_path = absolute_path(arguments.d2_manifest, "d2_manifest")
    d2_final_path = absolute_path(arguments.d2_final_record, "d2_final_record")
    state_argument, root = state_lock_target(arguments.state)
    with StateLock(root):
        state_path, state, _, repo, worktree = load_state(str(state_argument))
        gate_chain = require_gate_completion(root, state)
        try:
            dependency_snapshot = candidate_dependency_snapshot(root)
            bundle_record = load_candidate_bundle(
                root,
                state,
                gate_chain,
                dependency_snapshot,
            )
        except CandidateBundleError as error:
            fail(str(error))
        d2_manifest = load_json_file(d2_manifest_path, "d2_manifest", 0o600)
        if not isinstance(d2_manifest, dict):
            fail("d2_manifest_invalid")
        if d2_manifest.get("certification_class") != D2_CERTIFICATION_CLASS:
            fail("d2_certification_class_invalid")
        if d2_manifest.get("human_boundaries") != list(D2_HUMAN_BOUNDARIES):
            fail("d2_human_boundaries_invalid")
        manifest_digest_path = d2_manifest_path.with_name("manifest.sha256")
        manifest_digest = load_small_ascii(manifest_digest_path, "d2_manifest_digest")
        validate_digest(manifest_digest, "d2_manifest_digest")
        observed_manifest_digest = sha256_bytes(canonical_json(d2_manifest).encode("utf-8"))
        if manifest_digest != observed_manifest_digest:
            fail("d2_manifest_digest_mismatch")
        d2_run_identity = capture_d2_run_identity(
            d2_manifest_path,
            d2_manifest.get("run_id"),
            manifest_digest,
        )
        if d2_manifest.get("commit_sha") != state["merge_commit"]:
            fail("d2_commit_mismatch")
        if commit_tree(repo, d2_manifest["commit_sha"], "d2_commit") != state["merge_tree"]:
            fail("d2_tree_mismatch")
        try:
            candidate_bundle_file = validate_d2_manifest_binding(
                d2_manifest,
                state,
                worktree,
                root,
                bundle_record,
            )
        except CandidateBundleError as error:
            fail(str(error))
        receipts = load_d2_receipts(d2_manifest_path.with_name("receipts.jsonl"), d2_manifest, manifest_digest)
        final_record = load_json_file(d2_final_path, "d2_final_record", 0o600)
        expected_final_fields = {
            "schema_version",
            "kind",
            "run_id",
            "commit_sha",
            "manifest_sha256",
            "steps",
            "status",
            "resource_prefix",
            "receipt_chain_head_sha256",
            "coordinator_evidence_sha256",
        }
        if not isinstance(final_record, dict) or set(final_record) != expected_final_fields:
            fail("d2_final_record_fields_invalid")
        expected_final = {
            "schema_version": 1,
            "kind": "starring.d2.coordinator-final-record.v1",
            "run_id": d2_manifest.get("run_id"),
            "commit_sha": state["merge_commit"],
            "manifest_sha256": manifest_digest,
            "steps": 17,
            "status": "passed",
            "resource_prefix": d2_manifest.get("discord", {}).get("resource_prefix"),
            "receipt_chain_head_sha256": receipts[-1]["receipt_sha256"],
            "coordinator_evidence_sha256": final_record.get(
                "coordinator_evidence_sha256"
            ),
        }
        validate_digest(
            final_record.get("coordinator_evidence_sha256"),
            "d2_coordinator_evidence_sha256",
        )
        if (
            not valid_schema_version(final_record["schema_version"])
            or final_record != expected_final
        ):
            fail("d2_final_record_mismatch")
        verifier = worktree / "tools" / "d2-certification" / "d2_run.py"
        require_regular(verifier, "d2_verifier")
        _, raw_verified = run_process(
            [sys.executable, str(verifier), "verify", "--manifest", str(d2_manifest_path)],
            worktree,
            "d2_semantic_verify",
        )
        semantic = load_json_bytes(raw_verified.strip(), "d2_semantic_verify")
        if semantic != final_record:
            fail("d2_semantic_verify_mismatch")
        binding_path = state_path.parent / "d2-binding.json"
        identity = {
            "schema_version": SCHEMA_VERSION,
            "kind": "starring.d3.d2-binding.v1",
            "merge_commit": state["merge_commit"],
            "merge_tree": state["merge_tree"],
            "gate_evidence_chain_head_sha256": gate_chain,
            "candidate_bundle_path": str(
                root / "candidate-bundle" / "bundle.json"
            ),
            "candidate_bundle_record_sha256": bundle_record["record_sha256"],
            "candidate_bundle_file_sha256": candidate_bundle_file["sha256"],
            "run_id": final_record["run_id"],
            "manifest_sha256": manifest_digest,
            **d2_run_identity,
            "steps": 17,
            "receipt_chain_head_sha256": final_record["receipt_chain_head_sha256"],
            "coordinator_evidence_sha256": final_record[
                "coordinator_evidence_sha256"
            ],
            "final_record_sha256": sha256_bytes(canonical_json(final_record).encode("utf-8")),
        }
        if binding_path.exists():
            existing = load_json_file(binding_path, "d2_binding", 0o600)
            verify_sealed_record(existing, "d2_binding")
            if set(existing) != set(identity) | {"verified_at", "record_sha256"}:
                fail("d2_binding_fields_invalid")
            validate_timestamp(existing["verified_at"], "d2_binding_verified_at")
            if {key: existing.get(key) for key in identity} != identity:
                fail("d2_binding_replay_mismatch")
            require_active_d2_binding(existing)
            return {**existing, "disposition": "exact_replay"}
        require_active_d2_binding(identity)
        binding = seal_record({**identity, "verified_at": utc_now()})
        write_new_json(binding_path, binding)
        require_active_d2_binding(binding)
        return {**binding, "disposition": "created"}


def command_recheck(arguments):
    state_argument, root = state_lock_target(arguments.state)
    with StateLock(root):
        state_path, state, _, repo, _ = load_state(str(state_argument))
        gate_chain = require_gate_completion(state_path.parent, state)
        binding = load_binding(state_path.parent, state)
        try:
            dependency_snapshot = candidate_dependency_snapshot(state_path.parent)
            bundle_record = load_candidate_bundle(
                state_path.parent,
                state,
                gate_chain,
                dependency_snapshot,
            )
            candidate_bundle_file = record_file_identity(state_path.parent)
        except CandidateBundleError as error:
            fail(str(error))
        if (
            binding["gate_evidence_chain_head_sha256"] != gate_chain
            or binding["candidate_bundle_path"]
            != str(state_path.parent / "candidate-bundle" / "bundle.json")
            or binding["candidate_bundle_record_sha256"]
            != bundle_record["record_sha256"]
            or binding["candidate_bundle_file_sha256"]
            != candidate_bundle_file["sha256"]
        ):
            fail("candidate_bundle_binding_mismatch")
        if github_repository_from_remote(repo, state["remote"]) != state["github_repository"]:
            fail("state_github_repository_mismatch")
        head, base, merge = fetch_candidate(
            repo, state["remote"], state["base_ref"], state["pr_number"], "recheck"
        )
        if head != state["expected_head"]:
            fail("pr_head_changed")
        if base != state["expected_base"]:
            fail("pr_base_changed")
        if merge != state["merge_commit"]:
            fail("pr_merge_candidate_changed")
        if commit_parents(repo, merge, "recheck_merge") != state["merge_parents"]:
            fail("pr_merge_parents_changed")
        if commit_tree(repo, merge, "recheck_merge") != state["merge_tree"]:
            fail("pr_merge_tree_changed")
        pull_request = github_pull_recheck(state["github_repository"], state)
        require_active_d2_binding(binding)
        record_path = state_path.parent / "recheck.json"
        identity = {
            "schema_version": SCHEMA_VERSION,
            "kind": "starring.d3.pr-recheck.v1",
            "pr_number": state["pr_number"],
            "head_commit": head,
            "base_commit": base,
            "merge_commit": merge,
            "merge_tree": state["merge_tree"],
            "pull_request": pull_request,
            "gate_evidence_chain_head_sha256": gate_chain,
            "candidate_bundle_record_sha256": bundle_record["record_sha256"],
            "candidate_bundle_file_sha256": candidate_bundle_file["sha256"],
            "d2_receipt_chain_head_sha256": binding["receipt_chain_head_sha256"],
            "d2_coordinator_evidence_sha256": binding[
                "coordinator_evidence_sha256"
            ],
        }
        if record_path.exists():
            existing = load_json_file(record_path, "recheck", 0o600)
            verify_sealed_record(existing, "recheck")
            if set(existing) != set(identity) | {"verified_at", "record_sha256"}:
                fail("recheck_fields_invalid")
            validate_timestamp(existing["verified_at"], "recheck_verified_at")
            if {key: existing.get(key) for key in identity} != identity:
                fail("recheck_replay_mismatch")
            require_active_d2_binding(binding)
            return {**existing, "disposition": "exact_replay"}
        record = seal_record({**identity, "verified_at": utc_now()})
        write_new_json(record_path, record)
        require_active_d2_binding(binding)
        return {**record, "disposition": "created"}


def require_gate_completion(root, state):
    path = root / "gate-evidence.jsonl"
    runtime = load_gate_container_runtime(root)
    bootstrap = load_gate_bootstrap_record(root, runtime)
    records = load_gate_evidence(
        path,
        state,
        runtime["record_sha256"],
        bootstrap["record_sha256"],
    )
    _, latest, open_attempts = gate_status(records, len(state["gate_command_sha256"]))
    if open_attempts or any(index not in latest or latest[index]["exit_code"] != 0 for index in range(1, len(state["gate_command_sha256"]) + 1)):
        fail("gate_set_incomplete")
    return records[-1]["record_sha256"]


def load_binding(root, state):
    binding = load_json_file(root / "d2-binding.json", "d2_binding", 0o600)
    required = {
        "schema_version",
        "kind",
        "merge_commit",
        "merge_tree",
        "gate_evidence_chain_head_sha256",
        "candidate_bundle_path",
        "candidate_bundle_record_sha256",
        "candidate_bundle_file_sha256",
        "run_id",
        "manifest_sha256",
        "d2_manifest_path",
        "d2_run_directory_device",
        "d2_run_directory_inode",
        "d2_orchestrator_directory_device",
        "d2_orchestrator_directory_inode",
        "d2_run_entries",
        "d2_run_total_bytes",
        "d2_run_tree_sha256",
        "steps",
        "receipt_chain_head_sha256",
        "coordinator_evidence_sha256",
        "final_record_sha256",
        "verified_at",
        "record_sha256",
    }
    if not isinstance(binding, dict) or set(binding) != required:
        fail("d2_binding_fields_invalid")
    if (
        not valid_schema_version(binding["schema_version"])
        or binding["kind"] != "starring.d3.d2-binding.v1"
        or binding["merge_commit"] != state["merge_commit"]
        or binding["merge_tree"] != state["merge_tree"]
        or binding["candidate_bundle_path"]
        != str(root / "candidate-bundle" / "bundle.json")
        or binding["steps"] != 17
    ):
        fail("d2_binding_identity_invalid")
    validate_digest(binding["manifest_sha256"], "d2_binding_manifest")
    validate_digest(binding["d2_run_tree_sha256"], "d2_binding_run_tree")
    validate_digest(
        binding["gate_evidence_chain_head_sha256"], "d2_binding_gate_chain"
    )
    validate_digest(
        binding["candidate_bundle_record_sha256"], "d2_binding_candidate_record"
    )
    validate_digest(
        binding["candidate_bundle_file_sha256"], "d2_binding_candidate_file"
    )
    validate_digest(binding["receipt_chain_head_sha256"], "d2_binding_chain")
    validate_digest(
        binding["coordinator_evidence_sha256"], "d2_binding_coordinator"
    )
    validate_digest(binding["final_record_sha256"], "d2_binding_final")
    validate_timestamp(binding["verified_at"], "d2_binding_verified_at")
    verify_sealed_record(binding, "d2_binding")
    require_active_d2_binding(binding)
    return binding


def validate_recheck_pull_request(value, state, verified_at):
    required = {
        "number",
        "state",
        "draft",
        "merged",
        "base_ref",
        "base_sha",
        "head_sha",
        "repository",
        "author",
        "approvals",
    }
    if (
        not isinstance(value, dict)
        or set(value) != required
        or value.get("number") != state["pr_number"]
        or value.get("state") != "open"
        or value.get("draft") is not False
        or value.get("merged") is not False
        or value.get("base_ref") != state["base_ref"]
        or value.get("base_sha") != state["expected_base"]
        or value.get("head_sha") != state["expected_head"]
        or value.get("repository") != state["github_repository"]
    ):
        fail("recheck_pull_request_invalid")
    author = github_actor(value["author"], "recheck_pull_request_author")
    approvals = value["approvals"]
    if not isinstance(approvals, list) or not approvals or len(approvals) > 10000:
        fail("recheck_pull_request_approvals_invalid")
    verified_time = datetime.datetime.fromisoformat(
        verified_at[:-1] + "+00:00"
    )
    reviewer_ids = set()
    reviewer_logins = set()
    prior_order = None
    for approval in approvals:
        if not isinstance(approval, dict) or set(approval) != {
            "id",
            "state",
            "submitted_at",
            "commit_sha",
            "reviewer",
            "repository_permission",
            "repository_role_name",
        }:
            fail("recheck_pull_request_approvals_invalid")
        review_id = approval.get("id")
        submitted_at = validate_timestamp(
            approval.get("submitted_at"), "recheck_review_submitted_at"
        )
        reviewer = github_actor(
            approval.get("reviewer"), "recheck_pull_request_reviewer"
        )
        folded_login = reviewer["login"].casefold()
        order = (reviewer["id"], review_id)
        if (
            type(review_id) is not int
            or review_id <= 0
            or approval.get("state") != "APPROVED"
            or approval.get("commit_sha") != state["expected_head"]
            or approval.get("repository_permission")
            not in ("admin", "maintain", "write")
            or not isinstance(approval.get("repository_role_name"), str)
            or not 1 <= len(approval["repository_role_name"]) <= 100
            or any(
                character.isspace() and character not in " "
                for character in approval["repository_role_name"]
            )
            or reviewer["type"] != "User"
            or folded_login.endswith("[bot]")
            or reviewer["id"] == author["id"]
            or folded_login == author["login"].casefold()
            or reviewer["id"] in reviewer_ids
            or folded_login in reviewer_logins
            or datetime.datetime.fromisoformat(submitted_at[:-1] + "+00:00")
            > verified_time
            or (prior_order is not None and order <= prior_order)
        ):
            fail("recheck_pull_request_approvals_invalid")
        reviewer_ids.add(reviewer["id"])
        reviewer_logins.add(folded_login)
        prior_order = order
    return value


def load_recheck(root, state):
    record = load_json_file(root / "recheck.json", "recheck", 0o600)
    required = {
        "schema_version",
        "kind",
        "pr_number",
        "head_commit",
        "base_commit",
        "merge_commit",
        "merge_tree",
        "pull_request",
        "gate_evidence_chain_head_sha256",
        "candidate_bundle_record_sha256",
        "candidate_bundle_file_sha256",
        "d2_receipt_chain_head_sha256",
        "d2_coordinator_evidence_sha256",
        "verified_at",
        "record_sha256",
    }
    if (
        not isinstance(record, dict)
        or set(record) != required
        or not valid_schema_version(record.get("schema_version"))
        or record.get("kind") != "starring.d3.pr-recheck.v1"
        or type(record.get("pr_number")) is not int
        or record.get("pr_number") != state["pr_number"]
        or record.get("head_commit") != state["expected_head"]
        or record.get("base_commit") != state["expected_base"]
        or record.get("merge_commit") != state["merge_commit"]
        or record.get("merge_tree") != state["merge_tree"]
        or not isinstance(record.get("gate_evidence_chain_head_sha256"), str)
        or not DIGEST_PATTERN.fullmatch(record["gate_evidence_chain_head_sha256"])
        or not isinstance(record.get("candidate_bundle_record_sha256"), str)
        or not DIGEST_PATTERN.fullmatch(record["candidate_bundle_record_sha256"])
        or not isinstance(record.get("candidate_bundle_file_sha256"), str)
        or not DIGEST_PATTERN.fullmatch(record["candidate_bundle_file_sha256"])
        or not isinstance(record.get("d2_receipt_chain_head_sha256"), str)
        or not DIGEST_PATTERN.fullmatch(record["d2_receipt_chain_head_sha256"])
        or not isinstance(record.get("d2_coordinator_evidence_sha256"), str)
        or not DIGEST_PATTERN.fullmatch(record["d2_coordinator_evidence_sha256"])
    ):
        fail("recheck_identity_invalid")
    verified_at = validate_timestamp(record["verified_at"], "recheck_verified_at")
    validate_recheck_pull_request(record["pull_request"], state, verified_at)
    verify_sealed_record(record, "recheck")
    return record


def github_run(repository, run_id, main_commit, base_ref, workflow_file):
    if type(run_id) is not int or run_id <= 0 or run_id > 9223372036854775807:
        fail("actions_run_id_invalid")
    _, raw = run_process(
        ["gh", "api", f"repos/{repository}/actions/runs/{run_id}"],
        pathlib.Path.cwd(),
        f"actions_run_{run_id}",
    )
    value = load_json_bytes(raw, "actions_run")
    if not isinstance(value, dict):
        fail("actions_run_invalid")
    workflow_id = value.get("workflow_id")
    repository_identity = value.get("repository")
    head_repository_identity = value.get("head_repository")
    run_attempt = value.get("run_attempt")
    if (
        value.get("id") != run_id
        or value.get("head_sha") != main_commit
        or value.get("head_branch") != base_ref
        or value.get("event") != "push"
        or value.get("name") != REQUIRED_ACTIONS_WORKFLOW["name"]
        or value.get("path") != REQUIRED_ACTIONS_WORKFLOW["path"]
        or value.get("pull_requests") != []
        or not isinstance(repository_identity, dict)
        or repository_identity.get("full_name") != repository
        or not isinstance(head_repository_identity, dict)
        or head_repository_identity.get("full_name") != repository
        or type(run_attempt) is not int
        or run_attempt <= 0
    ):
        fail("actions_run_identity_invalid")
    if value.get("status") != "completed" or value.get("conclusion") != "success":
        fail("actions_run_not_successful")
    if type(workflow_id) is not int or workflow_id <= 0:
        fail("actions_workflow_identity_invalid")
    _, raw_workflow = run_process(
        ["gh", "api", f"repos/{repository}/actions/workflows/{workflow_id}"],
        pathlib.Path.cwd(),
        f"actions_workflow_{workflow_id}",
    )
    workflow = load_json_bytes(raw_workflow, "actions_workflow")
    if (
        not isinstance(workflow, dict)
        or workflow.get("id") != workflow_id
        or workflow.get("name") != REQUIRED_ACTIONS_WORKFLOW["name"]
        or workflow.get("path") != REQUIRED_ACTIONS_WORKFLOW["path"]
        or workflow.get("state") != REQUIRED_ACTIONS_WORKFLOW["state"]
    ):
        fail("actions_workflow_identity_invalid")
    _, raw_jobs = run_process(
        [
            "gh",
            "api",
            f"repos/{repository}/actions/runs/{run_id}/jobs?filter=latest&per_page=100",
        ],
        pathlib.Path.cwd(),
        f"actions_jobs_{run_id}",
    )
    jobs_value = load_json_bytes(raw_jobs, "actions_jobs")
    jobs = jobs_value.get("jobs") if isinstance(jobs_value, dict) else None
    if (
        not isinstance(jobs, list)
        or jobs_value.get("total_count") != len(REQUIRED_ACTIONS_JOBS)
        or len(jobs) != len(REQUIRED_ACTIONS_JOBS)
    ):
        fail("actions_jobs_invalid")
    normalized_jobs = []
    observed_names = set()
    for job in jobs:
        if (
            not isinstance(job, dict)
            or type(job.get("id")) is not int
            or job["id"] <= 0
            or job.get("run_id") != run_id
            or job.get("workflow_name") != REQUIRED_ACTIONS_WORKFLOW["name"]
            or job.get("head_branch") != base_ref
            or job.get("head_sha") != main_commit
            or job.get("name") not in REQUIRED_ACTIONS_JOBS
            or job.get("status") != "completed"
            or job.get("conclusion") != "success"
            or job["name"] in observed_names
        ):
            fail("actions_jobs_invalid")
        observed_names.add(job["name"])
        normalized_jobs.append(
            {
                "id": job["id"],
                "name": job["name"],
                "status": "completed",
                "conclusion": "success",
            }
        )
    if observed_names != set(REQUIRED_ACTIONS_JOBS):
        fail("actions_jobs_invalid")
    normalized_jobs.sort(key=lambda job: REQUIRED_ACTIONS_JOBS.index(job["name"]))
    return {
        "id": run_id,
        "run_attempt": run_attempt,
        "workflow_id": workflow_id,
        "workflow_name": workflow["name"],
        "workflow_path": workflow["path"],
        "workflow_file": workflow_file,
        "head_branch": value["head_branch"],
        "head_sha": value["head_sha"],
        "event": "push",
        "repository": repository,
        "status": "completed",
        "conclusion": "success",
        "jobs": normalized_jobs,
    }


def github_actor(value, label):
    if not isinstance(value, dict):
        fail(f"{label}_invalid")
    actor_id = value.get("id")
    login = value.get("login")
    actor_type = value.get("type")
    if (
        type(actor_id) is not int
        or actor_id <= 0
        or not isinstance(login, str)
        or not 1 <= len(login) <= 100
        or not all(
            character.isascii()
            and (character.isalnum() or character in "-[]")
            for character in login
        )
        or not isinstance(actor_type, str)
        or actor_type not in ("User", "Bot", "Mannequin")
    ):
        fail(f"{label}_invalid")
    return {"id": actor_id, "login": login, "type": actor_type}


def pull_request_approvals(pages, state, author, approved_before):
    validate_timestamp(approved_before, "pull_request_approval_deadline")
    if not isinstance(pages, list) or len(pages) > 100:
        fail("pull_request_reviews_invalid")
    reviews = []
    for page in pages:
        if not isinstance(page, list) or len(page) > 100:
            fail("pull_request_reviews_invalid")
        reviews.extend(page)
    if len(reviews) > 10000:
        fail("pull_request_reviews_invalid")
    identities = {}
    logins = {}
    review_ids = set()
    decisions = {}
    allowed_states = {
        "APPROVED",
        "CHANGES_REQUESTED",
        "COMMENTED",
        "DISMISSED",
        "PENDING",
    }
    for review in reviews:
        if not isinstance(review, dict):
            fail("pull_request_reviews_invalid")
        review_id = review.get("id")
        review_state = review.get("state")
        if (
            type(review_id) is not int
            or review_id <= 0
            or review_id in review_ids
            or not isinstance(review_state, str)
            or review_state not in allowed_states
        ):
            fail("pull_request_reviews_invalid")
        review_ids.add(review_id)
        reviewer = github_actor(review.get("user"), "pull_request_reviewer")
        submitted_at = review.get("submitted_at")
        if review_state == "PENDING":
            if submitted_at is not None:
                fail("pull_request_reviews_invalid")
        else:
            submitted_at = validate_timestamp(
                submitted_at, "pull_request_review_submitted_at"
            )
        commit_sha = validate_sha(
            review.get("commit_id"), "pull_request_review_commit"
        )
        prior_identity = identities.setdefault(reviewer["id"], reviewer)
        if prior_identity != reviewer:
            fail("pull_request_reviewer_identity_invalid")
        folded_login = reviewer["login"].casefold()
        prior_id = logins.setdefault(folded_login, reviewer["id"])
        if prior_id != reviewer["id"]:
            fail("pull_request_reviewer_identity_invalid")
        if review_state in ("APPROVED", "CHANGES_REQUESTED", "DISMISSED"):
            ordering = (
                datetime.datetime.fromisoformat(submitted_at[:-1] + "+00:00"),
                review_id,
            )
            prior = decisions.get(reviewer["id"])
            if prior is None or ordering > prior[0]:
                decisions[reviewer["id"]] = (
                    ordering,
                    {
                        "id": review_id,
                        "state": review_state,
                        "submitted_at": submitted_at,
                        "commit_sha": commit_sha,
                        "reviewer": reviewer,
                    },
                )
    approval_deadline = datetime.datetime.fromisoformat(
        approved_before[:-1] + "+00:00"
    )
    approvals = []
    for _, review in decisions.values():
        reviewer = review["reviewer"]
        is_bot = (
            reviewer["type"] != "User"
            or reviewer["login"].casefold().endswith("[bot]")
        )
        is_author = (
            reviewer["id"] == author["id"]
            or reviewer["login"].casefold() == author["login"].casefold()
        )
        submitted_time = datetime.datetime.fromisoformat(
            review["submitted_at"][:-1] + "+00:00"
        )
        if (
            review["state"] == "APPROVED"
            and review["commit_sha"] == state["expected_head"]
            and submitted_time <= approval_deadline
            and not is_bot
            and not is_author
        ):
            approvals.append(review)
    approvals.sort(key=lambda review: (review["reviewer"]["id"], review["id"]))
    if not approvals:
        fail("pull_request_approval_required")
    return approvals


def github_pull_common(repository, state, identity_error):
    _, raw = run_process(
        [
            "gh",
            "api",
            f"repos/{repository}/pulls/{state['pr_number']}",
        ],
        pathlib.Path.cwd(),
        f"pull_request_{state['pr_number']}",
    )
    value = load_json_bytes(raw, "pull_request")
    base = value.get("base") if isinstance(value, dict) else None
    head = value.get("head") if isinstance(value, dict) else None
    base_repository = base.get("repo") if isinstance(base, dict) else None
    head_repository = head.get("repo") if isinstance(head, dict) else None
    author = github_actor(
        value.get("user") if isinstance(value, dict) else None,
        "pull_request_author",
    )
    if (
        not isinstance(value, dict)
        or type(value.get("number")) is not int
        or value.get("number") != state["pr_number"]
        or value.get("draft") is not False
        or not isinstance(base, dict)
        or base.get("ref") != state["base_ref"]
        or base.get("sha") != state["expected_base"]
        or not isinstance(base_repository, dict)
        or base_repository.get("full_name") != repository
        or not isinstance(head, dict)
        or head.get("sha") != state["expected_head"]
        or not isinstance(head_repository, dict)
        or head_repository.get("full_name") != repository
    ):
        fail(identity_error)
    return value, author


def github_review_pages(repository, state):
    _, raw_reviews = run_process(
        [
            "gh",
            "api",
            "--paginate",
            "--slurp",
            f"repos/{repository}/pulls/{state['pr_number']}/reviews?per_page=100",
        ],
        pathlib.Path.cwd(),
        f"pull_request_reviews_{state['pr_number']}",
    )
    return load_json_bytes(raw_reviews, "pull_request_reviews")


def authorized_pull_request_approvals(repository, approvals):
    authorized = []
    for approval in approvals:
        reviewer = approval["reviewer"]
        _, raw = run_process(
            [
                "gh",
                "api",
                f"repos/{repository}/collaborators/{reviewer['login']}/permission",
            ],
            pathlib.Path.cwd(),
            f"pull_request_reviewer_permission_{reviewer['id']}",
        )
        value = load_json_bytes(raw, "pull_request_reviewer_permission")
        permission = value.get("permission") if isinstance(value, dict) else None
        role_name = value.get("role_name") if isinstance(value, dict) else None
        permission_actor = github_actor(
            value.get("user") if isinstance(value, dict) else None,
            "pull_request_permission_user",
        )
        if (
            permission not in ("admin", "maintain", "write", "triage", "read", "none")
            or not isinstance(role_name, str)
            or not 1 <= len(role_name) <= 100
            or any(character.isspace() and character not in " " for character in role_name)
            or permission_actor != reviewer
        ):
            fail("pull_request_reviewer_permission_invalid")
        if permission in ("admin", "maintain", "write"):
            authorized.append(
                {
                    **approval,
                    "repository_permission": permission,
                    "repository_role_name": role_name,
                }
            )
    if not authorized:
        fail("pull_request_approval_required")
    return authorized


def require_open_pull_request(value):
    if (
        value.get("state") != "open"
        or value.get("merged") is not False
        or value.get("merged_at") is not None
    ):
        fail("pull_request_recheck_identity_invalid")


def github_pull_recheck(repository, state):
    value, author = github_pull_common(
        repository, state, "pull_request_recheck_identity_invalid"
    )
    require_open_pull_request(value)
    approvals = authorized_pull_request_approvals(
        repository,
        pull_request_approvals(
            github_review_pages(repository, state),
            state,
            author,
            utc_now(),
        ),
    )
    confirmed_value, confirmed_author = github_pull_common(
        repository, state, "pull_request_recheck_identity_invalid"
    )
    require_open_pull_request(confirmed_value)
    if confirmed_author != author:
        fail("pull_request_recheck_identity_invalid")
    return {
        "number": state["pr_number"],
        "state": "open",
        "draft": False,
        "merged": False,
        "base_ref": state["base_ref"],
        "base_sha": state["expected_base"],
        "head_sha": state["expected_head"],
        "repository": repository,
        "author": author,
        "approvals": approvals,
    }


def github_pull(repository, state, main_commit):
    value, author = github_pull_common(
        repository, state, "pull_request_merge_identity_invalid"
    )
    if (
        value.get("state") != "closed"
        or value.get("merged") is not True
        or value.get("merge_commit_sha") != main_commit
    ):
        fail("pull_request_merge_identity_invalid")
    merged_at = validate_timestamp(value.get("merged_at"), "pull_request_merged_at")
    approvals = authorized_pull_request_approvals(
        repository,
        pull_request_approvals(
            github_review_pages(repository, state),
            state,
            author,
            merged_at,
        ),
    )
    return {
        "number": state["pr_number"],
        "state": "closed",
        "draft": False,
        "merged": True,
        "merged_at": merged_at,
        "merge_commit_sha": main_commit,
        "base_ref": state["base_ref"],
        "base_sha": state["expected_base"],
        "head_sha": state["expected_head"],
        "repository": repository,
        "author": author,
        "approvals": approvals,
    }


def validate_final_approval_records(approvals, state, author, merged_at):
    if not isinstance(approvals, list) or not approvals or len(approvals) > 10000:
        fail("final_pull_request_approvals_invalid")
    merged_time = datetime.datetime.fromisoformat(merged_at[:-1] + "+00:00")
    review_ids = set()
    reviewer_ids = set()
    reviewer_logins = set()
    prior_order = None
    required = {
        "id",
        "state",
        "submitted_at",
        "commit_sha",
        "reviewer",
        "repository_permission",
        "repository_role_name",
    }
    for approval in approvals:
        if not isinstance(approval, dict) or set(approval) != required:
            fail("final_pull_request_approvals_invalid")
        review_id = approval.get("id")
        if type(review_id) is not int or review_id <= 0:
            fail("final_pull_request_approvals_invalid")
        submitted_at = validate_timestamp(
            approval.get("submitted_at"), "final_review_submitted_at"
        )
        reviewer = github_actor(
            approval.get("reviewer"), "final_pull_request_reviewer"
        )
        folded_login = reviewer["login"].casefold()
        role_name = approval.get("repository_role_name")
        order = (reviewer["id"], review_id)
        if (
            review_id in review_ids
            or approval.get("state") != "APPROVED"
            or approval.get("commit_sha") != state["expected_head"]
            or reviewer["type"] != "User"
            or folded_login.endswith("[bot]")
            or reviewer["id"] == author["id"]
            or folded_login == author["login"].casefold()
            or reviewer["id"] in reviewer_ids
            or folded_login in reviewer_logins
            or approval.get("repository_permission")
            not in ("admin", "maintain", "write")
            or not isinstance(role_name, str)
            or not 1 <= len(role_name) <= 100
            or any(
                character.isspace() and character not in " "
                for character in role_name
            )
            or datetime.datetime.fromisoformat(submitted_at[:-1] + "+00:00")
            > merged_time
            or (prior_order is not None and order <= prior_order)
        ):
            fail("final_pull_request_approvals_invalid")
        review_ids.add(review_id)
        reviewer_ids.add(reviewer["id"])
        reviewer_logins.add(folded_login)
        prior_order = order
    return approvals


def validate_terminal_final_replay(
    record,
    state,
    gate_chain,
    binding,
    recheck,
    bundle_record,
    bundle_file,
    repo,
    repository,
    run_ids,
):
    verify_sealed_record(record, "final")
    required = {
        "schema_version",
        "kind",
        "pr_number",
        "merge_commit",
        "merge_tree",
        "main_commit",
        "main_tree",
        "gate_count",
        "gate_evidence_chain_head_sha256",
        "candidate_bundle_path",
        "candidate_bundle_record_sha256",
        "candidate_bundle_file_sha256",
        "d2_run_id",
        "d2_manifest_sha256",
        "d2_receipt_chain_head_sha256",
        "d2_coordinator_evidence_sha256",
        "d2_binding_sha256",
        "rechecked_head_commit",
        "rechecked_base_commit",
        "recheck_sha256",
        "github_repository",
        "pull_request",
        "actions_runs",
        "status",
        "finalized_at",
        "record_sha256",
    }
    if not valid_schema_version(record.get("schema_version")):
        fail("final_schema_invalid")
    if set(record) != required:
        fail("final_fields_invalid")
    validate_timestamp(record["finalized_at"], "finalized_at")
    local_identity = {
        "kind": "starring.d3.exact-tree-certification.v1",
        "pr_number": state["pr_number"],
        "merge_commit": state["merge_commit"],
        "merge_tree": state["merge_tree"],
        "main_tree": state["merge_tree"],
        "gate_count": len(state["gate_command_sha256"]),
        "gate_evidence_chain_head_sha256": gate_chain,
        "candidate_bundle_path": binding["candidate_bundle_path"],
        "candidate_bundle_record_sha256": bundle_record["record_sha256"],
        "candidate_bundle_file_sha256": bundle_file["sha256"],
        "d2_run_id": binding["run_id"],
        "d2_manifest_sha256": binding["manifest_sha256"],
        "d2_receipt_chain_head_sha256": binding["receipt_chain_head_sha256"],
        "d2_coordinator_evidence_sha256": binding[
            "coordinator_evidence_sha256"
        ],
        "d2_binding_sha256": binding["record_sha256"],
        "rechecked_head_commit": recheck["head_commit"],
        "rechecked_base_commit": recheck["base_commit"],
        "recheck_sha256": recheck["record_sha256"],
        "github_repository": repository,
        "status": "passed",
    }
    if any(record.get(key) != value for key, value in local_identity.items()):
        fail("finalize_replay_mismatch")
    main_commit = validate_sha(record.get("main_commit"), "final_main_commit")
    if commit_tree(repo, main_commit, "final_replay_main") != record["main_tree"]:
        fail("finalize_replay_mismatch")
    pull_request = record.get("pull_request")
    pull_fields = {
        "number",
        "state",
        "draft",
        "merged",
        "merged_at",
        "merge_commit_sha",
        "base_ref",
        "base_sha",
        "head_sha",
        "repository",
        "author",
        "approvals",
    }
    if (
        not isinstance(pull_request, dict)
        or set(pull_request) != pull_fields
        or pull_request.get("number") != state["pr_number"]
        or pull_request.get("state") != "closed"
        or pull_request.get("draft") is not False
        or pull_request.get("merged") is not True
        or pull_request.get("merge_commit_sha") != main_commit
        or pull_request.get("base_ref") != state["base_ref"]
        or pull_request.get("base_sha") != state["expected_base"]
        or pull_request.get("head_sha") != state["expected_head"]
        or pull_request.get("repository") != repository
        or pull_request.get("author") != recheck["pull_request"]["author"]
        or not isinstance(pull_request.get("approvals"), list)
    ):
        fail("finalize_replay_mismatch")
    merged_at = validate_timestamp(
        pull_request["merged_at"], "final_pull_request_merged_at"
    )
    if datetime.datetime.fromisoformat(
        recheck["verified_at"][:-1] + "+00:00"
    ) >= datetime.datetime.fromisoformat(merged_at[:-1] + "+00:00"):
        fail("finalize_replay_mismatch")
    validated_approvals = validate_final_approval_records(
        pull_request["approvals"],
        state,
        pull_request["author"],
        merged_at,
    )
    approvals = {
        approval.get("id"): approval
        for approval in validated_approvals
    }
    if any(
        approvals.get(approval["id"]) != approval
        for approval in recheck["pull_request"]["approvals"]
    ):
        fail("finalize_replay_mismatch")
    actions_runs = record.get("actions_runs")
    if not isinstance(actions_runs, list) or len(actions_runs) != 1:
        fail("finalize_replay_mismatch")
    actions_run = actions_runs[0]
    run_fields = {
        "id",
        "run_attempt",
        "workflow_id",
        "workflow_name",
        "workflow_path",
        "workflow_file",
        "head_branch",
        "head_sha",
        "event",
        "repository",
        "status",
        "conclusion",
        "jobs",
    }
    expected_workflow_file = git_file_identity(
        repo,
        main_commit,
        REQUIRED_ACTIONS_WORKFLOW["path"],
        "final_replay_workflow_file",
    )
    if (
        not isinstance(actions_run, dict)
        or set(actions_run) != run_fields
        or actions_run.get("id") != run_ids[0]
        or type(actions_run.get("run_attempt")) is not int
        or actions_run["run_attempt"] <= 0
        or type(actions_run.get("workflow_id")) is not int
        or actions_run["workflow_id"] <= 0
        or actions_run.get("workflow_name") != REQUIRED_ACTIONS_WORKFLOW["name"]
        or actions_run.get("workflow_path") != REQUIRED_ACTIONS_WORKFLOW["path"]
        or actions_run.get("workflow_file") != expected_workflow_file
        or actions_run.get("head_branch") != state["base_ref"]
        or actions_run.get("head_sha") != main_commit
        or actions_run.get("event") != "push"
        or actions_run.get("repository") != repository
        or actions_run.get("status") != "completed"
        or actions_run.get("conclusion") != "success"
    ):
        fail("finalize_replay_mismatch")
    jobs = actions_run.get("jobs")
    if (
        not isinstance(jobs, list)
        or [job.get("name") for job in jobs if isinstance(job, dict)]
        != list(REQUIRED_ACTIONS_JOBS)
        or any(
            set(job) != {"id", "name", "status", "conclusion"}
            or type(job.get("id")) is not int
            or job["id"] <= 0
            or job.get("status") != "completed"
            or job.get("conclusion") != "success"
            for job in jobs
            if isinstance(job, dict)
        )
        or any(not isinstance(job, dict) for job in jobs)
    ):
        fail("finalize_replay_mismatch")
    return record


def command_finalize(arguments):
    repository = arguments.github_repository
    if not REPOSITORY_PATTERN.fullmatch(repository):
        fail("github_repository_invalid")
    run_ids = arguments.actions_run_id
    if len(run_ids) != 1 or len(run_ids) != len(set(run_ids)):
        fail("actions_run_ids_invalid")
    state_argument, root = state_lock_target(arguments.state)
    with StateLock(root):
        state_path, state, _, repo, _ = load_state(str(state_argument))
        if repository != state["github_repository"]:
            fail("github_repository_mismatch")
        gate_chain = require_gate_completion(root, state)
        binding = load_binding(root, state)
        recheck = load_recheck(root, state)
        try:
            dependency_snapshot = candidate_dependency_snapshot(root)
            bundle_record = load_candidate_bundle(
                root,
                state,
                gate_chain,
                dependency_snapshot,
            )
            candidate_bundle_file = record_file_identity(root)
        except CandidateBundleError as error:
            fail(str(error))
        if (
            recheck["gate_evidence_chain_head_sha256"] != gate_chain
            or recheck["candidate_bundle_record_sha256"]
            != bundle_record["record_sha256"]
            or recheck["candidate_bundle_file_sha256"]
            != candidate_bundle_file["sha256"]
            or binding["candidate_bundle_record_sha256"]
            != bundle_record["record_sha256"]
            or binding["candidate_bundle_file_sha256"]
            != candidate_bundle_file["sha256"]
            or recheck["d2_receipt_chain_head_sha256"]
            != binding["receipt_chain_head_sha256"]
            or recheck["d2_coordinator_evidence_sha256"]
            != binding["coordinator_evidence_sha256"]
        ):
            fail("recheck_certification_binding_mismatch")
        if github_repository_from_remote(repo, state["remote"]) != repository:
            fail("github_repository_mismatch")
        final_path = root / "final.json"
        if final_path.exists():
            existing = validate_terminal_final_replay(
                load_json_file(final_path, "final", 0o600),
                state,
                gate_chain,
                binding,
                recheck,
                bundle_record,
                candidate_bundle_file,
                repo,
                repository,
                run_ids,
            )
            require_active_d2_binding(binding)
            try:
                candidate_bundle.retire_candidate_build_root(root)
            except CandidateBundleError as error:
                fail(str(error))
            return {**existing, "disposition": "exact_replay"}
        main_commit = fetch_main(repo, state["remote"], state["base_ref"])
        main_tree = commit_tree(repo, main_commit, "finalize_main")
        if main_tree != state["merge_tree"]:
            fail("main_tree_mismatch")
        pull_request = github_pull(repository, state, main_commit)
        sealed_pull_request = recheck["pull_request"]
        if datetime.datetime.fromisoformat(
            recheck["verified_at"][:-1] + "+00:00"
        ) >= datetime.datetime.fromisoformat(
            pull_request["merged_at"][:-1] + "+00:00"
        ):
            fail("pull_request_recheck_not_before_merge")
        current_approvals = {
            approval["id"]: approval
            for approval in pull_request["approvals"]
        }
        if (
            pull_request["author"] != sealed_pull_request["author"]
            or any(
                current_approvals.get(approval["id"]) != approval
                for approval in sealed_pull_request["approvals"]
            )
        ):
            fail("pull_request_recheck_approval_invalid")
        workflow_file = git_file_identity(
            repo,
            main_commit,
            REQUIRED_ACTIONS_WORKFLOW["path"],
            "actions_workflow_file",
        )
        runs = [
            github_run(
                repository,
                run_id,
                main_commit,
                state["base_ref"],
                workflow_file,
            )
            for run_id in run_ids
        ]
        require_active_d2_binding(binding)
        final_pull_request = github_pull(repository, state, main_commit)
        if final_pull_request != pull_request:
            fail("pull_request_final_snapshot_changed")
        identity = {
            "schema_version": SCHEMA_VERSION,
            "kind": "starring.d3.exact-tree-certification.v1",
            "pr_number": state["pr_number"],
            "merge_commit": state["merge_commit"],
            "merge_tree": state["merge_tree"],
            "main_commit": main_commit,
            "main_tree": main_tree,
            "gate_count": len(state["gate_command_sha256"]),
            "gate_evidence_chain_head_sha256": gate_chain,
            "candidate_bundle_path": binding["candidate_bundle_path"],
            "candidate_bundle_record_sha256": bundle_record["record_sha256"],
            "candidate_bundle_file_sha256": candidate_bundle_file["sha256"],
            "d2_run_id": binding["run_id"],
            "d2_manifest_sha256": binding["manifest_sha256"],
            "d2_receipt_chain_head_sha256": binding["receipt_chain_head_sha256"],
            "d2_coordinator_evidence_sha256": binding[
                "coordinator_evidence_sha256"
            ],
            "d2_binding_sha256": binding["record_sha256"],
            "rechecked_head_commit": recheck["head_commit"],
            "rechecked_base_commit": recheck["base_commit"],
            "recheck_sha256": recheck["record_sha256"],
            "github_repository": repository,
            "pull_request": pull_request,
            "actions_runs": runs,
            "status": "passed",
        }
        record = seal_record({**identity, "finalized_at": utc_now()})
        write_new_json(final_path, record)
        require_active_d2_binding(binding)
        try:
            candidate_bundle.retire_candidate_build_root(root)
        except CandidateBundleError as error:
            fail(str(error))
        return {**record, "disposition": "created"}


def parser():
    root = argparse.ArgumentParser(prog="d3-certification")
    commands = root.add_subparsers(dest="command", required=True)
    prepare = commands.add_parser("prepare")
    prepare.add_argument("--repo", required=True)
    prepare.add_argument("--output-root", required=True)
    prepare.add_argument("--pr-number", type=int, required=True)
    prepare.add_argument("--expected-head", required=True)
    prepare.add_argument("--expected-base", required=True)
    prepare.add_argument("--remote", default="origin")
    prepare.add_argument("--base-ref", default="main")
    prepare.add_argument("--gate", action="append", required=True)
    prepare.set_defaults(handler=command_prepare)
    gates = commands.add_parser("run-gates")
    gates.add_argument("--state", required=True)
    gates.add_argument("--gate", action="append", required=True)
    gates.add_argument("--postgres-database-url-file")
    gates.add_argument("--timeout-seconds", type=int, default=7200)
    gates.set_defaults(handler=command_run_gates)
    bind = commands.add_parser("bind-d2")
    bind.add_argument("--state", required=True)
    bind.add_argument("--d2-manifest", required=True)
    bind.add_argument("--d2-final-record", required=True)
    bind.set_defaults(handler=command_bind_d2)
    recheck = commands.add_parser("recheck")
    recheck.add_argument("--state", required=True)
    recheck.set_defaults(handler=command_recheck)
    finalize = commands.add_parser("finalize")
    finalize.add_argument("--state", required=True)
    finalize.add_argument("--github-repository", required=True)
    finalize.add_argument("--actions-run-id", action="append", type=int, required=True)
    finalize.set_defaults(handler=command_finalize)
    return root


def main(argv=None):
    try:
        arguments = parser().parse_args(argv)
        if getattr(arguments, "timeout_seconds", 1) <= 0:
            fail("timeout_invalid")
        result = arguments.handler(arguments)
        print(canonical_json(result))
        return 0
    except D3Error as error:
        print(str(error), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
