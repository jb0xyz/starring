import json
import os
import pathlib
import plistlib
import stat
import subprocess
import time


LAUNCHCTL = pathlib.Path("/bin/launchctl")
PYTHON = pathlib.Path("/usr/bin/python3")
MAX_LAUNCHCTL_OUTPUT_BYTES = 64 * 1024
MAX_RESULT_BYTES = 4096
MAX_PLIST_BYTES = 64 * 1024
POLL_SECONDS = 0.1
TERMINATE_SECONDS = 10
LAUNCHER = (
    "import json,os,resource,sys\n"
    "environment=json.loads(sys.argv[1])\n"
    "cwd=sys.argv[2]\n"
    "result=sys.argv[3]\n"
    "pending=result+'.pending'\n"
    "nonce=sys.argv[4]\n"
    "label=sys.argv[5]\n"
    "argv=sys.argv[6:]\n"
    "os.environ.clear()\n"
    "os.environ.update(environment)\n"
    "os.chdir(cwd)\n"
    "resource.setrlimit(resource.RLIMIT_CORE,(0,0))\n"
    "resource.setrlimit(resource.RLIMIT_NOFILE,(4096,4096))\n"
    "resource.setrlimit(resource.RLIMIT_NPROC,(2048,2048))\n"
    "resource.setrlimit(resource.RLIMIT_FSIZE,(4294967296,4294967296))\n"
    "resource.setrlimit(resource.RLIMIT_CPU,(7200,7200))\n"
    "pid=os.fork()\n"
    "if pid==0: os.execve(argv[0],argv,environment)\n"
    "_,status=os.waitpid(pid,0)\n"
    "code=os.waitstatus_to_exitcode(status)\n"
    "payload=json.dumps({'exit_code':code,'label':label,'nonce':nonce},sort_keys=True,separators=(',',':')).encode('utf-8')\n"
    "flags=os.O_WRONLY|os.O_CREAT|os.O_EXCL\n"
    "flags|=getattr(os,'O_NOFOLLOW',0)\n"
    "descriptor=os.open(pending,flags,0o600)\n"
    "view=memoryview(payload)\n"
    "while view:\n"
    " written=os.write(descriptor,view)\n"
    " if written<=0: os._exit(125)\n"
    " view=view[written:]\n"
    "os.fsync(descriptor)\n"
    "os.close(descriptor)\n"
    "os.link(pending,result,follow_symlinks=False)\n"
    "os.unlink(pending)\n"
    "directory=os.open(os.path.dirname(result),os.O_RDONLY)\n"
    "os.fsync(directory)\n"
    "os.close(directory)\n"
    "while True: os.pause()\n"
)


class LaunchdJobError(Exception):
    pass


def fail(code):
    raise LaunchdJobError(code)


def require_executable(path, label):
    try:
        metadata = path.lstat()
    except OSError as error:
        fail(f"{label}_unavailable:{error.__class__.__name__}")
    if (
        not stat.S_ISREG(metadata.st_mode)
        or path.is_symlink()
        or metadata.st_uid != 0
        or stat.S_IMODE(metadata.st_mode) & 0o022
        or not stat.S_IMODE(metadata.st_mode) & 0o111
    ):
        fail(f"{label}_identity_invalid")
    return path


def launchctl(arguments, label, allowed=(0,)):
    executable = require_executable(LAUNCHCTL, "candidate_launchctl")
    try:
        result = subprocess.run(
            [str(executable), *arguments],
            env={"HOME": "/var/empty", "LANG": "C", "LC_ALL": "C", "PATH": "/usr/bin:/bin"},
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        fail(f"{label}_unavailable:{error.__class__.__name__}")
    if (
        len(result.stdout) > MAX_LAUNCHCTL_OUTPUT_BYTES
        or len(result.stderr) > MAX_LAUNCHCTL_OUTPUT_BYTES
    ):
        fail(f"{label}_output_invalid")
    if result.returncode not in allowed:
        fail(f"{label}_failed:{result.returncode}")
    return result


def service_target(label):
    return f"gui/{os.getuid()}/{label}"


def service_status(label):
    result = launchctl(
        ["print", service_target(label)],
        "candidate_launchd_print",
        allowed=(0, 113),
    )
    if result.returncode == 113:
        return False, False
    try:
        output = result.stdout.decode("utf-8")
    except UnicodeDecodeError:
        fail("candidate_launchd_print_invalid")
    active_values = [
        line.split("=", 1)[1].strip()
        for line in output.splitlines()
        if line.startswith("\tactive count = ")
    ]
    run_values = [
        line.split("=", 1)[1].strip()
        for line in output.splitlines()
        if line.startswith("\truns = ")
    ]
    if (
        len(active_values) != 1
        or len(run_values) > 1
        or not active_values[0].isdigit()
        or run_values
        and not run_values[0].isdigit()
    ):
        fail("candidate_launchd_print_invalid")
    active = int(active_values[0]) > 0 or not run_values or int(run_values[0]) == 0
    return True, active


def service_exists(label):
    return service_status(label)[0]


def terminate_service(label):
    target = service_target(label)
    if not service_exists(label):
        return
    launchctl(
        ["bootout", target],
        "candidate_launchd_bootout",
        allowed=(0, 3, 113),
    )
    deadline = time.monotonic() + TERMINATE_SECONDS
    while service_exists(label):
        if time.monotonic() >= deadline:
            fail("candidate_launchd_job_survived")
        time.sleep(POLL_SECONDS)


def read_result(path, nonce, label):
    flags = os.O_RDONLY
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        before = path.lstat()
        descriptor = os.open(path, flags)
    except OSError as error:
        fail(f"candidate_launchd_result_unavailable:{error.__class__.__name__}")
    try:
        observed = os.fstat(descriptor)
        raw = os.read(descriptor, MAX_RESULT_BYTES + 1)
        after = os.fstat(descriptor)
    finally:
        os.close(descriptor)
    if (
        not stat.S_ISREG(before.st_mode)
        or path.is_symlink()
        or before.st_uid != os.getuid()
        or before.st_nlink != 1
        or stat.S_IMODE(before.st_mode) != 0o600
        or not 0 < before.st_size <= MAX_RESULT_BYTES
        or (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns)
        != (observed.st_dev, observed.st_ino, observed.st_size, observed.st_mtime_ns)
        or (observed.st_dev, observed.st_ino, observed.st_size, observed.st_mtime_ns)
        != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns)
        or len(raw) != before.st_size
    ):
        fail("candidate_launchd_result_invalid")
    try:
        value = json.loads(raw)
    except (UnicodeDecodeError, json.JSONDecodeError):
        fail("candidate_launchd_result_invalid")
    if (
        not isinstance(value, dict)
        or set(value) != {"exit_code", "label", "nonce"}
        or type(value["exit_code"]) is not int
        or value["label"] != label
        or value["nonce"] != nonce
    ):
        fail("candidate_launchd_result_invalid")
    try:
        path.unlink()
        descriptor = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(descriptor)
        finally:
            os.close(descriptor)
    except OSError as error:
        fail(f"candidate_launchd_result_cleanup_failed:{error.__class__.__name__}")
    return value["exit_code"]


def write_private_file(path, payload, label):
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        descriptor = os.open(path, flags, 0o600)
    except OSError as error:
        fail(f"{label}_create_failed:{error.__class__.__name__}")
    try:
        view = memoryview(payload)
        while view:
            written = os.write(descriptor, view)
            if written <= 0:
                fail(f"{label}_write_failed")
            view = view[written:]
        os.fsync(descriptor)
    finally:
        os.close(descriptor)
    directory = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(directory)
    finally:
        os.close(directory)


def require_private_file(path, expected, label):
    flags = os.O_RDONLY
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        before = path.lstat()
        descriptor = os.open(path, flags)
    except OSError as error:
        fail(f"{label}_unavailable:{error.__class__.__name__}")
    try:
        observed = os.fstat(descriptor)
        raw = os.read(descriptor, len(expected) + 1)
        after = os.fstat(descriptor)
    finally:
        os.close(descriptor)
    if (
        not stat.S_ISREG(before.st_mode)
        or path.is_symlink()
        or before.st_uid != os.getuid()
        or before.st_nlink != 1
        or stat.S_IMODE(before.st_mode) != 0o600
        or before.st_size != len(expected)
        or (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns)
        != (observed.st_dev, observed.st_ino, observed.st_size, observed.st_mtime_ns)
        or (observed.st_dev, observed.st_ino, observed.st_size, observed.st_mtime_ns)
        != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns)
        or raw != expected
    ):
        fail(f"{label}_invalid")


def remove_private_file(path, expected, label):
    require_private_file(path, expected, label)
    try:
        path.unlink()
        directory = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    except OSError as error:
        fail(f"{label}_cleanup_failed:{error.__class__.__name__}")


def job_plist(label, program_arguments):
    payload = plistlib.dumps(
        {
            "AbandonProcessGroup": False,
            "HardResourceLimits": {
                "CPU": 7200,
                "Core": 0,
                "Data": 8 * 1024 * 1024 * 1024,
                "FileSize": 4 * 1024 * 1024 * 1024,
                "NumberOfFiles": 4096,
                "NumberOfProcesses": 2048,
                "ResidentSetSize": 8 * 1024 * 1024 * 1024,
            },
            "KeepAlive": False,
            "Label": label,
            "LowPriorityIO": False,
            "ProgramArguments": program_arguments,
            "RunAtLoad": True,
            "Umask": 0o077,
        },
        fmt=plistlib.FMT_XML,
        sort_keys=True,
    )
    if len(payload) > MAX_PLIST_BYTES:
        fail("candidate_launchd_plist_invalid")
    return payload


def run_job(argv, cwd, environment, timeout, result_root, nonce, monitor):
    if (
        not isinstance(argv, list)
        or not argv
        or any(not isinstance(value, str) or not value or "\x00" in value for value in argv)
        or not isinstance(environment, dict)
        or any(
            not isinstance(name, str)
            or not isinstance(value, str)
            or not name
            or "\x00" in name
            or "\x00" in value
            for name, value in environment.items()
        )
        or type(timeout) not in (int, float)
        or timeout <= 0
        or not isinstance(nonce, str)
        or len(nonce) != 32
        or any(character not in "0123456789abcdef" for character in nonce)
        or not callable(monitor)
    ):
        fail("candidate_launchd_arguments_invalid")
    selected_cwd = pathlib.Path(cwd)
    selected_root = pathlib.Path(result_root)
    try:
        cwd_metadata = selected_cwd.lstat()
        root_metadata = selected_root.lstat()
    except OSError as error:
        fail(f"candidate_launchd_root_unavailable:{error.__class__.__name__}")
    if (
        not selected_cwd.is_absolute()
        or os.path.realpath(selected_cwd) != str(selected_cwd)
        or not stat.S_ISDIR(cwd_metadata.st_mode)
        or selected_cwd.is_symlink()
        or not selected_root.is_absolute()
        or os.path.realpath(selected_root) != str(selected_root)
        or not stat.S_ISDIR(root_metadata.st_mode)
        or selected_root.is_symlink()
        or root_metadata.st_uid != os.getuid()
        or stat.S_IMODE(root_metadata.st_mode) != 0o700
    ):
        fail("candidate_launchd_root_invalid")
    label = f"co.starring.d3.candidate.{nonce}"
    result_path = selected_root / f".candidate-launchd-result-{nonce}.json"
    pending_path = pathlib.Path(f"{result_path}.pending")
    plist_path = selected_root / f".candidate-launchd-job-{nonce}.plist"
    python = require_executable(PYTHON, "candidate_launchd_python")
    environment_payload = json.dumps(
        environment,
        sort_keys=True,
        separators=(",", ":"),
    )
    program_arguments = [
        str(python),
        "-I",
        "-c",
        LAUNCHER,
        environment_payload,
        str(selected_cwd),
        str(result_path),
        nonce,
        label,
        *argv,
    ]
    plist_payload = job_plist(label, program_arguments)
    result_exists = os.path.lexists(result_path)
    pending_exists = os.path.lexists(pending_path)
    plist_exists = os.path.lexists(plist_path)
    if plist_exists:
        require_private_file(plist_path, plist_payload, "candidate_launchd_plist")
    existing_service, existing_active = service_status(label)
    if existing_service:
        if not plist_exists:
            fail("candidate_launchd_plist_missing")
        if result_exists:
            terminate_service(label)
            monitor()
            code = read_result(result_path, nonce, label)
            remove_private_file(
                plist_path,
                plist_payload,
                "candidate_launchd_plist",
            )
            return code
        if existing_active:
            fail("candidate_launchd_job_active")
        terminate_service(label)
        fail("candidate_launchd_result_missing")
    if result_exists:
        monitor()
        code = read_result(result_path, nonce, label)
        if plist_exists:
            remove_private_file(
                plist_path,
                plist_payload,
                "candidate_launchd_plist",
            )
        return code
    if pending_exists:
        fail("candidate_launchd_result_incomplete")
    if not plist_exists:
        write_private_file(
            plist_path,
            plist_payload,
            "candidate_launchd_plist",
        )
    try:
        launchctl(
            [
                "bootstrap",
                f"gui/{os.getuid()}",
                str(plist_path),
            ],
            "candidate_launchd_bootstrap",
        )
        deadline = time.monotonic() + timeout
        while not os.path.lexists(result_path):
            monitor()
            observed_service, observed_active = service_status(label)
            if not observed_service or not observed_active:
                fail("candidate_launchd_result_missing")
            if time.monotonic() >= deadline:
                fail("candidate_launchd_timeout")
            time.sleep(POLL_SECONDS)
        terminate_service(label)
        monitor()
        code = read_result(result_path, nonce, label)
        remove_private_file(
            plist_path,
            plist_payload,
            "candidate_launchd_plist",
        )
        return code
    except BaseException:
        terminate_service(label)
        raise
