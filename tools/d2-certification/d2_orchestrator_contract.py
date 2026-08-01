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
    load_verified_manifest,
    sha256_file,
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


def write_atomic(path, payload, mode=0o600):
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    descriptor = os.open(
        temporary,
        os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0),
        mode,
    )
    try:
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
    os.chmod(path, mode)


def load_json(path, code):
    try:
        metadata = path.lstat()
        if not stat.S_ISREG(metadata.st_mode) or path.is_symlink():
            fail(code)
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError):
        fail(code)


def validate_identity(value, code):
    if not isinstance(value, str) or not IDENTITY_PATTERN.fullmatch(value):
        fail(code)
    return value


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


def load_context(raw_manifest):
    manifest_path, manifest, digest = load_verified_manifest(raw_manifest)
    run_id = manifest["run_id"]
    expected_root = pathlib.Path(f"/private/tmp/starring-d2-{run_id}")
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
    }
    if len(ports) != 4 or ports.intersection(PROTECTED_PORTS):
        fail("isolated_port_contract_invalid")
    if require_available:
        for port in ports:
            if not platform.port_available(port):
                fail("isolated_port_busy")


def append_journal(context, action, status, target):
    context.artifact_directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    descriptor = os.open(
        context.lock_path,
        os.O_RDWR | os.O_CREAT | getattr(os, "O_NOFOLLOW", 0),
        0o600,
    )
    try:
        fcntl.flock(descriptor, fcntl.LOCK_EX)
        sequence = 1
        if context.journal_path.exists():
            metadata = context.journal_path.lstat()
            if not stat.S_ISREG(metadata.st_mode) or context.journal_path.is_symlink():
                fail("journal_invalid")
            with context.journal_path.open("r", encoding="utf-8") as handle:
                sequence += sum(1 for _ in handle)
        receipt = {
            "schema_version": SCHEMA_VERSION,
            "sequence": sequence,
            "recorded_at": utc_now(),
            "manifest_sha256": context.digest,
            "action": validate_identity(action, "journal_action_invalid"),
            "status": validate_identity(status, "journal_status_invalid"),
            "target": validate_identity(target, "journal_target_invalid"),
        }
        flags = os.O_WRONLY | os.O_APPEND | os.O_CREAT | getattr(os, "O_NOFOLLOW", 0)
        output = os.open(context.journal_path, flags, 0o600)
        try:
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
