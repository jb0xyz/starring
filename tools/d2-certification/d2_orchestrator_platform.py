import json
import os
import re
import socket
import stat
import subprocess
import time

from d2_orchestrator_contract import OWNER_ACCOUNT, REQUIRED_PROGRAMS, fail


MAX_TRANSPORT_CONTROL_BYTES = 64 * 1024


class Platform:
    def run(self, arguments, input_bytes=None, timeout=30, environment=None):
        options = {
            "stdout": subprocess.PIPE,
            "stderr": subprocess.PIPE,
            "timeout": timeout,
            "check": False,
            "env": {} if environment is None else environment,
        }
        if input_bytes is None:
            options["stdin"] = subprocess.DEVNULL
        else:
            options["input"] = input_bytes
        try:
            return subprocess.run([str(argument) for argument in arguments], **options)
        except subprocess.TimeoutExpired:
            fail("platform_command_timeout")
        except OSError:
            fail("platform_command_unavailable")

    def executable(self, path):
        try:
            metadata = path.lstat()
        except OSError:
            return False
        return stat.S_ISREG(metadata.st_mode) and not path.is_symlink() and os.access(path, os.X_OK)

    def port_available(self, port):
        listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        try:
            listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 0)
            listener.bind(("127.0.0.1", port))
            return True
        except OSError:
            return False
        finally:
            listener.close()

    def launchd_loaded(self, label):
        result = self.run(
            [REQUIRED_PROGRAMS["launchctl"], "print", f"gui/{os.getuid()}/{label}"],
            timeout=5,
        )
        return result.returncode == 0

    def keychain_present(self, service, account):
        result = self.run(
            [
                REQUIRED_PROGRAMS["security"],
                "find-generic-password",
                "-s",
                service,
                "-a",
                account,
            ],
            timeout=10,
        )
        if result.returncode == 0:
            return True
        if result.returncode == 44:
            return False
        fail("keychain_probe_failed")

    def keychain_write_new(self, service, account, value):
        if self.keychain_present(service, account):
            fail("keychain_identity_busy")
        line = bytearray(b"add-generic-password -s ")
        line.extend(service.encode("ascii"))
        line.extend(b" -a ")
        line.extend(account.encode("ascii"))
        line.extend(b" -X ")
        line.extend(value.hex().encode("ascii"))
        line.extend(b"\n")
        try:
            result = self.run(
                [REQUIRED_PROGRAMS["security"], "-i"],
                input_bytes=bytes(line),
                timeout=10,
            )
        finally:
            for index in range(len(line)):
                line[index] = 0
        if result.returncode != 0 or not self.keychain_present(service, account):
            fail("keychain_write_failed")

    def keychain_delete(self, service, account):
        result = self.run(
            [
                REQUIRED_PROGRAMS["security"],
                "delete-generic-password",
                "-s",
                service,
                "-a",
                account,
            ],
            timeout=10,
        )
        if result.returncode not in (0, 44):
            fail("keychain_delete_failed")

    def keychain_owner_matches(self, service, expected):
        result = self.run(
            [
                REQUIRED_PROGRAMS["security"],
                "find-generic-password",
                "-w",
                "-s",
                service,
                "-a",
                OWNER_ACCOUNT,
            ],
            timeout=10,
        )
        if result.returncode != 0:
            return False
        value = result.stdout.rstrip(b"\r\n")
        return value == expected.encode("ascii")

    def postgres_running(self, cluster_root):
        result = self.run(
            [REQUIRED_PROGRAMS["pg_ctl"], "-D", cluster_root, "status"], timeout=10
        )
        return result.returncode == 0

    def initdb(self, cluster_root):
        result = self.run(
            [
                REQUIRED_PROGRAMS["initdb"],
                "-D",
                cluster_root,
                "--username=starring_cluster_admin",
                "--encoding=UTF8",
                "--locale=C",
                "--data-checksums",
                "--auth-local=reject",
                "--auth-host=reject",
                "--no-instructions",
            ],
            timeout=120,
        )
        if result.returncode != 0:
            fail("postgres_initdb_failed")

    def postgres_start(self, cluster_root, log_path):
        result = self.run(
            [
                REQUIRED_PROGRAMS["pg_ctl"],
                "-D",
                cluster_root,
                "-l",
                log_path,
                "-w",
                "-t",
                "30",
                "start",
            ],
            timeout=45,
            environment={
                "LC_ALL": "C",
                "LANG": "C",
                "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
            },
        )
        if result.returncode != 0:
            fail("postgres_start_failed")

    def postgres_stop(self, cluster_root):
        if not cluster_root.exists() or not self.postgres_running(cluster_root):
            return
        result = self.run(
            [
                REQUIRED_PROGRAMS["pg_ctl"],
                "-D",
                cluster_root,
                "-m",
                "fast",
                "-w",
                "-t",
                "30",
                "stop",
            ],
            timeout=45,
        )
        if result.returncode != 0:
            fail("postgres_stop_failed")

    def bootstrap_database(self, context):
        candidate = context.manifest["candidates"]["db_bootstrap"]["path"]
        result = self.run(
            [
                candidate,
                "--run-id",
                context.manifest["run_id"],
                "--cluster-root",
                context.cluster_root,
                "--socket-directory",
                context.socket_directory,
                "--port",
                str(context.manifest["database"]["port"]),
            ],
            timeout=300,
        )
        if result.returncode != 0 or len(result.stdout) > 16 * 1024:
            fail("database_bootstrap_failed")
        try:
            evidence = json.loads(result.stdout)
        except (UnicodeDecodeError, json.JSONDecodeError):
            fail("database_bootstrap_output_invalid")
        required = {
            "database_system_identifier",
            "migration_count",
            "migration_head",
            "migration_ledger_sha256",
            "relation_count",
            "capability_function_count",
        }
        if not isinstance(evidence, dict) or set(evidence) != required:
            fail("database_bootstrap_output_invalid")
        if (
            not isinstance(evidence["database_system_identifier"], str)
            or not evidence["database_system_identifier"].isdigit()
            or type(evidence["migration_count"]) is not int
            or evidence["migration_count"] <= 0
            or not isinstance(evidence["migration_head"], str)
            or not re.fullmatch(r"[0-9]{12}", evidence["migration_head"])
            or not isinstance(evidence["migration_ledger_sha256"], str)
            or not re.fullmatch(r"[0-9a-f]{64}", evidence["migration_ledger_sha256"])
            or type(evidence["relation_count"]) is not int
            or evidence["relation_count"] <= 0
            or type(evidence["capability_function_count"]) is not int
            or evidence["capability_function_count"] <= 0
        ):
            fail("database_bootstrap_output_invalid")
        return evidence

    def provision_credentials(self, context):
        candidate = context.manifest["candidates"]["sealed_provisioner"]["path"]
        result = self.run(
            [candidate, "provision", "--manifest", context.manifest_path],
            timeout=300,
            environment={"PATH": "/usr/bin:/bin:/usr/sbin:/sbin"},
        )
        if result.returncode != 0 or len(result.stdout) > 16 * 1024:
            fail("sealed_provisioning_failed")
        try:
            evidence = json.loads(result.stdout)
        except (UnicodeDecodeError, json.JSONDecodeError):
            fail("sealed_provisioning_output_invalid")
        required = {
            "outcome",
            "application_credentials",
            "keyrings",
            "worker_credentials",
            "external_credentials_checked",
            "activated_roles",
        }
        if not isinstance(evidence, dict) or set(evidence) != required:
            fail("sealed_provisioning_output_invalid")
        if evidence["outcome"] not in {"fresh", "exact_replay"}:
            fail("sealed_provisioning_output_invalid")
        expected_counts = {
            "application_credentials": 20,
            "keyrings": 3,
            "worker_credentials": 1,
            "external_credentials_checked": 3,
            "activated_roles": 20,
        }
        if any(evidence[field] != expected for field, expected in expected_counts.items()):
            fail("sealed_provisioning_output_invalid")
        return evidence

    def onboard_installation(self, context, principal_id, display_name, installation_id):
        candidate = context.manifest["candidates"]["sealed_provisioner"]["path"]
        result = self.run(
            [
                candidate,
                "onboard",
                "--manifest",
                context.manifest_path,
                "--principal-id",
                principal_id,
                "--display-name",
                display_name,
                "--installation-id",
                installation_id,
            ],
            timeout=120,
            environment={"PATH": "/usr/bin:/bin:/usr/sbin:/sbin"},
        )
        if result.returncode != 0 or len(result.stdout) > 16 * 1024:
            fail("installation_onboarding_failed")
        try:
            evidence = json.loads(result.stdout)
        except (UnicodeDecodeError, json.JSONDecodeError):
            fail("installation_onboarding_output_invalid")
        if (
            not isinstance(evidence, dict)
            or set(evidence) != {"outcome", "installation_id", "principal_id"}
            or evidence["outcome"] not in {"fresh", "exact_replay"}
            or evidence["installation_id"] != installation_id
            or evidence["principal_id"] != principal_id
        ):
            fail("installation_onboarding_output_invalid")
        return evidence

    def postgres_loopback_accepting(self, context):
        result = self.run(
            [
                REQUIRED_PROGRAMS["pg_isready"],
                "-h",
                "127.0.0.1",
                "-p",
                str(context.manifest["database"]["port"]),
                "-d",
                context.manifest["database"]["name"],
                "-t",
                "3",
            ],
            timeout=5,
        )
        return result.returncode == 0

    def launchd_start(self, label, plist_path):
        if self.launchd_loaded(label):
            fail("launchd_label_busy")
        domain = f"gui/{os.getuid()}"
        try:
            result = self.run(
                [REQUIRED_PROGRAMS["launchctl"], "bootstrap", domain, plist_path],
                timeout=20,
            )
            if result.returncode != 0 or not self.launchd_loaded(label):
                fail("launchd_bootstrap_failed")
            result = self.run(
                [REQUIRED_PROGRAMS["launchctl"], "enable", f"{domain}/{label}"],
                timeout=10,
            )
            if result.returncode != 0:
                fail("launchd_enable_failed")
            result = self.run(
                [
                    REQUIRED_PROGRAMS["launchctl"],
                    "kickstart",
                    "-k",
                    f"{domain}/{label}",
                ],
                timeout=20,
            )
            if result.returncode != 0 or not self.launchd_loaded(label):
                fail("launchd_kickstart_failed")
        except BaseException:
            if self.launchd_loaded(label):
                self.launchd_bootout(label)
            raise

    def http_status(self, url, timeout_seconds=3):
        result = self.run(
            [
                REQUIRED_PROGRAMS["curl"],
                "--silent",
                "--show-error",
                "--output",
                "/dev/null",
                "--write-out",
                "%{http_code}",
                "--connect-timeout",
                str(timeout_seconds),
                "--max-time",
                str(timeout_seconds),
                url,
            ],
            timeout=timeout_seconds + 2,
        )
        if result.returncode != 0:
            return 0
        try:
            status = int(result.stdout)
        except ValueError:
            fail("health_probe_output_invalid")
        if status < 100 or status > 599:
            fail("health_probe_output_invalid")
        return status

    def worker_health_status(self, context, timeout_seconds=3):
        port = context.manifest["services"]["worker"]["port"]
        service = context.manifest["keychain_services"]["worker"]
        account = "authoring.bearer-token"
        script = "\n".join(
            (
                'service="$1"',
                'account="$2"',
                'url="$3"',
                'value="$(/usr/bin/security find-generic-password -s "$service" -a "$account" -w)" || exit 71',
                'response="$({ printf \'header = "Authorization: Bearer %s"\\n\' "$value"; } | /usr/bin/curl --silent --show-error --max-filesize 16384 --write-out \'\\n%{http_code}\' --connect-timeout "$4" --max-time "$4" --config - "$url")"',
                'result="$?"',
                'unset value',
                'test "$result" -eq 0 || exit "$result"',
                'printf \'%s\' "$response"',
            )
        )
        result = self.run(
            [
                "/bin/zsh",
                "-c",
                script,
                "d2-worker-health",
                service,
                account,
                f"http://127.0.0.1:{port}/health",
                str(timeout_seconds),
            ],
            timeout=timeout_seconds + 3,
        )
        if result.returncode != 0:
            return 0
        try:
            body, raw_status = result.stdout.rsplit(b"\n", 1)
            status = int(raw_status)
        except (ValueError, TypeError):
            fail("health_probe_output_invalid")
        if status < 100 or status > 599:
            fail("health_probe_output_invalid")
        if status != 200:
            return status
        try:
            health = json.loads(body)
        except (UnicodeDecodeError, json.JSONDecodeError):
            fail("worker_health_output_invalid")
        required = {
            "schema_version",
            "status",
            "provider",
            "model",
            "reasoning_effort",
            "auth_mode",
            "codex_cli_version",
            "instance_id",
            "worker_source_sha256",
            "concurrency_limit",
            "queue_capacity",
            "request_timeout_ms",
            "active_requests",
            "queued_requests",
            "accepted_requests_total",
            "settled_requests_total",
        }
        manifest = context.manifest
        if (
            not isinstance(health, dict)
            or set(health) != required
            or health["schema_version"] != 1
            or health["status"] != "ok"
            or any(
                health[field] != manifest["authoring"][field]
                for field in ("provider", "model", "reasoning_effort", "auth_mode")
            )
            or health["worker_source_sha256"]
            != manifest["source_trees"]["codex_worker"]["sha256"]
            or health["concurrency_limit"] != 1
            or health["queue_capacity"] != 4
            or health["request_timeout_ms"] != 55000
            or not isinstance(health["codex_cli_version"], str)
            or not health["codex_cli_version"]
            or not isinstance(health["instance_id"], str)
            or not health["instance_id"]
            or any(
                type(health[field]) is not int or health[field] < 0
                for field in (
                    "active_requests",
                    "queued_requests",
                    "accepted_requests_total",
                    "settled_requests_total",
                )
            )
        ):
            fail("worker_health_identity_invalid")
        return status

    def transport_health_status(self, context, timeout_seconds=3):
        value = self._transport_control_exchange(
            context, "snapshot", {}, timeout_seconds, allow_unavailable=True
        )
        if value is None:
            return 0
        if (
            not isinstance(value, dict)
            or set(value) != {"ok", "snapshot"}
            or value["ok"] is not True
            or not self._transport_snapshot_valid(
                context, value["snapshot"], require_ready=False
            )
        ):
            fail("transport_control_response_invalid")
        return 200 if value["snapshot"]["ready"] else 503

    def transport_control(self, context, command, fields=None, timeout_seconds=3):
        value = self._transport_control_exchange(
            context, command, fields or {}, timeout_seconds, allow_unavailable=False
        )
        if command == "snapshot":
            if (
                not isinstance(value, dict)
                or set(value) != {"ok", "snapshot"}
                or value["ok"] is not True
                or not self._transport_snapshot_valid(context, value["snapshot"])
            ):
                fail("transport_control_response_invalid")
            return value["snapshot"]
        if command in {
            "arm_next_duplicate",
            "arm_next_create_role_indeterminate",
        }:
            if (
                not isinstance(value, dict)
                or set(value) != {"ok", "changed", "disposition"}
                or value["ok"] is not True
                or type(value["changed"]) is not bool
                or value["disposition"] not in {"armed", "replayed", "busy"}
                or (value["disposition"] == "armed") != value["changed"]
            ):
                fail("transport_control_response_invalid")
            return {
                "changed": value["changed"],
                "disposition": value["disposition"],
            }
        if (
            not isinstance(value, dict)
            or set(value) != {"ok", "changed"}
            or value["ok"] is not True
            or type(value["changed"]) is not bool
        ):
            fail("transport_control_response_invalid")
        return {"changed": value["changed"]}

    def _transport_control_exchange(
        self, context, command, fields, timeout_seconds, allow_unavailable
    ):
        path = context.root / "transport-control.sock"
        try:
            metadata = path.lstat()
        except OSError:
            if allow_unavailable:
                return None
            fail("transport_control_unavailable")
        if (
            not stat.S_ISSOCK(metadata.st_mode)
            or path.is_symlink()
            or metadata.st_uid != os.getuid()
            or stat.S_IMODE(metadata.st_mode) != 0o600
        ):
            fail("transport_control_socket_invalid")
        request = {
            "version": 1,
            "command": command,
            "run_id": context.manifest["run_id"],
            "guild_id": context.manifest["discord"]["guild_id"],
            "actor_id": context.manifest["discord"]["actor_id"],
            "bot_user_id": context.manifest["discord"]["bot_user_id"],
        }
        if not isinstance(fields, dict) or set(fields).intersection(request):
            fail("transport_control_request_invalid")
        request.update(fields)
        encoded = (
            json.dumps(request, ensure_ascii=True, sort_keys=True, separators=(",", ":"))
            + "\n"
        ).encode("ascii")
        control = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        try:
            control.settimeout(timeout_seconds)
            control.connect(str(path))
            control.sendall(encoded)
            control.shutdown(socket.SHUT_WR)
            response = bytearray()
            while True:
                chunk = control.recv(4096)
                if not chunk:
                    break
                response.extend(chunk)
                if len(response) > MAX_TRANSPORT_CONTROL_BYTES:
                    fail("transport_control_response_too_large")
        except (OSError, TimeoutError):
            if allow_unavailable:
                return None
            fail("transport_control_unavailable")
        finally:
            control.close()
        if response.count(b"\n") != 1 or not response.endswith(b"\n"):
            fail("transport_control_response_invalid")
        try:
            value = json.loads(response)
        except (UnicodeDecodeError, json.JSONDecodeError):
            fail("transport_control_response_invalid")
        return value

    def _transport_snapshot_valid(self, context, snapshot, require_ready=True):
        if not isinstance(snapshot, dict) or set(snapshot) != {
            "version",
            "ready",
            "run_id",
            "guild_id",
            "actor_id",
            "bot_user_id",
            "instance_id",
            "gateway",
            "effect_http",
        }:
            return False
        if (
            snapshot["version"] != 1
            or type(snapshot["ready"]) is not bool
            or require_ready
            and snapshot["ready"] is not True
            or snapshot["run_id"] != context.manifest["run_id"]
            or snapshot["guild_id"] != context.manifest["discord"]["guild_id"]
            or snapshot["actor_id"] != context.manifest["discord"]["actor_id"]
            or snapshot["bot_user_id"]
            != context.manifest["discord"]["bot_user_id"]
            or not isinstance(snapshot["instance_id"], str)
            or not re.fullmatch(r"d2ti-[0-9a-f]{32}", snapshot["instance_id"])
        ):
            return False
        gateway = snapshot["gateway"]
        if not isinstance(gateway, dict) or set(gateway) != {
            "partitioned",
            "connections",
            "ready_rewrites",
            "partition_events",
            "identity_rejections",
            "duplicate_armed",
            "armed_duplicate_operation_id",
            "duplicate_claimed",
            "claimed_duplicate_operation_id",
            "duplicate_injections",
            "duplicate_failed_attempts",
            "last_failed_duplicate_operation_id",
            "duplicate_delivery_count",
            "last_duplicate_interaction_id",
            "last_duplicate_operation_id",
        }:
            return False
        if any(
            type(gateway[field]) is not bool
            for field in ("partitioned", "duplicate_armed", "duplicate_claimed")
        ):
            return False
        if any(
            type(gateway[field]) is not int or gateway[field] < 0
            for field in (
                "connections",
                "ready_rewrites",
                "partition_events",
                "identity_rejections",
                "duplicate_injections",
                "duplicate_failed_attempts",
                "duplicate_delivery_count",
            )
        ):
            return False
        interaction_id = gateway["last_duplicate_interaction_id"]
        if interaction_id is not None and (
            not isinstance(interaction_id, str)
            or not re.fullmatch(r"[1-9][0-9]{0,19}", interaction_id)
        ):
            return False
        for field in (
            "armed_duplicate_operation_id",
            "claimed_duplicate_operation_id",
            "last_failed_duplicate_operation_id",
            "last_duplicate_operation_id",
        ):
            operation_id = gateway[field]
            if operation_id is not None and (
                not isinstance(operation_id, str)
                or not re.fullmatch(r"[a-z][a-z0-9_.:-]{7,95}", operation_id)
            ):
                return False
        if gateway["duplicate_armed"] != (
            gateway["armed_duplicate_operation_id"] is not None
        ) or gateway["duplicate_claimed"] != (
            gateway["claimed_duplicate_operation_id"] is not None
        ):
            return False
        effect = snapshot["effect_http"]
        if not isinstance(effect, dict) or set(effect) != {
            "forwarded_requests",
            "rejected_requests",
            "indeterminate_armed",
            "armed_indeterminate_operation_id",
            "indeterminate_claimed",
            "claimed_indeterminate_operation_id",
            "indeterminate_injections",
            "last_indeterminate_audit_reason_sha256",
            "last_indeterminate_operation_id",
            "last_indeterminate_upstream_status",
            "owned_role_count",
            "owned_channel_count",
            "owned_message_count",
        }:
            return False
        if any(
            type(effect[field]) is not bool
            for field in ("indeterminate_armed", "indeterminate_claimed")
        ) or any(
            type(effect[field]) is not int or effect[field] < 0
            for field in (
                "forwarded_requests",
                "rejected_requests",
                "indeterminate_injections",
                "owned_role_count",
                "owned_channel_count",
                "owned_message_count",
            )
        ):
            return False
        digest = effect["last_indeterminate_audit_reason_sha256"]
        if digest is not None and (
            not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest)
        ):
            return False
        for field in (
            "armed_indeterminate_operation_id",
            "claimed_indeterminate_operation_id",
            "last_indeterminate_operation_id",
        ):
            operation_id = effect[field]
            if operation_id is not None and (
                not isinstance(operation_id, str)
                or not re.fullmatch(r"[a-z][a-z0-9_.:-]{7,95}", operation_id)
            ):
                return False
        if effect["indeterminate_armed"] != (
            effect["armed_indeterminate_operation_id"] is not None
        ) or effect["indeterminate_claimed"] != (
            effect["claimed_indeterminate_operation_id"] is not None
        ):
            return False
        status = effect["last_indeterminate_upstream_status"]
        return status is None or type(status) is int and 100 <= status <= 599

    def wait_for_status(self, probe, expected, timeout_seconds=60):
        deadline = time.monotonic() + timeout_seconds
        status = 0
        while True:
            status = probe()
            if status == expected:
                return status
            if time.monotonic() >= deadline:
                return status
            time.sleep(0.25)

    def launchd_bootout(self, label):
        if not self.launchd_loaded(label):
            return
        result = self.run(
            [
                REQUIRED_PROGRAMS["launchctl"],
                "bootout",
                f"gui/{os.getuid()}/{label}",
            ],
            timeout=20,
        )
        if result.returncode != 0:
            fail("launchd_bootout_failed")
