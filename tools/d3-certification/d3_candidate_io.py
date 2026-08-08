import ctypes
import errno
import hashlib
import json
import os
import pathlib
import stat


MAX_JSON_BYTES = 1024 * 1024
MAX_SOURCE_FILE_BYTES = 16 * 1024 * 1024
MAX_SOURCE_TREE_BYTES = 64 * 1024 * 1024


class CandidateBundleError(Exception):
    pass


def fail(code):
    raise CandidateBundleError(code)


def canonical_json(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def sha256_bytes(value):
    return hashlib.sha256(value).hexdigest()


def seal_record(value):
    record = dict(value)
    record["record_sha256"] = sha256_bytes(canonical_json(record).encode("utf-8"))
    return record


def verify_sealed_record(value, label):
    if not isinstance(value, dict):
        fail(f"{label}_invalid")
    digest = value.get("record_sha256")
    if not isinstance(digest, str) or len(digest) != 64:
        fail(f"{label}_record_invalid")
    payload = dict(value)
    payload.pop("record_sha256")
    if sha256_bytes(canonical_json(payload).encode("utf-8")) != digest:
        fail(f"{label}_record_mismatch")


def absolute_path(raw, label):
    path = pathlib.Path(raw)
    if not path.is_absolute() or os.path.realpath(path) != str(path):
        fail(f"{label}_path_invalid")
    return path


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
            fail("candidate_bundle_write_failed")
        remaining = remaining[written:]


def write_new_file(path, payload, mode):
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    descriptor = os.open(path, flags, mode)
    try:
        write_all(descriptor, payload)
        os.fsync(descriptor)
    finally:
        os.close(descriptor)
    fsync_directory(path.parent)


def write_new_file_atomic(path, temporary_name, payload, mode):
    temporary = path.parent / temporary_name
    write_new_file(temporary, payload, mode)
    parent = os.open(path.parent, os.O_RDONLY)
    try:
        rename_exclusive(parent, temporary.name, path.name)
        os.fsync(parent)
    finally:
        os.close(parent)


def unlink_owned_regular(path, mode):
    try:
        metadata = path.lstat()
    except OSError as error:
        fail(f"candidate_bundle_temporary_unavailable:{error.__class__.__name__}")
    if (
        not stat.S_ISREG(metadata.st_mode)
        or path.is_symlink()
        or metadata.st_uid != os.getuid()
        or metadata.st_nlink != 1
        or stat.S_IMODE(metadata.st_mode) != mode
    ):
        fail("candidate_bundle_temporary_identity_invalid")
    path.unlink()
    fsync_directory(path.parent)


def strict_object(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            fail("candidate_bundle_json_duplicate_key")
        value[key] = item
    return value


def require_directory(path, label, mode=None):
    try:
        metadata = path.lstat()
    except OSError as error:
        fail(f"{label}_unavailable:{error.__class__.__name__}")
    if (
        not stat.S_ISDIR(metadata.st_mode)
        or path.is_symlink()
        or metadata.st_uid != os.getuid()
        or mode is not None
        and stat.S_IMODE(metadata.st_mode) != mode
    ):
        fail(f"{label}_identity_invalid")
    return metadata


def open_regular_snapshot(
    path, label, expected_mode=None, writable_forbidden=True, allow_empty=False
):
    flags = os.O_RDONLY
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        fail(f"{label}_unavailable:{error.__class__.__name__}")
    metadata = os.fstat(descriptor)
    mode = stat.S_IMODE(metadata.st_mode)
    if (
        not stat.S_ISREG(metadata.st_mode)
        or metadata.st_uid != os.getuid()
        or metadata.st_nlink != 1
        or not allow_empty
        and metadata.st_size <= 0
        or expected_mode is not None
        and mode != expected_mode
        or writable_forbidden
        and mode & 0o022
    ):
        os.close(descriptor)
        fail(f"{label}_identity_invalid")
    return descriptor, metadata


def read_json(path, label, mode):
    descriptor, metadata = open_regular_snapshot(
        path, label, expected_mode=mode, writable_forbidden=False
    )
    try:
        if metadata.st_size > MAX_JSON_BYTES:
            fail(f"{label}_size_invalid")
        raw = bytearray()
        while True:
            chunk = os.read(descriptor, 65536)
            if not chunk:
                break
            raw.extend(chunk)
            if len(raw) > MAX_JSON_BYTES:
                fail(f"{label}_size_invalid")
        after = os.fstat(descriptor)
    finally:
        os.close(descriptor)
    if (
        metadata.st_dev,
        metadata.st_ino,
        metadata.st_mode,
        metadata.st_size,
    ) != (after.st_dev, after.st_ino, after.st_mode, after.st_size) or len(raw) != metadata.st_size:
        fail(f"{label}_changed_during_read")
    try:
        return json.loads(bytes(raw), object_pairs_hook=strict_object)
    except (UnicodeDecodeError, json.JSONDecodeError):
        fail(f"{label}_json_invalid")


def descriptor_digest(descriptor):
    digest = hashlib.sha256()
    os.lseek(descriptor, 0, os.SEEK_SET)
    size = 0
    while True:
        chunk = os.read(descriptor, 1024 * 1024)
        if not chunk:
            break
        digest.update(chunk)
        size += len(chunk)
    return digest.hexdigest(), size


def stable_file_identity(metadata):
    return (
        metadata.st_dev,
        metadata.st_ino,
        metadata.st_mode,
        metadata.st_uid,
        metadata.st_gid,
        metadata.st_nlink,
        metadata.st_size,
        metadata.st_mtime_ns,
        metadata.st_ctime_ns,
    )


def named_file_identity(path, label):
    try:
        metadata = os.stat(path, follow_symlinks=False)
    except OSError as error:
        fail(f"{label}_unavailable:{error.__class__.__name__}")
    if not stat.S_ISREG(metadata.st_mode):
        fail(f"{label}_identity_invalid")
    return metadata


def file_identity(path, label, expected=None, expected_mode=None):
    descriptor, before = open_regular_snapshot(path, label, expected_mode=expected_mode)
    try:
        digest, size = descriptor_digest(descriptor)
        after = os.fstat(descriptor)
    finally:
        os.close(descriptor)
    if (
        before.st_dev,
        before.st_ino,
        before.st_mode,
        before.st_size,
    ) != (after.st_dev, after.st_ino, after.st_mode, after.st_size) or size != before.st_size:
        fail(f"{label}_changed_during_read")
    value = {
        "path": str(path),
        "sha256": digest,
        "size": size,
        "mode": stat.S_IMODE(before.st_mode),
        "uid": before.st_uid,
        "device": before.st_dev,
        "inode": before.st_ino,
        "links": before.st_nlink,
    }
    if expected is not None and value != expected:
        fail(f"{label}_identity_mismatch")
    return value


def system_file_identity(path, label):
    flags = os.O_RDONLY
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        fail(f"{label}_unavailable:{error.__class__.__name__}")
    before = os.fstat(descriptor)
    mode = stat.S_IMODE(before.st_mode)
    if (
        not stat.S_ISREG(before.st_mode)
        or before.st_uid != 0
        or before.st_nlink != 1
        or before.st_size <= 0
        or mode & 0o022
    ):
        os.close(descriptor)
        fail(f"{label}_identity_invalid")
    try:
        digest, size = descriptor_digest(descriptor)
        after = os.fstat(descriptor)
    finally:
        os.close(descriptor)
    if (
        before.st_dev,
        before.st_ino,
        before.st_mode,
        before.st_size,
    ) != (
        after.st_dev,
        after.st_ino,
        after.st_mode,
        after.st_size,
    ) or size != before.st_size:
        fail(f"{label}_changed_during_read")
    return {
        "path": str(path),
        "sha256": digest,
        "size": size,
        "mode": mode,
        "uid": before.st_uid,
        "device": before.st_dev,
        "inode": before.st_ino,
        "links": before.st_nlink,
    }


def directory_identity(path, label, mode):
    metadata = require_directory(path, label, mode)
    return {
        "path": str(path),
        "mode": stat.S_IMODE(metadata.st_mode),
        "uid": metadata.st_uid,
        "device": metadata.st_dev,
        "inode": metadata.st_ino,
    }


def verify_directory_identity(path, value, label):
    if not isinstance(value, dict) or set(value) != {
        "path",
        "mode",
        "uid",
        "device",
        "inode",
    }:
        fail(f"{label}_fields_invalid")
    for name in ("mode", "uid", "device", "inode"):
        if type(value[name]) is not int:
            fail(f"{label}_fields_invalid")
    if directory_identity(path, label, value["mode"]) != value:
        fail(f"{label}_identity_mismatch")


def source_tree_digest(root, files, label):
    root = absolute_path(str(root), f"{label}_root")
    metadata = require_directory(root, f"{label}_root")
    if stat.S_IMODE(metadata.st_mode) & 0o022:
        fail(f"{label}_root_mode_invalid")
    digest = hashlib.sha256()
    total = 0
    for name in files:
        path = absolute_path(str(root / name), f"{label}_{name}")
        descriptor, before = open_regular_snapshot(path, f"{label}_{name}")
        try:
            if before.st_size > MAX_SOURCE_FILE_BYTES:
                fail(f"{label}_{name}_size_invalid")
            total += before.st_size
            if total > MAX_SOURCE_TREE_BYTES:
                fail(f"{label}_size_invalid")
            encoded = name.encode("utf-8")
            digest.update(str(len(encoded)).encode("ascii"))
            digest.update(b":")
            digest.update(encoded)
            digest.update(b":")
            digest.update(str(before.st_size).encode("ascii"))
            digest.update(b":")
            size = 0
            while True:
                chunk = os.read(descriptor, 1024 * 1024)
                if not chunk:
                    break
                digest.update(chunk)
                size += len(chunk)
            after = os.fstat(descriptor)
        finally:
            os.close(descriptor)
        if (
            before.st_dev,
            before.st_ino,
            before.st_mode,
            before.st_size,
        ) != (after.st_dev, after.st_ino, after.st_mode, after.st_size) or size != before.st_size:
            fail(f"{label}_{name}_changed_during_read")
    return digest.hexdigest()


def copy_snapshot(source, destination, destination_mode, label):
    source = absolute_path(str(source), f"{label}_source")
    source_descriptor, before = open_regular_snapshot(source, f"{label}_source")
    flags = os.O_RDWR | os.O_CREAT | os.O_EXCL
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        destination_descriptor = os.open(destination, flags, destination_mode)
    except OSError as error:
        os.close(source_descriptor)
        fail(f"{label}_destination_unavailable:{error.__class__.__name__}")
    digest = hashlib.sha256()
    size = 0
    try:
        while True:
            chunk = os.read(source_descriptor, 1024 * 1024)
            if not chunk:
                break
            digest.update(chunk)
            size += len(chunk)
            write_all(destination_descriptor, chunk)
        after = os.fstat(source_descriptor)
        if (
            stable_file_identity(before) != stable_file_identity(after)
            or size != before.st_size
        ):
            fail(f"{label}_source_changed_during_copy")
        verified_digest, verified_size = descriptor_digest(source_descriptor)
        verified = os.fstat(source_descriptor)
        if (
            stable_file_identity(after) != stable_file_identity(verified)
            or verified_size != size
            or verified_digest != digest.hexdigest()
        ):
            fail(f"{label}_source_changed_during_copy")
        named_source = named_file_identity(source, f"{label}_source")
        if stable_file_identity(named_source) != stable_file_identity(verified):
            fail(f"{label}_source_path_changed_during_copy")
        os.fchmod(destination_descriptor, destination_mode)
        os.fsync(destination_descriptor)
        destination_before_verify = os.fstat(destination_descriptor)
        destination_digest, destination_size = descriptor_digest(
            destination_descriptor
        )
        destination_metadata = os.fstat(destination_descriptor)
        if (
            stable_file_identity(destination_before_verify)
            != stable_file_identity(destination_metadata)
            or destination_size != size
            or destination_digest != digest.hexdigest()
        ):
            fail(f"{label}_destination_changed_during_copy")
        named_destination = named_file_identity(destination, f"{label}_destination")
        if stable_file_identity(named_destination) != stable_file_identity(
            destination_metadata
        ):
            fail(f"{label}_destination_path_changed_during_copy")
    finally:
        os.close(source_descriptor)
        os.close(destination_descriptor)
    if (
        not stat.S_ISREG(destination_metadata.st_mode)
        or destination_metadata.st_uid != os.getuid()
        or destination_metadata.st_nlink != 1
        or destination_metadata.st_size != size
        or stat.S_IMODE(destination_metadata.st_mode) != destination_mode
    ):
        fail(f"{label}_destination_identity_invalid")
    source_identity = {
        "path": str(source),
        "sha256": digest.hexdigest(),
        "size": size,
        "mode": stat.S_IMODE(before.st_mode),
        "uid": before.st_uid,
        "device": before.st_dev,
        "inode": before.st_ino,
        "links": before.st_nlink,
    }
    destination_identity = {
        "path": str(destination),
        "sha256": digest.hexdigest(),
        "size": size,
        "mode": stat.S_IMODE(destination_metadata.st_mode),
        "uid": destination_metadata.st_uid,
        "device": destination_metadata.st_dev,
        "inode": destination_metadata.st_ino,
        "links": destination_metadata.st_nlink,
    }
    return source_identity, destination_identity


def rename_exclusive(parent, source_name, destination_name):
    library = ctypes.CDLL(None, use_errno=True)
    if hasattr(library, "renameatx_np"):
        operation = library.renameatx_np
        flags = 0x00000004
    elif hasattr(library, "renameat2"):
        operation = library.renameat2
        flags = 0x00000001
    else:
        fail("candidate_bundle_exclusive_rename_unavailable")
    operation.argtypes = [
        ctypes.c_int,
        ctypes.c_char_p,
        ctypes.c_int,
        ctypes.c_char_p,
        ctypes.c_uint,
    ]
    operation.restype = ctypes.c_int
    ctypes.set_errno(0)
    if operation(
        parent,
        os.fsencode(source_name),
        parent,
        os.fsencode(destination_name),
        flags,
    ) != 0:
        error = ctypes.get_errno()
        if error == errno.EEXIST:
            fail("candidate_bundle_destination_exists")
        fail(f"candidate_bundle_publish_failed:{error}")


def remove_tree_descriptor(descriptor):
    for name in os.listdir(descriptor):
        metadata = os.stat(name, dir_fd=descriptor, follow_symlinks=False)
        if stat.S_ISDIR(metadata.st_mode) and not stat.S_ISLNK(metadata.st_mode):
            flags = os.O_RDONLY
            if hasattr(os, "O_DIRECTORY"):
                flags |= os.O_DIRECTORY
            if hasattr(os, "O_NOFOLLOW"):
                flags |= os.O_NOFOLLOW
            child = os.open(name, flags, dir_fd=descriptor)
            try:
                observed = os.fstat(child)
                if (observed.st_dev, observed.st_ino) != (
                    metadata.st_dev,
                    metadata.st_ino,
                ):
                    fail("candidate_bundle_discard_identity_drift")
                if observed.st_uid != os.getuid():
                    fail("candidate_bundle_discard_owner_drift")
                os.fchmod(child, 0o700)
                remove_tree_descriptor(child)
            finally:
                os.close(child)
            os.rmdir(name, dir_fd=descriptor)
        else:
            os.unlink(name, dir_fd=descriptor)
    os.fsync(descriptor)


def normalized_file_identity(path, recorded_path, label, mode):
    value = file_identity(path, label, expected_mode=mode)
    value["path"] = str(recorded_path)
    return value
