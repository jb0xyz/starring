import contextlib
import datetime
import fcntl
import json
import os
import pathlib
import plistlib
import re
import stat

from d2_certification import (
    D2_API_PORT,
    D2_CLOUDFLARE_TUNNEL_ID,
    D2_ORIGIN_SERVICE,
    D2_PUBLIC_ORIGIN,
    fsync_directory,
    isolated_runtime_root,
    load_audited_recovery_manifest,
    load_verified_manifest,
    sha256_file,
    validate_utc_timestamp,
)


SCHEMA_VERSION = 1
PG_BIN_ROOT = pathlib.Path("/opt/homebrew/opt/postgresql@16/bin")
REQUIRED_PROGRAMS = {
    "initdb": PG_BIN_ROOT / "initdb",
    "pg_ctl": PG_BIN_ROOT / "pg_ctl",
    "pg_isready": PG_BIN_ROOT / "pg_isready",
    "psql": PG_BIN_ROOT / "psql",
    "launchctl": pathlib.Path("/bin/launchctl"),
    "plutil": pathlib.Path("/usr/bin/plutil"),
    "security": pathlib.Path("/usr/bin/security"),
    "curl": pathlib.Path("/usr/bin/curl"),
}
PROTECTED_PORTS = {5432, 18080, 18181, 19091}
PROTECTED_PLIST_ROOT = pathlib.Path.home() / "Library" / "LaunchAgents"
IDENTITY_PATTERN = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,191}$")
DIGEST_PATTERN = re.compile(r"^[0-9a-f]{64}$")
RUN_ID_PATTERN = re.compile(r"^d2-[0-9]{8}t[0-9]{6}z-[0-9a-f]{12}$")
SNOWFLAKE_PATTERN = re.compile(r"^[1-9][0-9]{0,19}$")
PHASES = {
    "preparing",
    "prepared",
    "substrate_starting",
    "substrate_started",
    "credentials_sealing",
    "candidate_starting",
    "candidate_started",
    "onboarding",
    "stopped",
    "cleaned",
}
API_DATABASE_ACCOUNTS = (
    "database.oauth-flow-writer",
    "database.session-issuer",
    "database.session-api",
    "database.security-revoker",
    "database.installation-authority-reader",
    "database.authorized-snapshot-reader",
    "database.promotion-executor",
    "database.decision-reader",
    "database.approval-executor",
    "database.rejection-executor",
    "database.apply-executor",
    "database.cancellation-executor",
    "database.deployment-status-reader",
    "database.operational-deployment-status-reader",
    "database.authoring-session-writer",
)
RUNTIME_DATABASE_ACCOUNTS = (
    "database.execution",
    "database.exact-target",
    "database.panel",
    "database.serving",
    "database.interaction",
)
API_KEYRING_ACCOUNTS = ("keyring.product-action", "keyring.snapshot-envelope")
RUNTIME_KEYRING_ACCOUNTS = ("interaction.token-envelope-keyring",)
POSTGRES_ACCOUNTS = ("database.cluster-admin",)
WORKER_ACCOUNTS = ("authoring.bearer-token",)
OWNER_ACCOUNT = "lifecycle-owner"
STANDING_DISCORD_IDENTITIES = (
    ("starring-api.staging", "discord.oauth-client-secret"),
    ("starring-api.staging", "discord.bot-token"),
    ("starring.runtime.staging", "discord.bot-token"),
)
STANDING_PUBLIC_ORIGIN = "https://api.starring.co.kr"
PROTECTED_KEYCHAIN_SERVICES = {
    "starring-api.staging",
    "starring.runtime.staging",
    "starring.postgres.staging",
    "com.starring.llm-api-key",
    "com.cloudflare.tunnel.macmini-llm-prod",
}
GLOBAL_LOCK_PATH = pathlib.Path("/private/tmp/starring-d2-certification.lock")
GLOBAL_DISCORD_OWNERSHIP_REGISTRY_PATH = pathlib.Path(
    "/private/tmp/starring-d2-discord-ownership-registry.json"
)
D2_RUNTIME_ROOT_PARENT = pathlib.Path("/private/tmp")
DISCORD_OWNERSHIP_REGISTRY_KIND = "starring.d2.discord-ownership-registry.v1"
DISCORD_OWNERSHIP_RECORD_FIELDS = {
    "run_id",
    "manifest_sha256",
    "manifest_path",
    "guild_id",
    "application_id",
    "bot_user_id",
}


class OrchestratorError(Exception):
    pass


def fail(code):
    raise OrchestratorError(code)


def utc_now():
    return (
        datetime.datetime.now(datetime.timezone.utc)
        .replace(microsecond=0)
        .isoformat()
        .replace("+00:00", "Z")
    )


def fsync_shared_runtime_parent(path, label):
    try:
        metadata = path.lstat()
    except OSError as error:
        fail(f"{label}_unavailable:{error.__class__.__name__}")
    if (
        path != D2_RUNTIME_ROOT_PARENT
        or not stat.S_ISDIR(metadata.st_mode)
        or path.is_symlink()
        or metadata.st_uid != 0
        or stat.S_IMODE(metadata.st_mode) != 0o1777
    ):
        fail(f"{label}_invalid")
    flags = os.O_RDONLY
    if hasattr(os, "O_DIRECTORY"):
        flags |= os.O_DIRECTORY
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        fail(f"{label}_open_failed:{error.__class__.__name__}")
    try:
        opened = os.fstat(descriptor)
        if (
            not stat.S_ISDIR(opened.st_mode)
            or opened.st_uid != 0
            or stat.S_IMODE(opened.st_mode) != 0o1777
            or (opened.st_dev, opened.st_ino) != (metadata.st_dev, metadata.st_ino)
        ):
            fail(f"{label}_identity_changed")
        os.fsync(descriptor)
    except OSError as error:
        fail(f"{label}_fsync_failed:{error.__class__.__name__}")
    finally:
        os.close(descriptor)


def write_atomic(path, payload, mode=0o600, shared_runtime_parent=False):
    parent_created = not path.parent.exists()
    if shared_runtime_parent and (
        path.parent != D2_RUNTIME_ROOT_PARENT or parent_created
    ):
        fail("atomic_parent_invalid")
    if shared_runtime_parent:
        fsync_shared_runtime_parent(path.parent, "atomic_parent")
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    if parent_created:
        fsync_directory(path.parent.parent, "atomic_parent_parent")
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    descriptor = os.open(
        temporary,
        os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0),
        mode,
    )
    try:
        os.fchmod(descriptor, mode)
        data = payload if isinstance(payload, bytes) else payload.encode("utf-8")
        written = 0
        while written < len(data):
            count = os.write(descriptor, data[written:])
            if count <= 0:
                fail("atomic_write_failed")
            written += count
        os.fsync(descriptor)
    finally:
        os.close(descriptor)
    os.replace(temporary, path)
    if shared_runtime_parent:
        fsync_shared_runtime_parent(path.parent, "atomic_parent")
    else:
        fsync_directory(path.parent, "atomic_parent")


def load_json(path, code):
    try:
        metadata = path.lstat()
        if not stat.S_ISREG(metadata.st_mode) or path.is_symlink():
            fail(code)
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError):
        fail(code)


def strict_registry_object(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            fail("discord_ownership_registry_invalid")
        value[key] = item
    return value


def validate_registry_snowflake(value):
    if (
        not isinstance(value, str)
        or not SNOWFLAKE_PATTERN.fullmatch(value)
        or int(value) > 18446744073709551615
    ):
        fail("discord_ownership_registry_invalid")
    return value


def validate_discord_ownership_registry(registry):
    if (
        not isinstance(registry, dict)
        or set(registry) != {"schema_version", "kind", "owners"}
        or type(registry.get("schema_version")) is not int
        or registry.get("schema_version") != SCHEMA_VERSION
        or registry.get("kind") != DISCORD_OWNERSHIP_REGISTRY_KIND
        or not isinstance(registry.get("owners"), list)
    ):
        fail("discord_ownership_registry_invalid")
    owners = registry["owners"]
    for owner in owners:
        if not isinstance(owner, dict) or set(owner) != DISCORD_OWNERSHIP_RECORD_FIELDS:
            fail("discord_ownership_registry_invalid")
        if (
            not isinstance(owner["run_id"], str)
            or not RUN_ID_PATTERN.fullmatch(owner["run_id"])
            or not isinstance(owner["manifest_sha256"], str)
            or not DIGEST_PATTERN.fullmatch(owner["manifest_sha256"])
            or not isinstance(owner["manifest_path"], str)
            or not pathlib.Path(owner["manifest_path"]).is_absolute()
        ):
            fail("discord_ownership_registry_invalid")
        validate_registry_snowflake(owner["guild_id"])
        validate_registry_snowflake(owner["application_id"])
        validate_registry_snowflake(owner["bot_user_id"])
    if owners != sorted(owners, key=lambda owner: owner["run_id"]):
        fail("discord_ownership_registry_invalid")
    for field in (
        "run_id",
        "manifest_sha256",
        "manifest_path",
        "guild_id",
        "application_id",
    ):
        values = [owner[field] for owner in owners]
        if len(values) != len(set(values)):
            fail("discord_ownership_registry_invalid")
    return registry


def empty_discord_ownership_registry():
    return {
        "schema_version": SCHEMA_VERSION,
        "kind": DISCORD_OWNERSHIP_REGISTRY_KIND,
        "owners": [],
    }


def load_discord_ownership_registry():
    path = GLOBAL_DISCORD_OWNERSHIP_REGISTRY_PATH
    try:
        descriptor = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    except FileNotFoundError:
        return empty_discord_ownership_registry()
    except OSError:
        fail("discord_ownership_registry_invalid")
    try:
        metadata = os.fstat(descriptor)
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_uid != os.getuid()
            or stat.S_IMODE(metadata.st_mode) != 0o600
            or metadata.st_nlink != 1
            or metadata.st_size <= 0
            or metadata.st_size > 1048576
        ):
            fail("discord_ownership_registry_invalid")
        payload = b""
        while len(payload) <= 1048576:
            chunk = os.read(descriptor, min(65536, 1048577 - len(payload)))
            if not chunk:
                break
            payload += chunk
        if len(payload) > 1048576:
            fail("discord_ownership_registry_invalid")
    finally:
        os.close(descriptor)
    try:
        registry = json.loads(
            payload.decode("utf-8"), object_pairs_hook=strict_registry_object
        )
    except OrchestratorError:
        raise
    except (UnicodeDecodeError, json.JSONDecodeError):
        fail("discord_ownership_registry_invalid")
    return validate_discord_ownership_registry(registry)


def write_discord_ownership_registry(registry):
    validate_discord_ownership_registry(registry)
    path = GLOBAL_DISCORD_OWNERSHIP_REGISTRY_PATH
    write_atomic(
        path,
        json.dumps(
            registry, ensure_ascii=False, sort_keys=True, separators=(",", ":")
        )
        + "\n",
        shared_runtime_parent=path.parent == D2_RUNTIME_ROOT_PARENT,
    )


def discord_ownership_record(context):
    discord = context.manifest["discord"]
    return {
        "run_id": context.manifest["run_id"],
        "manifest_sha256": context.digest,
        "manifest_path": str(context.manifest_path),
        "guild_id": discord["guild_id"],
        "application_id": discord["application_id"],
        "bot_user_id": discord["bot_user_id"],
    }


def require_registered_d2_runtime_roots(registry):
    registered_run_ids = {owner["run_id"] for owner in registry["owners"]}
    try:
        entries = tuple(D2_RUNTIME_ROOT_PARENT.iterdir())
    except OSError:
        fail("discord_ownership_reconciliation_failed")
    for entry in entries:
        name = entry.name
        if not name.startswith("starring-d2-"):
            continue
        run_id = name.removeprefix("starring-d2-")
        if RUN_ID_PATTERN.fullmatch(run_id) and run_id not in registered_run_ids:
            fail("unregistered_d2_runtime_present")


def require_discord_ownership_available(context):
    registry = load_discord_ownership_registry()
    require_registered_d2_runtime_roots(registry)
    record = discord_ownership_record(context)
    for owner in registry["owners"]:
        if owner == record:
            continue
        if any(
            owner[field] == record[field]
            for field in ("run_id", "manifest_sha256", "manifest_path")
        ):
            fail("discord_ownership_record_mismatch")
        if owner["guild_id"] == record["guild_id"]:
            fail("discord_guild_owned_by_other_d2_run")
        if owner["application_id"] == record["application_id"]:
            fail("discord_application_owned_by_other_d2_run")
    return registry


def claim_discord_ownership(context):
    registry = require_discord_ownership_available(context)
    record = discord_ownership_record(context)
    if record in registry["owners"]:
        return "already_claimed"
    registry["owners"].append(record)
    registry["owners"].sort(key=lambda owner: owner["run_id"])
    write_discord_ownership_registry(registry)
    return "claimed"


def require_discord_ownership_claimed(context):
    registry = load_discord_ownership_registry()
    record = discord_ownership_record(context)
    if record not in registry["owners"]:
        fail("discord_ownership_claim_absent")
    return registry


def require_discord_ownership_released(context):
    registry = load_discord_ownership_registry()
    record = discord_ownership_record(context)
    for owner in registry["owners"]:
        if owner == record:
            fail("cleaned_state_discord_ownership_drift")
        if any(
            owner[field] == record[field]
            for field in (
                "run_id",
                "manifest_sha256",
                "manifest_path",
            )
        ):
            fail("discord_ownership_claim_mismatch")
    return registry


def release_discord_ownership(context):
    registry = load_discord_ownership_registry()
    record = discord_ownership_record(context)
    if record not in registry["owners"]:
        for owner in registry["owners"]:
            if any(
                owner[field] == record[field]
                for field in (
                    "run_id",
                    "manifest_sha256",
                    "manifest_path",
                    "guild_id",
                    "application_id",
                )
            ):
                fail("discord_ownership_claim_mismatch")
        return "already_released"
    registry["owners"].remove(record)
    write_discord_ownership_registry(registry)
    return "released"


def validate_identity(value, code):
    if not isinstance(value, str) or not IDENTITY_PATTERN.fullmatch(value):
        fail(code)
    return value


def strict_journal_object(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            fail("journal_invalid")
        value[key] = item
    return value


def parse_journal_rows(context, raw):
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError:
        fail("journal_invalid")
    rows = []
    for sequence, line in enumerate(text.splitlines(), 1):
        try:
            row = json.loads(line, object_pairs_hook=strict_journal_object)
        except json.JSONDecodeError:
            fail("journal_invalid")
        if (
            not isinstance(row, dict)
            or set(row)
            != {
                "schema_version",
                "sequence",
                "recorded_at",
                "manifest_sha256",
                "action",
                "status",
                "target",
            }
            or type(row["schema_version"]) is not int
            or row["schema_version"] != SCHEMA_VERSION
            or type(row["sequence"]) is not int
            or row["sequence"] != sequence
            or row["manifest_sha256"] != context.digest
            or not validate_utc_timestamp(row["recorded_at"])
        ):
            fail("journal_invalid")
        for field in ("action", "status", "target"):
            validate_identity(row[field], "journal_invalid")
        rows.append(row)
    return rows


def journal_file_identity(metadata):
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


def read_repaired_journal_descriptor(context, descriptor):
    before = os.fstat(descriptor)
    if (
        not stat.S_ISREG(before.st_mode)
        or before.st_uid != os.getuid()
        or before.st_nlink != 1
        or stat.S_IMODE(before.st_mode) != 0o600
        or before.st_size > 8 * 1024 * 1024
    ):
        fail("journal_invalid")
    os.lseek(descriptor, 0, os.SEEK_SET)
    raw = bytearray()
    while len(raw) <= 8 * 1024 * 1024:
        chunk = os.read(descriptor, 64 * 1024)
        if not chunk:
            break
        raw.extend(chunk)
    if len(raw) > 8 * 1024 * 1024:
        fail("journal_invalid")
    after = os.fstat(descriptor)
    try:
        named = os.stat(context.journal_path, follow_symlinks=False)
    except OSError:
        fail("journal_invalid")
    if (
        journal_file_identity(before) != journal_file_identity(after)
        or journal_file_identity(after) != journal_file_identity(named)
        or len(raw) != after.st_size
    ):
        fail("journal_invalid")
    if raw and not raw.endswith(b"\n"):
        boundary = raw.rfind(b"\n") + 1
        rows = parse_journal_rows(context, bytes(raw[:boundary]))
        os.ftruncate(descriptor, boundary)
        os.fsync(descriptor)
        truncated = os.fstat(descriptor)
        try:
            named = os.stat(context.journal_path, follow_symlinks=False)
        except OSError:
            fail("journal_invalid")
        if (
            (truncated.st_dev, truncated.st_ino, truncated.st_uid, truncated.st_nlink)
            != (before.st_dev, before.st_ino, before.st_uid, before.st_nlink)
            or journal_file_identity(truncated) != journal_file_identity(named)
            or truncated.st_size != boundary
        ):
            fail("journal_invalid")
        return rows
    return parse_journal_rows(context, bytes(raw))


def read_repaired_journal(context):
    if not os.path.lexists(context.journal_path):
        return []
    flags = os.O_RDWR | getattr(os, "O_NOFOLLOW", 0)
    try:
        lock_descriptor = os.open(context.lock_path, flags)
    except OSError:
        fail("journal_invalid")
    try:
        lock_metadata = os.fstat(lock_descriptor)
        if (
            not stat.S_ISREG(lock_metadata.st_mode)
            or lock_metadata.st_uid != os.getuid()
            or lock_metadata.st_nlink != 1
            or stat.S_IMODE(lock_metadata.st_mode) != 0o600
        ):
            fail("journal_invalid")
        fcntl.flock(lock_descriptor, fcntl.LOCK_EX)
        try:
            journal_descriptor = os.open(context.journal_path, flags)
        except OSError:
            fail("journal_invalid")
        try:
            return read_repaired_journal_descriptor(context, journal_descriptor)
        finally:
            os.close(journal_descriptor)
    finally:
        fcntl.flock(lock_descriptor, fcntl.LOCK_UN)
        os.close(lock_descriptor)


def read_strict_journal_snapshot(context):
    """Read the journal without repairing or otherwise mutating it."""
    if not os.path.lexists(context.journal_path):
        return [], b""
    lock_flags = os.O_RDWR | getattr(os, "O_NOFOLLOW", 0)
    try:
        lock_descriptor = os.open(context.lock_path, lock_flags)
    except OSError:
        fail("journal_invalid")
    try:
        lock_metadata = os.fstat(lock_descriptor)
        if (
            not stat.S_ISREG(lock_metadata.st_mode)
            or lock_metadata.st_uid != os.getuid()
            or lock_metadata.st_nlink != 1
            or stat.S_IMODE(lock_metadata.st_mode) != 0o600
        ):
            fail("journal_invalid")
        fcntl.flock(lock_descriptor, fcntl.LOCK_EX)
        try:
            descriptor = os.open(
                context.journal_path,
                os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0),
            )
        except OSError:
            fail("journal_invalid")
        try:
            before = os.fstat(descriptor)
            if (
                not stat.S_ISREG(before.st_mode)
                or before.st_uid != os.getuid()
                or before.st_nlink != 1
                or stat.S_IMODE(before.st_mode) != 0o600
                or before.st_size > 8 * 1024 * 1024
            ):
                fail("journal_invalid")
            raw = bytearray()
            while len(raw) <= 8 * 1024 * 1024:
                chunk = os.read(descriptor, 64 * 1024)
                if not chunk:
                    break
                raw.extend(chunk)
            after = os.fstat(descriptor)
            try:
                named = os.stat(context.journal_path, follow_symlinks=False)
            except OSError:
                fail("journal_invalid")
            if (
                len(raw) > 8 * 1024 * 1024
                or len(raw) != before.st_size
                or journal_file_identity(before) != journal_file_identity(after)
                or journal_file_identity(after) != journal_file_identity(named)
                or (raw and not raw.endswith(b"\n"))
            ):
                fail("journal_invalid")
            rows = parse_journal_rows(context, bytes(raw))
            canonical = b"".join(
                (
                    json.dumps(
                        row,
                        ensure_ascii=False,
                        sort_keys=True,
                        separators=(",", ":"),
                    )
                    + "\n"
                ).encode("utf-8")
                for row in rows
            )
            if canonical != bytes(raw):
                fail("journal_invalid")
            return rows, bytes(raw)
        finally:
            os.close(descriptor)
    finally:
        fcntl.flock(lock_descriptor, fcntl.LOCK_UN)
        os.close(lock_descriptor)


@contextlib.contextmanager
def global_operation_lock():
    descriptor = os.open(
        GLOBAL_LOCK_PATH,
        os.O_RDWR | os.O_CREAT | getattr(os, "O_NOFOLLOW", 0),
        0o600,
    )
    try:
        metadata = os.fstat(descriptor)
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_uid != os.getuid()
            or stat.S_IMODE(metadata.st_mode) != 0o600
        ):
            fail("global_lock_invalid")
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            fail("d2_operation_busy")
        yield
    finally:
        try:
            fcntl.flock(descriptor, fcntl.LOCK_UN)
        finally:
            os.close(descriptor)


class RunContext:
    def __init__(self, manifest_path, manifest, digest):
        self.manifest_path = manifest_path
        self.manifest = manifest
        self.digest = digest
        self.run_directory = manifest_path.parent
        self.artifact_directory = self.run_directory / "orchestrator"
        self.plist_directory = self.artifact_directory / "launchd"
        self.state_path = self.artifact_directory / "state.json"
        self.journal_path = self.artifact_directory / "lifecycle.jsonl"
        self.lock_path = self.artifact_directory / "lifecycle.lock"
        self.root = pathlib.Path(manifest["database"]["cluster_root"]).parent
        self.cluster_root = pathlib.Path(manifest["database"]["cluster_root"])
        self.socket_directory = pathlib.Path(manifest["database"]["socket_directory"])
        self.log_directory = self.root / "logs"
        self.postgres_log = self.log_directory / "postgres.log"


def context_from_verified_manifest(manifest_path, manifest, digest):
    run_id = manifest["run_id"]
    expected_root = isolated_runtime_root(run_id)
    database = manifest.get("database")
    services = manifest.get("services")
    keychain = manifest.get("keychain_services")
    if not isinstance(database, dict) or not isinstance(services, dict) or not isinstance(keychain, dict):
        fail("manifest_orchestration_shape_invalid")
    if pathlib.Path(database.get("cluster_root", "")) != expected_root / "postgres":
        fail("manifest_cluster_root_invalid")
    if pathlib.Path(database.get("socket_directory", "")) != expected_root / "socket":
        fail("manifest_socket_directory_invalid")
    if database.get("name") != "starring_runtime_staging":
        fail("manifest_database_name_invalid")
    discord = manifest.get("discord")
    if not isinstance(discord, dict):
        fail("manifest_discord_shape_invalid")
    validate_identity(discord.get("application_id"), "manifest_discord_application_invalid")
    validate_identity(discord.get("bot_user_id"), "manifest_discord_bot_invalid")
    validate_identity(discord.get("actor_id"), "manifest_discord_actor_invalid")
    if manifest.get("public_origin") == STANDING_PUBLIC_ORIGIN:
        fail("dedicated_public_origin_required")
    if manifest.get("cloudflare") != {
        "tunnel_id": D2_CLOUDFLARE_TUNNEL_ID,
        "public_origin": D2_PUBLIC_ORIGIN,
        "origin_service": D2_ORIGIN_SERVICE,
    } or services.get("api", {}).get("port") != D2_API_PORT:
        fail("manifest_cloudflare_route_binding_invalid")
    suffix = run_id.rsplit("-", 1)[1]
    expected_labels = {
        "api": f"local.starring.d2.{suffix}.api",
        "runtime": f"local.starring.d2.{suffix}.runtime",
        "worker": f"local.starring.d2.{suffix}.worker",
        "transport": f"local.starring.d2.{suffix}.transport",
        "tunnel": f"local.starring.d2.{suffix}.tunnel",
    }
    if set(services) != set(expected_labels):
        fail("manifest_service_inventory_invalid")
    for name, expected in expected_labels.items():
        if services[name].get("label") != expected:
            fail("manifest_service_label_invalid")
    expected_keychain = {
        "api": f"starring.d2.{suffix}.api",
        "runtime": f"starring.d2.{suffix}.runtime",
        "postgres": f"starring.d2.{suffix}.postgres",
        "worker": f"starring.d2.{suffix}.worker",
    }
    if keychain != expected_keychain:
        fail("manifest_keychain_namespace_invalid")
    protected = set(manifest["protected_staging"]["launchd_labels"])
    if protected.intersection(expected_labels.values()):
        fail("manifest_protected_label_collision")
    if set(expected_keychain.values()).intersection(
        {identity[0] for identity in STANDING_DISCORD_IDENTITIES}
    ):
        fail("manifest_protected_keychain_collision")
    external_keychain = manifest.get("external_keychain")
    if not isinstance(external_keychain, dict) or set(external_keychain) != {
        "discord_oauth_client_secret",
        "discord_bot_token",
        "tunnel_token",
    }:
        fail("manifest_external_keychain_invalid")
    external_identities = []
    for identity in external_keychain.values():
        if not isinstance(identity, dict) or set(identity) != {"service", "account"}:
            fail("manifest_external_keychain_invalid")
        external_identities.append(
            (
                validate_identity(identity["service"], "manifest_external_keychain_invalid"),
                validate_identity(identity["account"], "manifest_external_keychain_invalid"),
            )
        )
    if len(set(external_identities)) != 3:
        fail("manifest_external_keychain_duplicate")
    if set(external_identities).intersection(STANDING_DISCORD_IDENTITIES):
        fail("manifest_protected_keychain_collision")
    if {identity[0] for identity in external_identities}.intersection(
        PROTECTED_KEYCHAIN_SERVICES
    ):
        fail("manifest_protected_keychain_collision")
    if {identity[0] for identity in external_identities}.intersection(
        set(expected_keychain.values())
    ):
        fail("manifest_external_keychain_collision")
    context = RunContext(manifest_path, manifest, digest)
    metadata = context.run_directory.lstat()
    if (
        not stat.S_ISDIR(metadata.st_mode)
        or context.run_directory.is_symlink()
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) != 0o700
    ):
        fail("run_directory_invalid")
    if context.artifact_directory.exists():
        artifact_metadata = context.artifact_directory.lstat()
        if (
            not stat.S_ISDIR(artifact_metadata.st_mode)
            or context.artifact_directory.is_symlink()
            or artifact_metadata.st_uid != os.getuid()
            or stat.S_IMODE(artifact_metadata.st_mode) != 0o700
        ):
            fail("artifact_directory_invalid")
    return context


def load_context(raw_manifest):
    return context_from_verified_manifest(*load_verified_manifest(raw_manifest))


def load_audited_recovery_context(raw_manifest):
    manifest_path, manifest, digest, observations = load_audited_recovery_manifest(
        raw_manifest
    )
    return (
        context_from_verified_manifest(manifest_path, manifest, digest),
        observations,
    )


def keychain_inventory(context):
    services = context.manifest["keychain_services"]
    inventory = []
    for account in API_DATABASE_ACCOUNTS + API_KEYRING_ACCOUNTS + (OWNER_ACCOUNT,):
        inventory.append((services["api"], account))
    for account in RUNTIME_DATABASE_ACCOUNTS + RUNTIME_KEYRING_ACCOUNTS + (OWNER_ACCOUNT,):
        inventory.append((services["runtime"], account))
    for account in POSTGRES_ACCOUNTS + (OWNER_ACCOUNT,):
        inventory.append((services["postgres"], account))
    for account in WORKER_ACCOUNTS + (OWNER_ACCOUNT,):
        inventory.append((services["worker"], account))
    if len(inventory) != len(set(inventory)):
        fail("keychain_inventory_duplicate")
    return tuple(inventory)


def owner_identities(context):
    services = context.manifest["keychain_services"]
    return tuple((services[name], OWNER_ACCOUNT) for name in sorted(services))


def external_keychain_inventory(context):
    identities = context.manifest["external_keychain"]
    return tuple(
        (identities[name]["service"], identities[name]["account"])
        for name in sorted(identities)
    )


def plist_sha(path):
    try:
        metadata = path.lstat()
    except OSError:
        return None
    if not stat.S_ISREG(metadata.st_mode) or path.is_symlink():
        fail("protected_plist_invalid")
    return sha256_file(path)


def standing_snapshot(context, platform):
    labels = context.manifest["protected_staging"]["launchd_labels"]
    return {
        "launchd_loaded": {
            label: platform.launchd_loaded(label) for label in sorted(labels)
        },
        "plist_sha256": {
            label: plist_sha(PROTECTED_PLIST_ROOT / f"{label}.plist")
            for label in sorted(labels)
        },
        "port_occupied": {
            str(port): not platform.port_available(port) for port in sorted(PROTECTED_PORTS)
        },
    }


def validate_dedicated_discord_identity(context):
    path = PROTECTED_PLIST_ROOT / "local.starring.api.staging.plist"
    if not path.exists():
        return
    try:
        metadata = path.lstat()
        if not stat.S_ISREG(metadata.st_mode) or path.is_symlink():
            fail("protected_plist_invalid")
        value = plistlib.loads(path.read_bytes())
        environment = value.get("EnvironmentVariables", {})
    except (OSError, plistlib.InvalidFileException, AttributeError):
        fail("protected_plist_invalid")
    if context.manifest["discord"]["application_id"] == environment.get(
        "STARRING_API_DISCORD_APPLICATION_ID"
    ):
        fail("dedicated_discord_application_required")
    if context.manifest["discord"]["bot_user_id"] == environment.get(
        "STARRING_API_DISCORD_BOT_USER_ID"
    ):
        fail("dedicated_discord_bot_required")


def validate_programs(platform):
    for name, path in REQUIRED_PROGRAMS.items():
        if not platform.executable(path):
            fail(f"required_program_unavailable:{name}")
    version = platform.run([REQUIRED_PROGRAMS["initdb"], "--version"], timeout=5)
    if version.returncode != 0 or b" 16." not in version.stdout:
        fail("postgres_version_invalid")


def validate_candidate_programs(context, platform):
    for name in (
        "api",
        "runtime",
        "codex",
        "node",
        "cloudflared",
        "db_bootstrap",
        "sealed_provisioner",
        "certification_transport",
    ):
        candidate = pathlib.Path(context.manifest["candidates"][name]["path"])
        if not platform.executable(candidate):
            fail(f"candidate_not_executable:{name}")


def validate_ports(context, platform, require_available=True):
    ports = {
        context.manifest["database"]["port"],
        context.manifest["services"]["api"]["port"],
        context.manifest["services"]["runtime"]["port"],
        context.manifest["services"]["worker"]["port"],
        context.manifest["services"]["transport"]["gateway_port"],
        context.manifest["services"]["transport"]["http_port"],
    }
    if len(ports) != 6 or ports.intersection(PROTECTED_PORTS):
        fail("isolated_port_contract_invalid")
    if require_available:
        for port in ports:
            if not platform.port_available(port):
                fail("isolated_port_busy")


def append_journal(context, action, status, target):
    artifact_created = not context.artifact_directory.exists()
    context.artifact_directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    if artifact_created:
        fsync_directory(context.artifact_directory.parent, "journal_artifact_parent")
    lock_created = not context.lock_path.exists()
    descriptor = os.open(
        context.lock_path,
        os.O_RDWR | os.O_CREAT | getattr(os, "O_NOFOLLOW", 0),
        0o600,
    )
    try:
        os.fchmod(descriptor, 0o600)
        if lock_created:
            fsync_directory(context.artifact_directory, "journal_lock_parent")
        fcntl.flock(descriptor, fcntl.LOCK_EX)
        journal_created = not context.journal_path.exists()
        flags = (
            os.O_RDWR
            | os.O_APPEND
            | os.O_CREAT
            | getattr(os, "O_NOFOLLOW", 0)
        )
        output = os.open(context.journal_path, flags, 0o600)
        try:
            os.fchmod(output, 0o600)
            rows = read_repaired_journal_descriptor(context, output)
            sequence = len(rows) + 1
            receipt = {
                "schema_version": SCHEMA_VERSION,
                "sequence": sequence,
                "recorded_at": utc_now(),
                "manifest_sha256": context.digest,
                "action": validate_identity(action, "journal_action_invalid"),
                "status": validate_identity(status, "journal_status_invalid"),
                "target": validate_identity(target, "journal_target_invalid"),
            }
            payload = (json.dumps(receipt, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n").encode("utf-8")
            written = 0
            while written < len(payload):
                count = os.write(output, payload[written:])
                if count <= 0:
                    fail("journal_write_failed")
                written += count
            os.fsync(output)
        finally:
            os.close(output)
        if journal_created:
            fsync_directory(context.artifact_directory, "journal_entry_parent")
    finally:
        fcntl.flock(descriptor, fcntl.LOCK_UN)
        os.close(descriptor)


def save_state(context, phase, snapshot):
    if phase not in PHASES:
        fail("state_phase_invalid")
    state = {
        "schema_version": SCHEMA_VERSION,
        "manifest_sha256": context.digest,
        "run_id": context.manifest["run_id"],
        "phase": phase,
        "updated_at": utc_now(),
        "standing_snapshot": snapshot,
    }
    write_atomic(
        context.state_path,
        json.dumps(state, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n",
    )
    return state


def load_state(context, allowed_phases=None):
    if not context.state_path.exists():
        fail("orchestrator_state_absent")
    state = load_json(context.state_path, "orchestrator_state_invalid")
    if (
        state.get("schema_version") != SCHEMA_VERSION
        or state.get("manifest_sha256") != context.digest
        or state.get("run_id") != context.manifest["run_id"]
        or state.get("phase") not in PHASES
        or not isinstance(state.get("standing_snapshot"), dict)
    ):
        fail("orchestrator_state_invalid")
    if allowed_phases is not None and state["phase"] not in allowed_phases:
        fail("orchestrator_phase_invalid")
    return state
