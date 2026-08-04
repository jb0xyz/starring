import hashlib
import json
import os
import pathlib
import re
import socket
import stat
import subprocess
import time

from d2_orchestrator_contract import OWNER_ACCOUNT, REQUIRED_PROGRAMS, fail


MAX_TRANSPORT_CONTROL_BYTES = 64 * 1024
MAX_DISCORD_RESPONSE_BYTES = 256 * 1024
RUNTIME_PROCESS_INSTANCE_PATTERN = re.compile(r"^[0-9a-f]{32}$")
TRANSPORT_INSTANCE_PATTERN = re.compile(r"^d2ti-[0-9a-f]{32}$")
SNOWFLAKE_PATTERN = re.compile(r"^[1-9][0-9]{0,19}$")
DIGEST_PATTERN = re.compile(r"^[0-9a-f]{64}$")
WORKER_INSTANCE_PATTERN = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$")
TRANSPORT_RESOURCE_HISTORY_LIMIT = 128
TRANSPORT_RESOURCE_INVENTORY_KIND = "starring.d2.run-owned-resource-inventory.v1"
DISCORD_RESOURCE_UNKNOWN = {
    "role": {10011: "Unknown Role"},
    "channel": {10003: "Unknown Channel"},
    "message": {10003: "Unknown Channel", 10008: "Unknown Message"},
}
DISCORD_RESOURCE_SUCCESS = {
    ("role", "GET"): 200,
    ("role", "DELETE"): 204,
    ("channel", "GET"): 200,
    ("channel", "DELETE"): 200,
    ("message", "GET"): 200,
    ("message", "DELETE"): 204,
}
LAUNCHD_DECORATED_EXIT_PATTERN = re.compile(
    r"^(-?[0-9]+): ([A-Z][A-Z0-9_]{0,63})$"
)
LAUNCHD_STATE_NORMALIZATION = {
    "running": "running",
    "exited": "exited",
    "not running": "exited",
}


def strict_json_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate_key")
        result[key] = value
    return result


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

    def launchd_job(self, label):
        result = self.run(
            [REQUIRED_PROGRAMS["launchctl"], "print", f"gui/{os.getuid()}/{label}"],
            timeout=5,
        )
        if result.returncode == 113:
            return None
        if result.returncode != 0:
            fail("launchd_observation_failed")
        if len(result.stdout) > 256 * 1024:
            fail("launchd_observation_invalid")
        try:
            output = result.stdout.decode("utf-8")
        except UnicodeDecodeError:
            fail("launchd_observation_invalid")
        pid_matches = re.findall(r"(?m)^\tpid = ([1-9][0-9]*)\s*$", output)
        program_matches = re.findall(r"(?m)^\tprogram = (\S(?:.*\S)?)\s*$", output)
        path_matches = re.findall(r"(?m)^\tpath = (\S(?:.*\S)?)\s*$", output)
        state_matches = re.findall(r"(?m)^\tstate = (\S(?:.*\S)?)\s*$", output)
        runs_matches = re.findall(r"(?m)^\truns = ([1-9][0-9]*)\s*$", output)
        exit_matches = re.findall(r"(?m)^\tlast exit code = (\S(?:.*\S)?)\s*$", output)
        argument_blocks = []
        lines = output.splitlines()
        for index, line in enumerate(lines):
            if line != "\targuments = {":
                continue
            arguments = []
            for nested in lines[index + 1 :]:
                if nested == "\t}":
                    break
                if not nested.startswith("\t\t"):
                    fail("launchd_observation_invalid")
                arguments.append(nested[2:])
            else:
                fail("launchd_observation_invalid")
            argument_blocks.append(arguments)
        if (
            len(pid_matches) > 1
            or len(program_matches) != 1
            or len(path_matches) != 1
            or len(state_matches) != 1
            or len(runs_matches) != 1
            or len(exit_matches) > 1
            or len(argument_blocks) > 1
        ):
            fail("launchd_observation_invalid")
        last_exit_code = None
        if exit_matches:
            if exit_matches[0] != "(never exited)":
                plain_exit = re.fullmatch(r"-?[0-9]+", exit_matches[0])
                decorated_exit = LAUNCHD_DECORATED_EXIT_PATTERN.fullmatch(
                    exit_matches[0]
                )
                if plain_exit:
                    last_exit_code = int(exit_matches[0])
                elif decorated_exit and int(decorated_exit.group(1)) != 0:
                    last_exit_code = int(decorated_exit.group(1))
                else:
                    fail("launchd_observation_invalid")
        state = LAUNCHD_STATE_NORMALIZATION.get(state_matches[0])
        pid = int(pid_matches[0]) if pid_matches else None
        if (
            state is None
            or (state == "running" and pid is None)
            or (
                state == "exited"
                and (pid is not None or last_exit_code is None)
            )
        ):
            fail("launchd_observation_invalid")
        return {
            "pid": pid,
            "program": program_matches[0],
            "plist_path": path_matches[0],
            "arguments": argument_blocks[0] if argument_blocks else None,
            "runs": int(runs_matches[0]),
            "state": state,
            "last_exit_code": last_exit_code,
        }

    def postgres_pid(self, cluster_root):
        path = pathlib.Path(cluster_root) / "postmaster.pid"
        try:
            metadata = path.lstat()
            raw = path.read_bytes()
        except OSError:
            return None
        if (
            not stat.S_ISREG(metadata.st_mode)
            or path.is_symlink()
            or metadata.st_uid != os.getuid()
            or len(raw) > 64 * 1024
        ):
            fail("postgres_pid_invalid")
        try:
            first_line = raw.splitlines()[0].decode("ascii")
        except (IndexError, UnicodeDecodeError):
            fail("postgres_pid_invalid")
        if not re.fullmatch(r"[1-9][0-9]*", first_line):
            fail("postgres_pid_invalid")
        return int(first_line)

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

    def preflight_discord_hub_channel(self, context, timeout_seconds=10):
        identity = context.manifest["external_keychain"]["discord_bot_token"]
        channel_id = context.manifest["discord"]["hub_channel_id"]
        script = "\n".join(
            (
                'service="$1"',
                'account="$2"',
                'url="$3"',
                'value="$(/usr/bin/security find-generic-password -s "$service" -a "$account" -w)" || exit 71',
                'case "$value" in (""|*[!A-Za-z0-9._~-]*) unset value; exit 72;; esac',
                'response="$({ printf \'header = "Authorization: Bot %s"\\n\' "$value"; } | /usr/bin/curl --silent --show-error --request GET --proto \'=https\' --header \'Accept: application/json\' --max-filesize 16384 --write-out \'\\n%{http_code}\' --connect-timeout "$4" --max-time "$4" --config - "$url")"',
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
                "d2-discord-hub-preflight",
                identity["service"],
                identity["account"],
                f"https://discord.com/api/v10/channels/{channel_id}",
                str(timeout_seconds),
            ],
            timeout=timeout_seconds + 3,
        )
        if result.returncode != 0:
            fail("discord_hub_channel_preflight_unavailable")
        if len(result.stdout) > 20 * 1024:
            fail("discord_hub_channel_preflight_output_invalid")
        try:
            body, raw_status = result.stdout.rsplit(b"\n", 1)
            status = int(raw_status)
        except (ValueError, TypeError):
            fail("discord_hub_channel_preflight_output_invalid")
        if status < 100 or status > 599:
            fail("discord_hub_channel_preflight_output_invalid")
        if status != 200:
            fail("discord_hub_channel_preflight_status_invalid")
        try:
            channel = json.loads(body)
        except (UnicodeDecodeError, json.JSONDecodeError):
            fail("discord_hub_channel_preflight_response_invalid")
        if (
            not isinstance(channel, dict)
            or channel.get("id") != channel_id
            or channel.get("guild_id") != context.manifest["discord"]["guild_id"]
            or type(channel.get("type")) is not int
            or channel["type"] != 0
        ):
            fail("discord_hub_channel_preflight_response_invalid")

    def onboard_installation(self, context, principal_id, display_name, installation_id):
        self.preflight_discord_hub_channel(context)
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
            or set(evidence)
            != {
                "outcome",
                "installation_id",
                "principal_id",
                "binding_key",
                "hub_channel_id",
            }
            or evidence["outcome"] not in {"fresh", "exact_replay"}
            or evidence["installation_id"] != installation_id
            or evidence["principal_id"] != principal_id
            or evidence["binding_key"] != "community_hub"
            or evidence["hub_channel_id"]
            != context.manifest["discord"]["hub_channel_id"]
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

    def launchd_signal(self, label, signal_name):
        if signal_name != "SIGTERM":
            fail("launchd_signal_invalid")
        result = self.run(
            [
                REQUIRED_PROGRAMS["launchctl"],
                "kill",
                signal_name,
                f"gui/{os.getuid()}/{label}",
            ],
            timeout=10,
        )
        if result.returncode != 0:
            fail("launchd_signal_failed")

    def http_status(self, url, timeout_seconds=3, host_header=None):
        arguments = [
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
        ]
        if host_header is not None:
            arguments.extend(("--header", f"Host: {host_header}"))
        arguments.append(url)
        result = self.run(arguments, timeout=timeout_seconds + 2)
        if result.returncode != 0:
            return 0
        try:
            status = int(result.stdout)
        except ValueError:
            fail("health_probe_output_invalid")
        if status < 100 or status > 599:
            fail("health_probe_output_invalid")
        return status

    def runtime_process_identity(self, context, timeout_seconds=3):
        port = context.manifest["services"]["runtime"]["port"]
        result = self.run(
            [
                REQUIRED_PROGRAMS["curl"],
                "--silent",
                "--show-error",
                "--proto",
                "=http",
                "--max-filesize",
                "16384",
                "--write-out",
                "\n%{http_code}",
                "--connect-timeout",
                str(timeout_seconds),
                "--max-time",
                str(timeout_seconds),
                f"http://127.0.0.1:{port}/health/identity",
            ],
            timeout=timeout_seconds + 2,
        )
        if result.returncode != 0 or len(result.stdout) > 20 * 1024:
            return None
        try:
            body, raw_status = result.stdout.rsplit(b"\n", 1)
            status = int(raw_status)
        except (ValueError, TypeError):
            fail("runtime_identity_probe_output_invalid")
        if status < 100 or status > 599:
            fail("runtime_identity_probe_output_invalid")
        if status != 200:
            return None
        try:
            identity = json.loads(
                body, object_pairs_hook=strict_json_object
            )
        except (UnicodeDecodeError, json.JSONDecodeError, ValueError):
            fail("runtime_identity_probe_output_invalid")
        if (
            not isinstance(identity, dict)
            or set(identity)
            != {"schema_version", "os_pid", "process_instance_id"}
            or type(identity["schema_version"]) is not int
            or identity["schema_version"] != 1
            or type(identity["os_pid"]) is not int
            or identity["os_pid"] <= 0
            or not isinstance(identity["process_instance_id"], str)
            or not RUNTIME_PROCESS_INSTANCE_PATTERN.fullmatch(
                identity["process_instance_id"]
            )
        ):
            fail("runtime_identity_probe_output_invalid")
        return identity

    def _worker_health_probe(self, context, timeout_seconds=3):
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
            return 0, None
        try:
            body, raw_status = result.stdout.rsplit(b"\n", 1)
            status = int(raw_status)
        except (ValueError, TypeError):
            fail("health_probe_output_invalid")
        if status < 100 or status > 599:
            fail("health_probe_output_invalid")
        if status != 200:
            return status, None
        try:
            health = json.loads(body, object_pairs_hook=strict_json_object)
        except (UnicodeDecodeError, json.JSONDecodeError, ValueError):
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
            or type(health["schema_version"]) is not int
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
            or not WORKER_INSTANCE_PATTERN.fullmatch(health["instance_id"])
            or not isinstance(health["worker_source_sha256"], str)
            or not DIGEST_PATTERN.fullmatch(health["worker_source_sha256"])
            or any(
                type(health[field]) is not int
                for field in ("concurrency_limit", "queue_capacity", "request_timeout_ms")
            )
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
        return status, health

    def worker_health_status(self, context, timeout_seconds=3):
        status, _health = self._worker_health_probe(context, timeout_seconds)
        return status

    def worker_health_snapshot(self, context, timeout_seconds=3):
        status, health = self._worker_health_probe(context, timeout_seconds)
        if status != 200 or health is None:
            fail("worker_health_unready")
        return health

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
        if command == "resource_inventory":
            if (
                not isinstance(value, dict)
                or set(value) != {"ok", "resource_inventory"}
                or value["ok"] is not True
                or not self._transport_resource_inventory_valid(
                    context, value["resource_inventory"]
                )
            ):
                fail("transport_control_response_invalid")
            return value["resource_inventory"]
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
            value = json.loads(response, object_pairs_hook=strict_json_object)
        except (UnicodeDecodeError, json.JSONDecodeError, ValueError):
            fail("transport_control_response_invalid")
        return value

    def _pinned_transport_instance_id(self, context):
        path = context.artifact_directory / "step-03-evidence.json"
        try:
            metadata = path.lstat()
            raw = path.read_bytes()
        except OSError:
            fail("transport_instance_evidence_absent")
        if (
            not stat.S_ISREG(metadata.st_mode)
            or path.is_symlink()
            or metadata.st_uid != os.getuid()
            or stat.S_IMODE(metadata.st_mode) != 0o600
            or not raw
            or len(raw) > MAX_TRANSPORT_CONTROL_BYTES
        ):
            fail("transport_instance_evidence_invalid")
        try:
            evidence = json.loads(raw, object_pairs_hook=strict_json_object)
        except (UnicodeDecodeError, json.JSONDecodeError, ValueError):
            fail("transport_instance_evidence_invalid")
        instance_id = (
            evidence.get("transport_instance_id")
            if isinstance(evidence, dict)
            else None
        )
        if (
            not isinstance(instance_id, str)
            or not TRANSPORT_INSTANCE_PATTERN.fullmatch(instance_id)
        ):
            fail("transport_instance_evidence_invalid")
        return instance_id

    def _transport_resource_identity(self, value, history=False):
        if not isinstance(value, dict):
            return None
        kind = value.get("kind")
        expected = {"kind", "resource_id", "state"} if history else {
            "kind",
            "resource_id",
        }
        if kind == "message":
            expected.add("channel_id")
        if kind not in {"role", "channel", "message"} or set(value) != expected:
            return None
        resource_id = value["resource_id"]
        if not self._discord_snowflake_valid(resource_id):
            return None
        channel_id = value.get("channel_id")
        if kind == "message" and not self._discord_snowflake_valid(channel_id):
            return None
        state = value.get("state") if history else None
        if history and state not in {"created", "deleted"}:
            return None
        identity = {"kind": kind, "resource_id": resource_id}
        if channel_id is not None:
            identity["channel_id"] = channel_id
        normalized = dict(identity)
        if history:
            normalized["state"] = state
        return identity, normalized, (kind, resource_id, channel_id or ""), state

    def _transport_resource_inventory_valid(self, context, inventory):
        required = {
            "version",
            "kind",
            "instance_id",
            "run_id",
            "guild_id",
            "hub_channel_id",
            "actor_id",
            "bot_user_id",
            "history_limit",
            "history",
            "created",
            "deleted",
            "active",
            "digest_sha256",
        }
        manifest = context.manifest
        discord = manifest["discord"]
        if (
            not isinstance(inventory, dict)
            or set(inventory) != required
            or type(inventory["version"]) is not int
            or inventory["version"] != 1
            or inventory["kind"] != TRANSPORT_RESOURCE_INVENTORY_KIND
            or inventory["instance_id"] != self._pinned_transport_instance_id(context)
            or inventory["run_id"] != manifest["run_id"]
            or inventory["guild_id"] != discord["guild_id"]
            or inventory["hub_channel_id"] != discord["hub_channel_id"]
            or inventory["actor_id"] != discord["actor_id"]
            or inventory["bot_user_id"] != discord["bot_user_id"]
            or type(inventory["history_limit"]) is not int
            or inventory["history_limit"] != TRANSPORT_RESOURCE_HISTORY_LIMIT
            or not isinstance(inventory["history"], list)
            or not isinstance(inventory["created"], list)
            or not isinstance(inventory["deleted"], list)
            or not isinstance(inventory["active"], list)
            or not isinstance(inventory["digest_sha256"], str)
            or not DIGEST_PATTERN.fullmatch(inventory["digest_sha256"])
            or len(inventory["history"]) > TRANSPORT_RESOURCE_HISTORY_LIMIT
        ):
            return False
        normalized_history = []
        history_keys = []
        history_identities = []
        states = {}
        for entry in inventory["history"]:
            parsed = self._transport_resource_identity(entry, history=True)
            if parsed is None:
                return False
            identity, normalized, key, state = parsed
            normalized_history.append(normalized)
            history_keys.append(key)
            history_identities.append(identity)
            if key in states:
                return False
            states[key] = state
        if history_keys != sorted(history_keys):
            return False
        normalized_lists = {}
        for name in ("created", "deleted", "active"):
            entries = []
            keys = []
            for entry in inventory[name]:
                parsed = self._transport_resource_identity(entry)
                if parsed is None:
                    return False
                identity, _, key, _ = parsed
                entries.append(identity)
                keys.append(key)
            if keys != sorted(keys) or len(keys) != len(set(keys)):
                return False
            normalized_lists[name] = entries
        expected_deleted = [
            identity
            for identity, key in zip(history_identities, history_keys)
            if states[key] == "deleted"
        ]
        expected_active = [
            identity
            for identity, key in zip(history_identities, history_keys)
            if states[key] == "created"
        ]
        if (
            normalized_lists["created"] != history_identities
            or normalized_lists["deleted"] != expected_deleted
            or normalized_lists["active"] != expected_active
        ):
            return False
        protected_ids = {
            discord["guild_id"],
            discord["hub_channel_id"],
            discord["application_id"],
            discord["actor_id"],
            discord["bot_user_id"],
        }
        resource_ids = [identity["resource_id"] for identity in history_identities]
        if (
            len(resource_ids) != len(set(resource_ids))
            or protected_ids.intersection(resource_ids)
        ):
            return False
        channel_states = {
            identity["resource_id"]: states[key]
            for identity, key in zip(history_identities, history_keys)
            if identity["kind"] == "channel"
        }
        for identity, key in zip(history_identities, history_keys):
            if identity["kind"] != "message":
                continue
            channel_id = identity["channel_id"]
            parent_state = (
                "created"
                if channel_id == discord["hub_channel_id"]
                else channel_states.get(channel_id)
            )
            if parent_state is None or (
                states[key] == "created" and parent_state != "created"
            ):
                return False
        payload = {
            "version": 1,
            "kind": TRANSPORT_RESOURCE_INVENTORY_KIND,
            "instance_id": inventory["instance_id"],
            "run_id": inventory["run_id"],
            "guild_id": inventory["guild_id"],
            "hub_channel_id": inventory["hub_channel_id"],
            "actor_id": inventory["actor_id"],
            "bot_user_id": inventory["bot_user_id"],
            "history_limit": TRANSPORT_RESOURCE_HISTORY_LIMIT,
            "history": normalized_history,
            "created": normalized_lists["created"],
            "deleted": normalized_lists["deleted"],
            "active": normalized_lists["active"],
        }
        encoded = json.dumps(
            payload, ensure_ascii=True, separators=(",", ":")
        ).encode("ascii")
        return hashlib.sha256(encoded).hexdigest() == inventory["digest_sha256"]

    def _discord_snowflake_valid(self, value):
        return (
            isinstance(value, str)
            and SNOWFLAKE_PATTERN.fullmatch(value) is not None
            and int(value) <= 18446744073709551615
        )

    def _manifest_owned_discord_resource(self, context, resource, inventory):
        if not self._transport_resource_inventory_valid(context, inventory):
            fail("discord_resource_inventory_invalid")
        parsed = self._transport_resource_identity(resource)
        if parsed is None:
            fail("discord_resource_identity_invalid")
        identity = parsed[0]
        if identity not in inventory["created"]:
            fail("discord_resource_not_manifest_owned")
        protected_ids = {
            context.manifest["discord"][field]
            for field in (
                "guild_id",
                "hub_channel_id",
                "application_id",
                "actor_id",
                "bot_user_id",
            )
        }
        if identity["resource_id"] in protected_ids:
            fail("discord_resource_protected")
        return identity

    def _discord_resource_url(self, context, resource):
        guild_id = context.manifest["discord"]["guild_id"]
        if resource["kind"] == "role":
            return f"https://discord.com/api/v10/guilds/{guild_id}/roles"
        if resource["kind"] == "channel":
            return f"https://discord.com/api/v10/channels/{resource['resource_id']}"
        return (
            "https://discord.com/api/v10/channels/"
            f"{resource['channel_id']}/messages/{resource['resource_id']}"
        )

    def _discord_resource_delete_url(self, context, resource):
        if resource["kind"] != "role":
            return self._discord_resource_url(context, resource)
        guild_id = context.manifest["discord"]["guild_id"]
        return (
            f"https://discord.com/api/v10/guilds/{guild_id}/roles/"
            f"{resource['resource_id']}"
        )

    def _discord_request(self, context, method, url, timeout_seconds):
        if (
            method not in {"GET", "DELETE"}
            or not isinstance(url, str)
            or not url.startswith("https://discord.com/api/v10/")
            or "?" in url
            or "#" in url
            or type(timeout_seconds) is not int
            or timeout_seconds < 1
            or timeout_seconds > 30
        ):
            fail("discord_resource_request_invalid")
        return self._discord_curl_request(
            context, method, url, timeout_seconds, "=https"
        )

    def _discord_proxy_request(self, context, method, url, timeout_seconds):
        port = context.manifest["services"]["transport"]["http_port"]
        prefix = f"http://127.0.0.1:{port}/api/v10/"
        if (
            method != "DELETE"
            or type(port) is not int
            or port < 1024
            or port > 65535
            or not isinstance(url, str)
            or not url.startswith(prefix)
            or "?" in url
            or "#" in url
            or type(timeout_seconds) is not int
            or timeout_seconds < 1
            or timeout_seconds > 30
        ):
            fail("discord_resource_proxy_request_invalid")
        return self._discord_curl_request(
            context, method, url, timeout_seconds, "=http"
        )

    def _discord_curl_request(
        self, context, method, url, timeout_seconds, protocol
    ):
        identity = context.manifest["external_keychain"]["discord_bot_token"]
        script = "\n".join(
            (
                'method="$1"',
                'service="$2"',
                'account="$3"',
                'url="$4"',
                'timeout="$5"',
                'maximum="$6"',
                'security="$7"',
                'curl="$8"',
                'protocol="$9"',
                'value="$("$security" find-generic-password -s "$service" -a "$account" -w)" || exit 71',
                'case "$value" in (""|*[!A-Za-z0-9._~-]*) unset value; exit 72;; esac',
                'response="$({ printf \'header = "Authorization: Bot %s"\\n\' "$value"; } | "$curl" -q --silent --show-error --request "$method" --proto "$protocol" --proto-redir "$protocol" --max-redirs 0 --noproxy \'*\' --proxy \'\' --header \'Accept: application/json\' --header \'User-Agent: Starring-D2-Certification/1\' --max-filesize "$maximum" --write-out \'\\n%{http_code}\' --connect-timeout "$timeout" --max-time "$timeout" --config - "$url")"',
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
                "d2-discord-resource-request",
                method,
                identity["service"],
                identity["account"],
                url,
                str(timeout_seconds),
                str(MAX_DISCORD_RESPONSE_BYTES),
                REQUIRED_PROGRAMS["security"],
                REQUIRED_PROGRAMS["curl"],
                protocol,
            ],
            timeout=timeout_seconds + 3,
        )
        if result.returncode != 0:
            fail("discord_resource_request_failed")
        if not result.stdout or len(result.stdout) > MAX_DISCORD_RESPONSE_BYTES + 4:
            fail("discord_resource_response_invalid")
        try:
            body, raw_status = result.stdout.rsplit(b"\n", 1)
        except ValueError:
            fail("discord_resource_response_invalid")
        if re.fullmatch(rb"[1-5][0-9]{2}", raw_status) is None:
            fail("discord_resource_response_invalid")
        return int(raw_status), body

    def _discord_json(self, body):
        if not body or len(body) > MAX_DISCORD_RESPONSE_BYTES:
            fail("discord_resource_response_invalid")
        try:
            return json.loads(body, object_pairs_hook=strict_json_object)
        except (UnicodeDecodeError, json.JSONDecodeError, ValueError):
            fail("discord_resource_response_invalid")

    def _discord_unknown_code(self, resource_kind, body):
        value = self._discord_json(body)
        if (
            not isinstance(value, dict)
            or set(value) != {"message", "code"}
            or type(value["code"]) is not int
            or DISCORD_RESOURCE_UNKNOWN[resource_kind].get(value["code"])
            != value["message"]
        ):
            fail("discord_resource_unknown_response_invalid")
        return value["code"]

    def _discord_success_body_valid(self, context, resource, method, body):
        if DISCORD_RESOURCE_SUCCESS[(resource["kind"], method)] == 204:
            return body == b"", True
        value = self._discord_json(body)
        if resource["kind"] == "role":
            if not isinstance(value, list) or len(value) > 512:
                return False, False
            role_ids = []
            for role in value:
                if not isinstance(role, dict) or not self._discord_snowflake_valid(
                    role.get("id")
                ):
                    return False, False
                role_ids.append(role["id"])
            if len(role_ids) != len(set(role_ids)):
                return False, False
            return True, resource["resource_id"] in role_ids
        if not isinstance(value, dict) or value.get("id") != resource["resource_id"]:
            return False, False
        if value.get("guild_id") != context.manifest["discord"]["guild_id"]:
            return False, False
        if resource["kind"] == "message" and value.get("channel_id") != resource[
            "channel_id"
        ]:
            return False, False
        return True, True

    def _discord_resource_request_result(
        self, context, resource, method, timeout_seconds, through_transport=False
    ):
        direct_url = (
            self._discord_resource_url(context, resource)
            if method == "GET"
            else self._discord_resource_delete_url(context, resource)
        )
        if through_transport:
            direct_prefix = "https://discord.com"
            if not direct_url.startswith(direct_prefix):
                fail("discord_resource_proxy_request_invalid")
            port = context.manifest["services"]["transport"]["http_port"]
            url = f"http://127.0.0.1:{port}{direct_url.removeprefix(direct_prefix)}"
            status, body = self._discord_proxy_request(
                context, method, url, timeout_seconds
            )
        else:
            status, body = self._discord_request(
                context, method, direct_url, timeout_seconds
            )
        expected = DISCORD_RESOURCE_SUCCESS[(resource["kind"], method)]
        if status == expected:
            valid, exists = self._discord_success_body_valid(
                context, resource, method, body
            )
            if not valid:
                fail("discord_resource_success_response_invalid")
            return status, None, exists if method == "GET" else False
        if status != 404:
            fail("discord_resource_status_invalid")
        code = self._discord_unknown_code(resource["kind"], body)
        return status, code, False

    def discord_observe_resource(
        self, context, resource, inventory=None, timeout_seconds=10
    ):
        if inventory is None:
            inventory = self.transport_control(context, "resource_inventory")
        identity = self._manifest_owned_discord_resource(
            context, resource, inventory
        )
        status, discord_code, exists = self._discord_resource_request_result(
            context, identity, "GET", timeout_seconds
        )
        return {
            "schema_version": 1,
            "kind": "starring.d2.discord-resource-observation.v1",
            "transport_instance_id": inventory["instance_id"],
            "inventory_digest_sha256": inventory["digest_sha256"],
            "resource_kind": identity["kind"],
            "resource_id": identity["resource_id"],
            "channel_id": identity.get("channel_id"),
            "http_status": status,
            "discord_code": discord_code,
            "exists": exists,
        }

    def discord_delete_resource(
        self, context, resource, inventory=None, timeout_seconds=10
    ):
        if inventory is None:
            inventory = self.transport_control(context, "resource_inventory")
        identity = self._manifest_owned_discord_resource(
            context, resource, inventory
        )
        delete_status, delete_code, _ = self._discord_resource_request_result(
            context, identity, "DELETE", timeout_seconds
        )
        observe_status, observe_code, exists = self._discord_resource_request_result(
            context, identity, "GET", timeout_seconds
        )
        if exists:
            fail("discord_resource_delete_not_confirmed")
        return {
            "schema_version": 1,
            "kind": "starring.d2.discord-resource-deletion.v1",
            "transport_instance_id": inventory["instance_id"],
            "inventory_digest_sha256": inventory["digest_sha256"],
            "resource_kind": identity["kind"],
            "resource_id": identity["resource_id"],
            "channel_id": identity.get("channel_id"),
            "delete_http_status": delete_status,
            "delete_discord_code": delete_code,
            "observe_http_status": observe_status,
            "observe_discord_code": observe_code,
            "deleted": delete_status == DISCORD_RESOURCE_SUCCESS[
                (identity["kind"], "DELETE")
            ],
            "exists": False,
        }

    def discord_delete_resource_through_transport(
        self, context, resource, inventory=None, timeout_seconds=10
    ):
        if inventory is None:
            inventory = self.transport_control(context, "resource_inventory")
        identity = self._manifest_owned_discord_resource(
            context, resource, inventory
        )
        if identity not in inventory["active"]:
            fail("discord_resource_not_active")
        status, discord_code, _ = self._discord_resource_request_result(
            context,
            identity,
            "DELETE",
            timeout_seconds,
            through_transport=True,
        )
        return {
            "schema_version": 1,
            "kind": "starring.d2.discord-resource-proxy-deletion.v1",
            "transport_instance_id": inventory["instance_id"],
            "inventory_digest_sha256": inventory["digest_sha256"],
            "resource_kind": identity["kind"],
            "resource_id": identity["resource_id"],
            "channel_id": identity.get("channel_id"),
            "http_status": status,
            "discord_code": discord_code,
            "deleted": status
            == DISCORD_RESOURCE_SUCCESS[(identity["kind"], "DELETE")],
        }

    def _transport_snapshot_valid(self, context, snapshot, require_ready=True):
        if not isinstance(snapshot, dict) or set(snapshot) != {
            "version",
            "ready",
            "run_id",
            "guild_id",
            "hub_channel_id",
            "actor_id",
            "bot_user_id",
            "instance_id",
            "gateway",
            "effect_http",
        }:
            return False
        if (
            snapshot["version"] != 3
            or type(snapshot["ready"]) is not bool
            or require_ready
            and snapshot["ready"] is not True
            or snapshot["run_id"] != context.manifest["run_id"]
            or snapshot["guild_id"] != context.manifest["discord"]["guild_id"]
            or snapshot["hub_channel_id"]
            != context.manifest["discord"]["hub_channel_id"]
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
            "active_connections",
            "completed_connections",
            "clean_close_relays",
            "relay_failures",
            "connection_aborts",
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
                "active_connections",
                "completed_connections",
                "clean_close_relays",
                "relay_failures",
                "connection_aborts",
                "ready_rewrites",
                "partition_events",
                "identity_rejections",
                "duplicate_injections",
                "duplicate_failed_attempts",
                "duplicate_delivery_count",
            )
        ):
            return False
        if (
            gateway["active_connections"] > gateway["connections"]
            or gateway["completed_connections"] > gateway["connections"]
            or gateway["active_connections"] + gateway["completed_connections"]
            != gateway["connections"]
            or gateway["clean_close_relays"] > gateway["completed_connections"]
            or gateway["relay_failures"] > gateway["completed_connections"]
            or gateway["connection_aborts"] > gateway["completed_connections"]
            or gateway["clean_close_relays"]
            + gateway["relay_failures"]
            + gateway["connection_aborts"]
            > gateway["completed_connections"]
            or (
                gateway["relay_failures"] > 0
                or gateway["connection_aborts"] > 0
            )
            and snapshot["ready"] is not False
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
