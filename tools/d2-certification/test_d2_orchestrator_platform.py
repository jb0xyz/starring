import copy
import ctypes
import hashlib
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest
from types import SimpleNamespace
from unittest import mock


DIRECTORY = pathlib.Path(__file__).parent
sys.path.insert(0, str(DIRECTORY))

import d2_orchestrator_contract as CONTRACT
import d2_orchestrator_platform as PLATFORM


INSTANCE_ID = "d2ti-0123456789abcdef0123456789abcdef"


def canonical_inventory(context, history=None):
    if history is None:
        history = [
            {"kind": "channel", "resource_id": "1524810437118525560", "state": "created"},
            {
                "kind": "message",
                "resource_id": "1524810437118525561",
                "channel_id": "1524810437118525560",
                "state": "created",
            },
            {"kind": "role", "resource_id": "1524810437118525562", "state": "deleted"},
        ]
    created = []
    deleted = []
    active = []
    for entry in history:
        identity = {"kind": entry["kind"], "resource_id": entry["resource_id"]}
        if entry["kind"] == "message":
            identity["channel_id"] = entry["channel_id"]
        created.append(identity)
        if entry["state"] == "deleted":
            deleted.append(identity)
        else:
            active.append(identity)
    payload = {
        "version": 1,
        "kind": PLATFORM.TRANSPORT_RESOURCE_INVENTORY_KIND,
        "instance_id": INSTANCE_ID,
        "run_id": context.manifest["run_id"],
        "guild_id": context.manifest["discord"]["guild_id"],
        "hub_channel_id": context.manifest["discord"]["hub_channel_id"],
        "actor_id": context.manifest["discord"]["actor_id"],
        "bot_user_id": context.manifest["discord"]["bot_user_id"],
        "history_limit": 128,
        "history": history,
        "created": created,
        "deleted": deleted,
        "active": active,
    }
    encoded = json.dumps(payload, ensure_ascii=True, separators=(",", ":")).encode(
        "ascii"
    )
    return {**payload, "digest_sha256": hashlib.sha256(encoded).hexdigest()}


def discord_response(status, body=b""):
    if not isinstance(body, bytes):
        body = json.dumps(body, separators=(",", ":")).encode("utf-8")
    return subprocess.CompletedProcess([], 0, body + b"\n" + str(status).encode(), b"")


class ExchangePlatform(PLATFORM.Platform):
    def __init__(self, value):
        self.value = value

    def _transport_control_exchange(
        self, context, command, fields, timeout_seconds, allow_unavailable
    ):
        return copy.deepcopy(self.value)


class DiscordPlatform(PLATFORM.Platform):
    def __init__(self, responses):
        self.responses = list(responses)
        self.calls = []

    def run(self, arguments, input_bytes=None, timeout=30, environment=None):
        self.calls.append(
            {
                "arguments": [str(value) for value in arguments],
                "input_bytes": input_bytes,
                "timeout": timeout,
                "environment": environment,
            }
        )
        if not self.responses:
            raise AssertionError("unexpected_discord_request")
        return self.responses.pop(0)


class WorkerPlatform(PLATFORM.Platform):
    def __init__(self, response):
        self.response = response

    def run(self, arguments, input_bytes=None, timeout=30, environment=None):
        return self.response


class CommandPlatform(PLATFORM.Platform):
    def __init__(self, responses):
        self.responses = list(responses)
        self.calls = []

    def run(self, arguments, input_bytes=None, timeout=30, environment=None):
        self.calls.append([str(argument) for argument in arguments])
        if not self.responses:
            raise AssertionError("unexpected_platform_command")
        return self.responses.pop(0)


def command_response(returncode, stdout=b"", stderr=b""):
    return subprocess.CompletedProcess([], returncode, stdout, stderr)


class FakeCFunction:
    def __init__(self, callback):
        self.callback = callback
        self.argtypes = None
        self.restype = None

    def __call__(self, *arguments):
        return self.callback(*arguments)


def pointer_value(value):
    return value.value if isinstance(value, ctypes.c_void_p) else value


class FakeKeychainFrameworkState:
    def __init__(self):
        self.name = (b"service", b"account")
        self.items = {self.name: 101}
        self.references = {101: b"persistent-reference-a"}
        self.find_status = 0
        self.find_output_on_error = False
        self.reference_status = 0
        self.reference_output_on_error = False
        self.delete_status = 0
        self.swap_before_delete = None
        self.deleted = []
        self.released = []
        self.data_handles = {}
        self.buffers = {}
        self.next_data_handle = 1001


class FakeSecurityFramework:
    def __init__(self, state):
        self.state = state
        self.SecKeychainFindGenericPassword = FakeCFunction(self.find)
        self.SecKeychainItemCreatePersistentReference = FakeCFunction(
            self.create_reference
        )
        self.SecKeychainItemDelete = FakeCFunction(self.delete)

    def find(
        self,
        keychain,
        service_length,
        service,
        account_length,
        account,
        password_length,
        password_data,
        item_output,
    ):
        if self.state.find_status != 0:
            if self.state.find_output_on_error:
                item_output._obj.value = 101
            return self.state.find_status
        name = (service[:service_length], account[:account_length])
        item = self.state.items.get(name)
        if item is None:
            return -25300
        item_output._obj.value = item
        return 0

    def create_reference(self, item, persistent_output):
        if self.state.reference_status != 0:
            if self.state.reference_output_on_error:
                handle = self.state.next_data_handle
                self.state.next_data_handle += 1
                self.state.data_handles[handle] = b"error-reference"
                persistent_output._obj.value = handle
            return self.state.reference_status
        item_value = pointer_value(item)
        reference = self.state.references.get(item_value)
        if reference is None:
            return -50
        handle = self.state.next_data_handle
        self.state.next_data_handle += 1
        self.state.data_handles[handle] = reference
        persistent_output._obj.value = handle
        return 0

    def delete(self, item):
        item_value = pointer_value(item)
        if self.state.swap_before_delete is not None:
            replacement_item, replacement_reference = self.state.swap_before_delete
            self.state.items[self.state.name] = replacement_item
            self.state.references[replacement_item] = replacement_reference
            self.state.swap_before_delete = None
        self.state.deleted.append(item_value)
        if self.state.delete_status != 0:
            return self.state.delete_status
        for name, current in list(self.state.items.items()):
            if current == item_value:
                del self.state.items[name]
        self.state.references.pop(item_value, None)
        return 0


class FakeCoreFoundationFramework:
    def __init__(self, state):
        self.state = state
        self.CFDataGetLength = FakeCFunction(self.data_length)
        self.CFDataGetBytePtr = FakeCFunction(self.data_pointer)
        self.CFRelease = FakeCFunction(self.release)

    def data_length(self, data):
        return len(self.state.data_handles[pointer_value(data)])

    def data_pointer(self, data):
        handle = pointer_value(data)
        buffer = ctypes.create_string_buffer(self.state.data_handles[handle])
        self.state.buffers[handle] = buffer
        return ctypes.addressof(buffer)

    def release(self, value):
        self.state.released.append(pointer_value(value))


class PlatformKeychainReferenceTests(unittest.TestCase):
    def setUp(self):
        self.state = FakeKeychainFrameworkState()
        self.security = FakeSecurityFramework(self.state)
        self.core = FakeCoreFoundationFramework(self.state)
        self.cdll = mock.patch.object(
            PLATFORM.ctypes,
            "CDLL",
            side_effect=lambda path: (
                self.security if "Security.framework" in path else self.core
            ),
        )
        self.cdll.start()
        self.platform = PLATFORM.Platform()

    def tearDown(self):
        self.cdll.stop()

    def assert_platform_failure(self, code, operation):
        with self.assertRaisesRegex(CONTRACT.OrchestratorError, f"^{code}$"):
            operation()

    def test_identity_hashes_persistent_reference_and_releases_both_objects(self):
        expected = hashlib.sha256(b"persistent-reference-a").hexdigest()
        self.assertEqual(
            self.platform.keychain_item_identity("service", "account"), expected
        )
        self.assertEqual(self.state.released, [1001, 101])
        self.assertIsNotNone(self.security.SecKeychainFindGenericPassword.argtypes)
        self.assertEqual(
            self.security.SecKeychainItemDelete.restype, ctypes.c_int32
        )

    def test_exact_delete_cannot_delete_name_replacement(self):
        expected = hashlib.sha256(b"persistent-reference-a").hexdigest()
        self.state.swap_before_delete = (202, b"persistent-reference-b")
        self.platform.keychain_delete_exact("service", "account", expected)
        self.assertEqual(self.state.deleted, [101])
        self.assertEqual(self.state.items[self.state.name], 202)
        self.assertEqual(self.state.references[202], b"persistent-reference-b")
        self.assertEqual(self.state.released, [1001, 101])

    def test_exact_delete_rejects_identity_drift_without_delete(self):
        self.assert_platform_failure(
            "keychain_reference_identity_drift",
            lambda: self.platform.keychain_delete_exact(
                "service", "account", "f" * 64
            ),
        )
        self.assertEqual(self.state.deleted, [])
        self.assertEqual(self.state.items[self.state.name], 101)
        self.assertEqual(self.state.released, [1001, 101])

    def test_not_found_and_framework_errors_fail_closed_and_release_refs(self):
        self.state.items.clear()
        self.assertIsNone(
            self.platform.keychain_item_identity("service", "account")
        )
        self.state.find_status = -50
        self.state.find_output_on_error = True
        self.assert_platform_failure(
            "keychain_reference_observation_failed",
            lambda: self.platform.keychain_item_identity("service", "account"),
        )
        self.state.find_status = 0
        self.state.find_output_on_error = False
        self.state.items[self.state.name] = 101
        self.state.reference_status = -50
        self.state.reference_output_on_error = True
        self.assert_platform_failure(
            "keychain_reference_observation_failed",
            lambda: self.platform.keychain_item_identity("service", "account"),
        )
        self.assertEqual(self.state.released, [101, 1001, 101])

    def test_delete_error_releases_reference_and_item(self):
        expected = hashlib.sha256(b"persistent-reference-a").hexdigest()
        self.state.delete_status = -50
        self.assert_platform_failure(
            "keychain_reference_delete_failed",
            lambda: self.platform.keychain_delete_exact(
                "service", "account", expected
            ),
        )
        self.assertEqual(self.state.deleted, [101])
        self.assertEqual(self.state.items[self.state.name], 101)
        self.assertEqual(self.state.released, [1001, 101])

    def test_name_based_delete_is_forbidden_without_command_fallback(self):
        self.assert_platform_failure(
            "keychain_name_delete_forbidden",
            lambda: self.platform.keychain_delete("service", "account"),
        )

    def test_production_cleanup_has_no_name_based_delete_calls(self):
        for name in (
            "isolated_orchestrator.py",
            "d2_legacy_substrate_recovery.py",
        ):
            source = (DIRECTORY / name).read_text(encoding="utf-8")
            self.assertNotIn(".keychain_delete(", source)
            self.assertIn(".keychain_delete_exact(", source)


class PlatformExclusiveRenameTests(unittest.TestCase):
    def test_exclusive_rename_succeeds_only_when_destination_is_absent(self):
        with tempfile.TemporaryDirectory() as temporary:
            parent = pathlib.Path(temporary)
            source = parent / "source"
            destination = parent / "destination"
            source.mkdir()
            descriptor = os.open(
                parent,
                os.O_RDONLY | getattr(os, "O_DIRECTORY", 0),
            )
            try:
                PLATFORM.rename_exclusive(
                    descriptor,
                    source.name,
                    descriptor,
                    destination.name,
                )
            finally:
                os.close(descriptor)
            self.assertFalse(source.exists())
            self.assertTrue(destination.is_dir())

    def test_exclusive_rename_preserves_preexisting_directory_and_symlink(self):
        for destination_kind in ("directory", "symlink"):
            with self.subTest(destination_kind=destination_kind):
                with tempfile.TemporaryDirectory() as temporary:
                    parent = pathlib.Path(temporary)
                    source = parent / "source"
                    destination = parent / "destination"
                    target = parent / "target"
                    source.mkdir()
                    target.mkdir()
                    if destination_kind == "directory":
                        destination.mkdir()
                    else:
                        destination.symlink_to(target, target_is_directory=True)
                    descriptor = os.open(
                        parent,
                        os.O_RDONLY | getattr(os, "O_DIRECTORY", 0),
                    )
                    try:
                        with self.assertRaises(OSError):
                            PLATFORM.rename_exclusive(
                                descriptor,
                                source.name,
                                descriptor,
                                destination.name,
                            )
                    finally:
                        os.close(descriptor)
                    self.assertTrue(source.is_dir())
                    if destination_kind == "directory":
                        self.assertTrue(destination.is_dir())
                        self.assertFalse(destination.is_symlink())
                    else:
                        self.assertTrue(destination.is_symlink())


class PlatformAbsenceObservationTests(unittest.TestCase):
    def assert_platform_failure(self, code, operation):
        with self.assertRaisesRegex(CONTRACT.OrchestratorError, f"^{code}$"):
            operation()

    def test_launchd_absence_accepts_only_exact_not_found(self):
        self.assertTrue(CommandPlatform([command_response(113)]).launchd_absent("job"))
        self.assertFalse(CommandPlatform([command_response(0)]).launchd_absent("job"))
        self.assert_platform_failure(
            "launchd_observation_failed",
            lambda: CommandPlatform([command_response(1)]).launchd_absent("job"),
        )

    def test_postgres_absence_requires_exact_status_process_and_open_file_absence(self):
        with tempfile.TemporaryDirectory() as temporary:
            cluster = pathlib.Path(temporary)
            absent = CommandPlatform(
                [
                    command_response(3),
                    command_response(0, b"  7 /usr/bin/python\n"),
                    command_response(1),
                ]
            )
            self.assertTrue(absent.postgres_absent(cluster))
            self.assertFalse(
                CommandPlatform([command_response(0)]).postgres_absent(cluster)
            )
            process = CommandPlatform(
                [
                    command_response(3),
                    command_response(0, f"  9 postgres -D {cluster}\n".encode()),
                ]
            )
            self.assertFalse(process.postgres_absent(cluster))
            open_file = CommandPlatform(
                [
                    command_response(3),
                    command_response(0, b"  7 /usr/bin/python\n"),
                    command_response(0, b"p9\nf1\n"),
                ]
            )
            self.assertFalse(open_file.postgres_absent(cluster))
            (cluster / "postmaster.pid").write_text("123\n", encoding="ascii")
            self.assertFalse(
                CommandPlatform([command_response(3)]).postgres_absent(cluster)
            )

    def test_postgres_absence_fails_closed_on_observation_errors(self):
        with tempfile.TemporaryDirectory() as temporary:
            cluster = pathlib.Path(temporary)
            self.assert_platform_failure(
                "postgres_observation_failed",
                lambda: CommandPlatform([command_response(1)]).postgres_absent(
                    cluster
                ),
            )
            self.assert_platform_failure(
                "postgres_process_observation_failed",
                lambda: CommandPlatform(
                    [command_response(3), command_response(1)]
                ).postgres_absent(cluster),
            )
            self.assert_platform_failure(
                "postgres_open_file_observation_failed",
                lambda: CommandPlatform(
                    [
                        command_response(3),
                        command_response(0),
                        command_response(2),
                    ]
                ).postgres_absent(cluster),
            )

    def test_postgres_cluster_identity_is_exact(self):
        output = b"Database system identifier:           7669853998318333589\n"
        platform = CommandPlatform([command_response(0, output)])
        self.assertEqual(
            platform.postgres_cluster_identity(pathlib.Path("/tmp/cluster")),
            "7669853998318333589",
        )
        self.assert_platform_failure(
            "postgres_cluster_identity_observation_failed",
            lambda: CommandPlatform([command_response(0, b"invalid\n")])
            .postgres_cluster_identity(pathlib.Path("/tmp/cluster")),
        )


def worker_context():
    return SimpleNamespace(
        manifest={
            "services": {"worker": {"port": 28181}},
            "keychain_services": {"worker": "starring.d2.worker"},
            "authoring": {
                "provider": "codex_chatgpt",
                "model": "gpt-5.6-luna",
                "reasoning_effort": "medium",
                "auth_mode": "chatgpt",
            },
            "source_trees": {"codex_worker": {"sha256": "a" * 64}},
        }
    )


def worker_health(**overrides):
    value = {
        "schema_version": 1,
        "status": "ok",
        "provider": "codex_chatgpt",
        "model": "gpt-5.6-luna",
        "reasoning_effort": "medium",
        "auth_mode": "chatgpt",
        "codex_cli_version": "codex-cli 1.2.3",
        "instance_id": "worker-0123456789abcdef",
        "worker_source_sha256": "a" * 64,
        "concurrency_limit": 1,
        "queue_capacity": 4,
        "request_timeout_ms": 55000,
        "active_requests": 0,
        "queued_requests": 0,
        "accepted_requests_total": 7,
        "settled_requests_total": 7,
    }
    value.update(overrides)
    return value


def worker_response(value, status=200):
    body = json.dumps(value, separators=(",", ":")).encode("utf-8")
    return subprocess.CompletedProcess([], 0, body + b"\n" + str(status).encode(), b"")


class PlatformWorkerHealthTests(unittest.TestCase):
    def assert_platform_failure(self, code, operation):
        with self.assertRaisesRegex(CONTRACT.OrchestratorError, f"^{code}$"):
            operation()

    def test_worker_snapshot_is_exact_and_status_reuses_probe(self):
        context = worker_context()
        health = worker_health()
        platform = WorkerPlatform(worker_response(health))
        self.assertEqual(platform.worker_health_status(context), 200)
        self.assertEqual(platform.worker_health_snapshot(context), health)

    def test_worker_snapshot_rejects_duplicate_and_type_confused_fields(self):
        context = worker_context()
        duplicate = (
            b'{"schema_version":1,"schema_version":1}\n200'
        )
        invalid = [
            subprocess.CompletedProcess([], 0, duplicate, b""),
            worker_response(worker_health(schema_version=True)),
            worker_response(worker_health(concurrency_limit=True)),
            worker_response(worker_health(instance_id="worker identity")),
            worker_response(worker_health(worker_source_sha256="f" * 63)),
        ]
        for response in invalid:
            with self.subTest(response=response.stdout):
                self.assert_platform_failure(
                    "worker_health_output_invalid"
                    if response.stdout == duplicate
                    else "worker_health_identity_invalid",
                    lambda response=response: WorkerPlatform(response).worker_health_snapshot(
                        context
                    ),
                )

    def test_worker_snapshot_fails_closed_when_not_ready(self):
        context = worker_context()
        platform = WorkerPlatform(worker_response({"error": "unready"}, status=503))
        self.assertEqual(platform.worker_health_status(context), 503)
        self.assert_platform_failure(
            "worker_health_unready", lambda: platform.worker_health_snapshot(context)
        )


class PlatformResourceTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        root = pathlib.Path(self.temporary.name)
        artifact_directory = root / "artifacts"
        artifact_directory.mkdir(mode=0o700)
        step_three = artifact_directory / "step-03-evidence.json"
        step_three.write_text(
            json.dumps({"transport_instance_id": INSTANCE_ID}), encoding="utf-8"
        )
        step_three.chmod(0o600)
        self.context = SimpleNamespace(
            root=root,
            artifact_directory=artifact_directory,
            manifest={
                "run_id": "d2-20260804t010203z-012345abcdef",
                "discord": {
                    "guild_id": "1524810437118525551",
                    "hub_channel_id": "1524810437118525554",
                    "application_id": "1524810437118525552",
                    "bot_user_id": "1524810437118525553",
                    "actor_id": "1056857223529250906",
                },
                "external_keychain": {
                    "discord_bot_token": {
                        "service": "starring.d2.credentials",
                        "account": "discord.bot-token",
                    }
                },
                "services": {"transport": {"http_port": 29102}},
            },
        )
        self.inventory = canonical_inventory(self.context)

    def tearDown(self):
        self.temporary.cleanup()

    def assert_platform_failure(self, code, operation):
        with self.assertRaisesRegex(CONTRACT.OrchestratorError, f"^{code}$"):
            operation()

    def test_resource_inventory_is_exact_sorted_digest_and_instance_bound(self):
        platform = ExchangePlatform(
            {"ok": True, "resource_inventory": self.inventory}
        )
        observed = platform.transport_control(self.context, "resource_inventory")
        self.assertEqual(observed, self.inventory)
        self.assertEqual(observed["instance_id"], INSTANCE_ID)
        self.assertEqual(
            [entry["kind"] for entry in observed["created"]],
            ["channel", "message", "role"],
        )

    def test_resource_inventory_rejects_schema_state_and_digest_drift(self):
        cases = []
        extra = copy.deepcopy(self.inventory)
        extra["unexpected"] = True
        cases.append(extra)
        bad_digest = copy.deepcopy(self.inventory)
        bad_digest["digest_sha256"] = "f" * 64
        cases.append(bad_digest)
        wrong_instance = copy.deepcopy(self.inventory)
        wrong_instance["instance_id"] = "d2ti-fedcba9876543210fedcba9876543210"
        cases.append(wrong_instance)
        unsorted = canonical_inventory(
            self.context, list(reversed(copy.deepcopy(self.inventory["history"])))
        )
        cases.append(unsorted)
        wrong_projection = copy.deepcopy(self.inventory)
        wrong_projection["active"] = wrong_projection["active"][:-1]
        cases.append(wrong_projection)
        parent_deleted = canonical_inventory(
            self.context,
            [
                {
                    "kind": "channel",
                    "resource_id": "1524810437118525560",
                    "state": "deleted",
                },
                {
                    "kind": "message",
                    "resource_id": "1524810437118525561",
                    "channel_id": "1524810437118525560",
                    "state": "created",
                },
            ],
        )
        cases.append(parent_deleted)
        for value in cases:
            with self.subTest(value=value):
                platform = ExchangePlatform(
                    {"ok": True, "resource_inventory": value}
                )
                self.assert_platform_failure(
                    "transport_control_response_invalid",
                    lambda: platform.transport_control(
                        self.context, "resource_inventory"
                    ),
                )

    def test_resource_inventory_rejects_protected_and_duplicate_resource_ids(self):
        for protected in (
            self.context.manifest["discord"]["guild_id"],
            self.context.manifest["discord"]["hub_channel_id"],
            self.context.manifest["discord"]["application_id"],
            self.context.manifest["discord"]["actor_id"],
            self.context.manifest["discord"]["bot_user_id"],
        ):
            inventory = canonical_inventory(
                self.context,
                [{"kind": "role", "resource_id": protected, "state": "created"}],
            )
            self.assertFalse(
                PLATFORM.Platform()._transport_resource_inventory_valid(
                    self.context, inventory
                )
            )
        duplicate = canonical_inventory(
            self.context,
            [
                {"kind": "channel", "resource_id": "1524810437118525560", "state": "created"},
                {"kind": "role", "resource_id": "1524810437118525560", "state": "created"},
            ],
        )
        self.assertFalse(
            PLATFORM.Platform()._transport_resource_inventory_valid(
                self.context, duplicate
            )
        )

    def test_resource_inventory_requires_private_pinned_evidence(self):
        path = self.context.artifact_directory / "step-03-evidence.json"
        path.chmod(0o644)
        self.assert_platform_failure(
            "transport_instance_evidence_invalid",
            lambda: PLATFORM.Platform()._transport_resource_inventory_valid(
                self.context, self.inventory
            ),
        )

    def test_role_observation_is_redacted_and_uses_stdin_curl_config(self):
        resource = {"kind": "role", "resource_id": "1524810437118525562"}
        platform = DiscordPlatform(
            [
                discord_response(
                    200,
                    [
                        {"id": "1524810437118525562", "name": "private"},
                        {"id": "1524810437118525570", "name": "other"},
                    ],
                )
            ]
        )
        evidence = platform.discord_observe_resource(
            self.context, resource, self.inventory
        )
        self.assertEqual(
            set(evidence),
            {
                "schema_version",
                "kind",
                "transport_instance_id",
                "inventory_digest_sha256",
                "resource_kind",
                "resource_id",
                "channel_id",
                "http_status",
                "discord_code",
                "exists",
            },
        )
        self.assertTrue(evidence["exists"])
        self.assertEqual(evidence["http_status"], 200)
        self.assertIsNone(evidence["discord_code"])
        self.assertNotIn("name", evidence)
        call = platform.calls[0]
        arguments = call["arguments"]
        script = arguments[2]
        self.assertIn("--config -", script)
        self.assertIn("Authorization: Bot %s", script)
        self.assertNotIn("not-a-real-token", "\n".join(arguments))
        self.assertEqual(arguments[5], "starring.d2.credentials")
        self.assertEqual(arguments[6], "discord.bot-token")
        self.assertIsNone(call["input_bytes"])
        self.assertIsNone(call["environment"])
        self.assertEqual(platform.responses, [])

    def test_proxy_delete_is_exact_loopback_http_and_proxy_disabled(self):
        resource = {
            "kind": "message",
            "resource_id": "1524810437118525561",
            "channel_id": "1524810437118525560",
        }
        platform = DiscordPlatform([discord_response(204)])
        evidence = platform.discord_delete_resource_through_transport(
            self.context, resource, self.inventory
        )
        self.assertEqual(evidence["http_status"], 204)
        self.assertTrue(evidence["deleted"])
        arguments = platform.calls[0]["arguments"]
        self.assertEqual(
            arguments[7],
            "http://127.0.0.1:29102/api/v10/channels/"
            "1524810437118525560/messages/1524810437118525561",
        )
        self.assertEqual(arguments[12], "=http")
        self.assertIn("--noproxy '*'", arguments[2])
        self.assertIn("--proxy ''", arguments[2])
        self.assertIn("--max-redirs 0", arguments[2])
        self.assertIn("--config -", arguments[2])
        self.assertNotIn("Bot secret", "\n".join(arguments))

    def test_proxy_delete_requires_active_manifest_owned_resource(self):
        deleted_role = {
            "kind": "role",
            "resource_id": "1524810437118525562",
        }
        self.assert_platform_failure(
            "discord_resource_not_active",
            lambda: DiscordPlatform([]).discord_delete_resource_through_transport(
                self.context, deleted_role, self.inventory
            ),
        )
        active_role_inventory = canonical_inventory(
            self.context,
            [
                {
                    "kind": "role",
                    "resource_id": "1524810437118525562",
                    "state": "created",
                }
            ],
        )
        wrong_port = copy.deepcopy(self.context.manifest["services"])
        self.context.manifest["services"]["transport"]["http_port"] = 443
        self.assert_platform_failure(
            "discord_resource_proxy_request_invalid",
            lambda: DiscordPlatform([]).discord_delete_resource_through_transport(
                self.context, deleted_role, active_role_inventory
            ),
        )
        self.context.manifest["services"] = wrong_port

    def test_role_observation_can_confirm_absence_from_successful_list(self):
        resource = {"kind": "role", "resource_id": "1524810437118525562"}
        platform = DiscordPlatform(
            [discord_response(200, [{"id": "1524810437118525570"}])]
        )
        evidence = platform.discord_observe_resource(
            self.context, resource, self.inventory
        )
        self.assertFalse(evidence["exists"])
        self.assertEqual(evidence["http_status"], 200)

    def test_channel_observation_accepts_only_exact_unknown_channel(self):
        resource = {"kind": "channel", "resource_id": "1524810437118525560"}
        platform = DiscordPlatform(
            [discord_response(404, {"message": "Unknown Channel", "code": 10003})]
        )
        evidence = platform.discord_observe_resource(
            self.context, resource, self.inventory
        )
        self.assertFalse(evidence["exists"])
        self.assertEqual(evidence["discord_code"], 10003)
        for body in (
            {"message": "Unknown Message", "code": 10008},
            {"message": "Unknown Channel", "code": 10003, "extra": True},
        ):
            invalid = DiscordPlatform([discord_response(404, body)])
            self.assert_platform_failure(
                "discord_resource_unknown_response_invalid",
                lambda: invalid.discord_observe_resource(
                    self.context, resource, self.inventory
                ),
            )

    def test_channel_delete_requires_bound_success_and_absence_confirmation(self):
        resource = {"kind": "channel", "resource_id": "1524810437118525560"}
        platform = DiscordPlatform(
            [
                discord_response(
                    200,
                    {
                        "id": resource["resource_id"],
                        "guild_id": self.context.manifest["discord"]["guild_id"],
                        "name": "private",
                    },
                ),
                discord_response(
                    404, {"message": "Unknown Channel", "code": 10003}
                ),
            ]
        )
        evidence = platform.discord_delete_resource(
            self.context, resource, self.inventory
        )
        self.assertEqual(
            set(evidence),
            {
                "schema_version",
                "kind",
                "transport_instance_id",
                "inventory_digest_sha256",
                "resource_kind",
                "resource_id",
                "channel_id",
                "delete_http_status",
                "delete_discord_code",
                "observe_http_status",
                "observe_discord_code",
                "deleted",
                "exists",
            },
        )
        self.assertTrue(evidence["deleted"])
        self.assertFalse(evidence["exists"])
        self.assertEqual(evidence["delete_http_status"], 200)
        self.assertEqual(evidence["observe_discord_code"], 10003)
        self.assertIn(
            "/channels/1524810437118525560",
            platform.calls[0]["arguments"][7],
        )

    def test_message_delete_accepts_channel_terminalization(self):
        resource = {
            "kind": "message",
            "resource_id": "1524810437118525561",
            "channel_id": "1524810437118525560",
        }
        unknown_channel = {"message": "Unknown Channel", "code": 10003}
        platform = DiscordPlatform(
            [discord_response(404, unknown_channel), discord_response(404, unknown_channel)]
        )
        evidence = platform.discord_delete_resource(
            self.context, resource, self.inventory
        )
        self.assertFalse(evidence["deleted"])
        self.assertFalse(evidence["exists"])
        self.assertEqual(evidence["delete_discord_code"], 10003)
        self.assertEqual(evidence["observe_discord_code"], 10003)

    def test_delete_rejects_reappearing_or_wrong_scope_resource(self):
        role = {"kind": "role", "resource_id": "1524810437118525562"}
        reappearing = DiscordPlatform(
            [
                discord_response(204),
                discord_response(200, [{"id": role["resource_id"]}]),
            ]
        )
        self.assert_platform_failure(
            "discord_resource_delete_not_confirmed",
            lambda: reappearing.discord_delete_resource(
                self.context, role, self.inventory
            ),
        )
        unknown = {"kind": "role", "resource_id": "1524810437118525599"}
        self.assert_platform_failure(
            "discord_resource_not_manifest_owned",
            lambda: DiscordPlatform([]).discord_observe_resource(
                self.context, unknown, self.inventory
            ),
        )

    def test_discord_request_is_https_time_body_and_status_bounded(self):
        platform = DiscordPlatform([])
        self.assert_platform_failure(
            "discord_resource_request_invalid",
            lambda: platform._discord_request(
                self.context, "GET", "http://discord.com/api/v10/channels/1", 10
            ),
        )
        self.assert_platform_failure(
            "discord_resource_request_invalid",
            lambda: platform._discord_request(
                self.context,
                "GET",
                "https://discord.com/api/v10/channels/1",
                31,
            ),
        )
        resource = {"kind": "channel", "resource_id": "1524810437118525560"}
        forbidden = DiscordPlatform([discord_response(403, {"code": 50013})])
        self.assert_platform_failure(
            "discord_resource_status_invalid",
            lambda: forbidden.discord_observe_resource(
                self.context, resource, self.inventory
            ),
        )
        oversized = DiscordPlatform(
            [
                subprocess.CompletedProcess(
                    [],
                    0,
                    b"x" * (PLATFORM.MAX_DISCORD_RESPONSE_BYTES + 5),
                    b"",
                )
            ]
        )
        self.assert_platform_failure(
            "discord_resource_response_invalid",
            lambda: oversized.discord_observe_resource(
                self.context, resource, self.inventory
            ),
        )

    def test_success_payloads_are_identity_and_guild_bound(self):
        channel = {"kind": "channel", "resource_id": "1524810437118525560"}
        wrong_guild = DiscordPlatform(
            [
                discord_response(
                    200,
                    {
                        "id": channel["resource_id"],
                        "guild_id": "1524810437118525599",
                    },
                )
            ]
        )
        self.assert_platform_failure(
            "discord_resource_success_response_invalid",
            lambda: wrong_guild.discord_observe_resource(
                self.context, channel, self.inventory
            ),
        )
        duplicate = b'{"id":"1524810437118525560","id":"1524810437118525560","guild_id":"1524810437118525551"}'
        duplicate_body = DiscordPlatform([discord_response(200, duplicate)])
        self.assert_platform_failure(
            "discord_resource_response_invalid",
            lambda: duplicate_body.discord_observe_resource(
                self.context, channel, self.inventory
            ),
        )


if __name__ == "__main__":
    unittest.main()
