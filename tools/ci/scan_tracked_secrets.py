from __future__ import annotations

import os
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True, order=True)
class Finding:
    path: str
    rule: str


DIRECT_RULES = (
    (
        "cloudflare_api_token",
        re.compile(rb"(?<![A-Za-z0-9_-])cf(?:at|ut)_[A-Za-z0-9_-]{32,}(?![A-Za-z0-9_-])"),
    ),
    (
        "private_key_or_age_handoff",
        re.compile(
            rb"-{5}BEGIN (?:AGE ENCRYPTED FILE|(?:RSA |EC |OPENSSH )?PRIVATE KEY)-{5}"
        ),
    ),
    ("aws_access_key", re.compile(rb"(?<![A-Z0-9])AKIA[0-9A-Z]{16}(?![A-Z0-9])")),
    (
        "github_token",
        re.compile(rb"(?<![A-Za-z0-9_])gh[pousr]_[A-Za-z0-9]{30,}(?![A-Za-z0-9])"),
    ),
    (
        "openai_api_key",
        re.compile(rb"(?<![A-Za-z0-9_-])sk-(?:proj-)?[A-Za-z0-9_-]{32,}(?![A-Za-z0-9_-])"),
    ),
    (
        "slack_token",
        re.compile(rb"(?<![A-Za-z0-9-])xox[baprs]-[A-Za-z0-9-]{24,}(?![A-Za-z0-9-])"),
    ),
    (
        "discord_bot_token",
        re.compile(
            rb"(?<![A-Za-z0-9_-])[A-Za-z0-9_-]{20,30}\.[A-Za-z0-9_-]{6}\.[A-Za-z0-9_-]{25,50}(?![A-Za-z0-9_-])"
        ),
    ),
    (
        "bearer_credential",
        re.compile(
            rb"(?i)\bauthorization\s*:\s*bearer\s+[A-Za-z0-9_./+=-]{20,}"
        ),
    ),
)

DATABASE_CREDENTIAL = re.compile(
    rb"(?i)\bpostgres(?:ql)?://[^:/@\s]+:([^@\s/?#]+)@"
)
SENSITIVE_QUOTED_ASSIGNMENT = re.compile(
    rb"(?i)\b(?:api[_-]?(?:token|key)|client[_-]?secret|discord[_-]?bot[_-]?token|cloudflare[_-]?(?:api[_-]?)?token|access[_-]?key[_-]?id|secret[_-]?access[_-]?key)\b\s*[:=]\s*[\"']([A-Za-z0-9_./+=-]{16,})"
)
SENSITIVE_ENV_ASSIGNMENT = re.compile(
    rb"(?m)^(?:export\s+)?(?:[A-Z][A-Z0-9_]*_)?(?:API_(?:TOKEN|KEY)|CLIENT_SECRET|DISCORD_BOT_TOKEN|CLOUDFLARE_(?:API_)?TOKEN|ACCESS_KEY_ID|SECRET_ACCESS_KEY)=([A-Za-z0-9_./+=-]{16,})\s*$"
)
PLACEHOLDER_VALUES = {
    b"change-me",
    b"client-secret",
    b"example",
    b"password",
    b"postgres",
    b"private-password",
    b"secret",
    b"test-value",
    b"unused",
}
PLACEHOLDER_MARKERS = (
    b"${",
    b"<",
    b">",
    b"example",
    b"fixture",
    b"placeholder",
    b"replace",
)


def is_placeholder(value: bytes) -> bool:
    normalized = value.strip(b"\"'").lower()
    if normalized in PLACEHOLDER_VALUES:
        return True
    if any(marker in normalized for marker in PLACEHOLDER_MARKERS):
        return True
    if b"{" in normalized or b"}" in normalized:
        return True
    return len(set(normalized)) < 4


def scan_content(path: str, content: bytes) -> set[Finding]:
    findings = {
        Finding(path, rule)
        for rule, pattern in DIRECT_RULES
        if pattern.search(content) is not None
    }
    for match in DATABASE_CREDENTIAL.finditer(content):
        if not is_placeholder(match.group(1)):
            findings.add(Finding(path, "database_url_password"))
    for pattern in [SENSITIVE_QUOTED_ASSIGNMENT, SENSITIVE_ENV_ASSIGNMENT]:
        for match in pattern.finditer(content):
            if not is_placeholder(match.group(1)):
                findings.add(Finding(path, "sensitive_assignment"))
    return findings


def repository_root() -> Path:
    result = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"],
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )
    return Path(os.fsdecode(result.stdout.rstrip(b"\n")))


def tracked_paths(root: Path) -> list[str]:
    result = subprocess.run(
        ["git", "ls-files", "-z"],
        cwd=root,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )
    return sorted(os.fsdecode(path) for path in result.stdout.split(b"\0") if path)


def tracked_content(root: Path, relative: str) -> bytes:
    path = root / relative
    if path.is_symlink():
        return os.fsencode(os.readlink(path))
    return path.read_bytes()


def self_test() -> None:
    unsafe = {
        finding.rule
        for finding in scan_content(
            "self-test",
            b"\n".join(
                [
                    b"cf" + b"at_" + b"A9_-" * 10,
                    b"-" * 5 + b"BEGIN AGE ENCRYPTED FILE" + b"-" * 5,
                    b"AK" + b"IA" + b"A1" * 8,
                    b"gh" + b"p_" + b"A1" * 15,
                    b"s" + b"k-" + b"A1" * 16,
                    b"xo" + b"xb-" + b"A1" * 12,
                    b"A1" * 10 + b"." + b"B2" * 3 + b"." + b"C3" * 13,
                    b"Authorization: " + b"Bearer " + b"D4" * 10,
                    b"postgresql://" + b"app:" + b"aB3_" * 8 + b"@db/starring",
                    b"API_" + b"TOKEN=" + b"zY8_" * 8,
                ]
            ),
        )
    }
    expected = {
        "cloudflare_api_token",
        "private_key_or_age_handoff",
        "aws_access_key",
        "github_token",
        "openai_api_key",
        "slack_token",
        "discord_bot_token",
        "bearer_credential",
        "database_url_password",
        "sensitive_assignment",
    }
    if not expected.issubset(unsafe):
        raise RuntimeError("tracked secret scanner self-test failed closed")
    safe = b"\n".join(
        [
            b"cfat_",
            b"cfut_",
            b"postgres://postgres:postgres@localhost/starring_test",
            b"postgresql://app:REPLACE_WITH_RANDOM_VALUE@localhost/starring",
            b"Authorization: Bearer ${API_KEY}",
        ]
    )
    if scan_content("self-test", safe):
        raise RuntimeError("tracked secret scanner rejected documented placeholders")


def main() -> int:
    self_test()
    root = repository_root()
    findings = set()
    for relative in tracked_paths(root):
        try:
            findings.update(scan_content(relative, tracked_content(root, relative)))
        except FileNotFoundError:
            findings.add(Finding(relative, "tracked_file_missing"))
    if findings:
        print("tracked repository secret scan failed", file=sys.stderr)
        for finding in sorted(findings):
            print(f"{finding.path}: {finding.rule}", file=sys.stderr)
        return 1
    print("tracked repository secret scan passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
