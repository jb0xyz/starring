import argparse
import datetime
import fcntl
import hashlib
import json
import os
import pathlib
import re
import shlex
import stat
import subprocess
import sys
import time
import urllib.parse


SCHEMA_VERSION = 1
MAX_JSON_BYTES = 1024 * 1024
MAX_PROCESS_OUTPUT_BYTES = 1024 * 1024
SHA_PATTERN = re.compile(r"^[0-9a-f]{40}$")
DIGEST_PATTERN = re.compile(r"^[0-9a-f]{64}$")
NAME_PATTERN = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._/-]{0,191}$")
REPOSITORY_PATTERN = re.compile(r"^[A-Za-z0-9_.-]{1,100}/[A-Za-z0-9_.-]{1,100}$")
REPOSITORY_COMPONENT_PATTERN = re.compile(r"^[A-Za-z0-9_.-]{1,100}$")
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
REQUIRED_ACTIONS_WORKFLOW = {
    "name": "CI",
    "path": ".github/workflows/ci.yml",
    "state": "active",
}
REQUIRED_ACTIONS_JOBS = ("checks", "postgres")
REQUIRED_GATE_COMMANDS = (
    "cargo fmt --all -- --check",
    "cargo build --locked --workspace --all-targets",
    "cargo test --locked --workspace",
    "cargo clippy --locked --workspace --all-targets -- -D warnings",
    "cargo build --locked -p interaction-smoke --features unsafe-dev-activation",
    "python3 -m unittest discover -s tools/d2-certification -p 'test_*.py'",
    "npm --prefix tools/codex-worker run check",
    "npm --prefix tools/codex-worker test",
    "npm --prefix eval/codex-worker-slo run check",
    "npm --prefix eval/design-harness ci",
    "npm --prefix eval/design-harness run audit",
    "npm --prefix eval/design-harness run check",
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


def run_process(argv, cwd, label, allowed=(0,), timeout=None, discard=False):
    environment = {
        name: os.environ[name]
        for name in SAFE_ENVIRONMENT_NAMES
        if name in os.environ
    }
    environment["GIT_TERMINAL_PROMPT"] = "0"
    environment["LC_ALL"] = "C"
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
    if manifest["schema_version"] != SCHEMA_VERSION or manifest["kind"] != "starring.d3.exact-tree-state.v1":
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
    root = create_state_root(output_root, state_name(arguments.pr_number, expected_head, expected_base))
    with StateLock(root):
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
        state_path = root / "state.json"
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


def load_gate_evidence(path, state):
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
            record["schema_version"] != SCHEMA_VERSION
            or record["merge_commit"] != state["merge_commit"]
            or record["merge_tree"] != state["merge_tree"]
            or record["previous_sha256"] != previous
            or type(record["gate_index"]) is not int
            or not 1 <= record["gate_index"] <= len(state["gate_command_sha256"])
            or record["command_sha256"] != state["gate_command_sha256"][record["gate_index"] - 1]
            or type(record["attempt"]) is not int
            or record["attempt"] <= 0
        ):
            fail("gate_evidence_identity_invalid")
        validate_timestamp(record["observed_at"], "gate_evidence_observed_at")
        previous = evidence_record_hash(record)
        records.append(record)
    return records


def append_gate_evidence(path, state, records, value):
    record = {
        "schema_version": SCHEMA_VERSION,
        **value,
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


def command_run_gates(arguments):
    digests = gate_digests(arguments.gate)
    state_argument, root = state_lock_target(arguments.state)
    with StateLock(root):
        state_path, state, _, repo, worktree = load_state(str(state_argument))
        if digests != state["gate_command_sha256"]:
            fail("gate_plan_mismatch")
        validate_worktree(state, repo, worktree)
        evidence_path = root / "gate-evidence.jsonl"
        records = load_gate_evidence(evidence_path, state)
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
                )
            started = time.monotonic_ns()
            try:
                return_code, _ = run_process(
                    ["/bin/zsh", "-f", "-c", command],
                    worktree,
                    f"gate_{index}",
                    allowed=tuple(range(256)),
                    timeout=arguments.timeout_seconds,
                    discard=True,
                )
            except D3Error:
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
            )
            validate_worktree(state, repo, worktree)
            if return_code != 0:
                fail(f"gate_failed:{index}:{return_code}")
            latest[index] = completion
        records = load_gate_evidence(evidence_path, state)
        _, latest, open_attempts = gate_status(records, len(digests))
        if open_attempts or any(index not in latest or latest[index]["exit_code"] != 0 for index in range(1, len(digests) + 1)):
            fail("gate_set_incomplete")
        return {
            "merge_commit": state["merge_commit"],
            "merge_tree": state["merge_tree"],
            "gates": len(digests),
            "status": "passed",
            "evidence_chain_head_sha256": records[-1]["record_sha256"],
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
            or receipt["schema_version"] != 1
            or receipt["run_id"] != manifest.get("run_id")
            or receipt["manifest_sha256"] != manifest_digest
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


def command_bind_d2(arguments):
    d2_manifest_path = absolute_path(arguments.d2_manifest, "d2_manifest")
    d2_final_path = absolute_path(arguments.d2_final_record, "d2_final_record")
    state_argument, root = state_lock_target(arguments.state)
    with StateLock(root):
        state_path, state, _, repo, worktree = load_state(str(state_argument))
        d2_manifest = load_json_file(d2_manifest_path, "d2_manifest", 0o600)
        if not isinstance(d2_manifest, dict):
            fail("d2_manifest_invalid")
        manifest_digest_path = d2_manifest_path.with_name("manifest.sha256")
        manifest_digest = load_small_ascii(manifest_digest_path, "d2_manifest_digest")
        validate_digest(manifest_digest, "d2_manifest_digest")
        observed_manifest_digest = sha256_bytes(canonical_json(d2_manifest).encode("utf-8"))
        if manifest_digest != observed_manifest_digest:
            fail("d2_manifest_digest_mismatch")
        if d2_manifest.get("commit_sha") != state["merge_commit"]:
            fail("d2_commit_mismatch")
        if commit_tree(repo, d2_manifest["commit_sha"], "d2_commit") != state["merge_tree"]:
            fail("d2_tree_mismatch")
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
        if final_record != expected_final:
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
            "run_id": final_record["run_id"],
            "manifest_sha256": manifest_digest,
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
            return {**existing, "disposition": "exact_replay"}
        binding = seal_record({**identity, "verified_at": utc_now()})
        write_new_json(binding_path, binding)
        return {**binding, "disposition": "created"}


def command_recheck(arguments):
    state_argument, root = state_lock_target(arguments.state)
    with StateLock(root):
        state_path, state, _, repo, _ = load_state(str(state_argument))
        gate_chain = require_gate_completion(state_path.parent, state)
        binding = load_binding(state_path.parent, state)
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
        record_path = state_path.parent / "recheck.json"
        identity = {
            "schema_version": SCHEMA_VERSION,
            "kind": "starring.d3.pr-recheck.v1",
            "pr_number": state["pr_number"],
            "head_commit": head,
            "base_commit": base,
            "merge_commit": merge,
            "merge_tree": state["merge_tree"],
            "gate_evidence_chain_head_sha256": gate_chain,
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
            return {**existing, "disposition": "exact_replay"}
        record = seal_record({**identity, "verified_at": utc_now()})
        write_new_json(record_path, record)
        return {**record, "disposition": "created"}


def require_gate_completion(root, state):
    path = root / "gate-evidence.jsonl"
    records = load_gate_evidence(path, state)
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
        "run_id",
        "manifest_sha256",
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
        binding["schema_version"] != SCHEMA_VERSION
        or binding["kind"] != "starring.d3.d2-binding.v1"
        or binding["merge_commit"] != state["merge_commit"]
        or binding["merge_tree"] != state["merge_tree"]
        or binding["steps"] != 17
    ):
        fail("d2_binding_identity_invalid")
    validate_digest(binding["manifest_sha256"], "d2_binding_manifest")
    validate_digest(binding["receipt_chain_head_sha256"], "d2_binding_chain")
    validate_digest(
        binding["coordinator_evidence_sha256"], "d2_binding_coordinator"
    )
    validate_digest(binding["final_record_sha256"], "d2_binding_final")
    validate_timestamp(binding["verified_at"], "d2_binding_verified_at")
    verify_sealed_record(binding, "d2_binding")
    return binding


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
        "gate_evidence_chain_head_sha256",
        "d2_receipt_chain_head_sha256",
        "d2_coordinator_evidence_sha256",
        "verified_at",
        "record_sha256",
    }
    if (
        not isinstance(record, dict)
        or set(record) != required
        or record.get("head_commit") != state["expected_head"]
        or record.get("base_commit") != state["expected_base"]
        or record.get("merge_commit") != state["merge_commit"]
        or record.get("merge_tree") != state["merge_tree"]
        or not isinstance(record.get("gate_evidence_chain_head_sha256"), str)
        or not DIGEST_PATTERN.fullmatch(record["gate_evidence_chain_head_sha256"])
        or not isinstance(record.get("d2_receipt_chain_head_sha256"), str)
        or not DIGEST_PATTERN.fullmatch(record["d2_receipt_chain_head_sha256"])
        or not isinstance(record.get("d2_coordinator_evidence_sha256"), str)
        or not DIGEST_PATTERN.fullmatch(record["d2_coordinator_evidence_sha256"])
    ):
        fail("recheck_identity_invalid")
    validate_timestamp(record["verified_at"], "recheck_verified_at")
    verify_sealed_record(record, "recheck")
    return record


def github_run(repository, run_id, main_commit, base_ref):
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
        or value.get("path")
        != f"{REQUIRED_ACTIONS_WORKFLOW['path']}@{base_ref}"
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
        "head_branch": value["head_branch"],
        "head_sha": value["head_sha"],
        "event": "push",
        "repository": repository,
        "status": "completed",
        "conclusion": "success",
        "jobs": normalized_jobs,
    }


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
        if (
            recheck["gate_evidence_chain_head_sha256"] != gate_chain
            or recheck["d2_receipt_chain_head_sha256"]
            != binding["receipt_chain_head_sha256"]
            or recheck["d2_coordinator_evidence_sha256"]
            != binding["coordinator_evidence_sha256"]
        ):
            fail("recheck_certification_binding_mismatch")
        if github_repository_from_remote(repo, state["remote"]) != repository:
            fail("github_repository_mismatch")
        base_remote_ref = f"refs/remotes/{state['remote']}/{state['base_ref']}"
        git(
            repo,
            [
                "fetch",
                "--atomic",
                "--no-tags",
                "--force",
                state["remote"],
                f"refs/heads/{state['base_ref']}:{base_remote_ref}",
            ],
            "finalize_fetch",
        )
        main_commit = validate_sha(git_text(repo, ["rev-parse", base_remote_ref], "finalize_main"), "finalize_main")
        main_tree = commit_tree(repo, main_commit, "finalize_main")
        if main_tree != state["merge_tree"]:
            fail("main_tree_mismatch")
        runs = [
            github_run(repository, run_id, main_commit, state["base_ref"])
            for run_id in run_ids
        ]
        final_path = root / "final.json"
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
            "actions_runs": runs,
            "status": "passed",
        }
        if final_path.exists():
            existing = load_json_file(final_path, "final", 0o600)
            verify_sealed_record(existing, "final")
            if set(existing) != set(identity) | {"finalized_at", "record_sha256"}:
                fail("final_fields_invalid")
            validate_timestamp(existing["finalized_at"], "finalized_at")
            if {key: existing.get(key) for key in identity} != identity:
                fail("finalize_replay_mismatch")
            return {**existing, "disposition": "exact_replay"}
        record = seal_record({**identity, "finalized_at": utc_now()})
        write_new_json(final_path, record)
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
