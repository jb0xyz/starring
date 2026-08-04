use std::io::{ErrorKind, Read};
use std::os::fd::{AsRawFd, RawFd};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use thiserror::Error;
use zeroize::{Zeroize, Zeroizing};

pub const ADMIN_KEYCHAIN_SERVICE: &str = "starring.postgres.staging";
pub const ADMIN_KEYCHAIN_ACCOUNT: &str = "database.cluster-admin";

const SECURITY_PATH: &str = "/usr/bin/security";
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
const COMMAND_POLL: Duration = Duration::from_millis(10);
const MAX_CAPTURE_BYTES: usize = 16 * 1024;
const MAX_DRAIN_READS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum AdminKeychainErrorV1 {
    #[error("admin_keychain_unsupported_platform")]
    UnsupportedPlatform,
    #[error("admin_keychain_read_failed")]
    Read,
    #[error("admin_keychain_timeout")]
    Timeout,
    #[error("admin_keychain_output_too_large")]
    OutputTooLarge,
    #[error("admin_keychain_value_invalid")]
    InvalidValue,
}

impl AdminKeychainErrorV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::UnsupportedPlatform => "admin_keychain_unsupported_platform",
            Self::Read => "admin_keychain_read_failed",
            Self::Timeout => "admin_keychain_timeout",
            Self::OutputTooLarge => "admin_keychain_output_too_large",
            Self::InvalidValue => "admin_keychain_value_invalid",
        }
    }
}

pub fn read_admin_url_from_keychain() -> Result<Zeroizing<String>, AdminKeychainErrorV1> {
    if !cfg!(target_os = "macos") {
        return Err(AdminKeychainErrorV1::UnsupportedPlatform);
    }
    let command = security_command();
    let captured = run_with_capture(command, COMMAND_TIMEOUT)?;
    decode_output(captured)
}

struct CapturedOutputV1 {
    status: ExitStatus,
    bytes: Zeroizing<Vec<u8>>,
    overflowed: bool,
}

fn security_command() -> Command {
    let mut command = Command::new(SECURITY_PATH);
    command.args([
        "find-generic-password",
        "-w",
        "-s",
        ADMIN_KEYCHAIN_SERVICE,
        "-a",
        ADMIN_KEYCHAIN_ACCOUNT,
    ]);
    configure_child(&mut command);
    command
}

fn configure_child(command: &mut Command) {
    command
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
}

fn run_with_capture(
    mut command: Command,
    timeout: Duration,
) -> Result<CapturedOutputV1, AdminKeychainErrorV1> {
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or(AdminKeychainErrorV1::Timeout)?;
    let mut child = command.spawn().map_err(|_| AdminKeychainErrorV1::Read)?;
    let mut stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            terminate_child(&mut child);
            return Err(AdminKeychainErrorV1::Read);
        }
    };
    if set_nonblocking(stdout.as_raw_fd()).is_err() {
        terminate_child(&mut child);
        return Err(AdminKeychainErrorV1::Read);
    }
    let mut bytes = Zeroizing::new(Vec::with_capacity(MAX_CAPTURE_BYTES));
    let mut buffer = Zeroizing::new([0_u8; 1024]);
    let mut overflowed = false;
    loop {
        if drain_available(&mut stdout, &mut bytes, &mut buffer, &mut overflowed).is_err() {
            terminate_child(&mut child);
            return Err(AdminKeychainErrorV1::Read);
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                if drain_available(&mut stdout, &mut bytes, &mut buffer, &mut overflowed).is_err() {
                    return Err(AdminKeychainErrorV1::Read);
                }
                return Ok(CapturedOutputV1 {
                    status,
                    bytes,
                    overflowed,
                });
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(COMMAND_POLL.min(timeout));
            }
            Ok(None) => {
                terminate_child(&mut child);
                return Err(AdminKeychainErrorV1::Timeout);
            }
            Err(_) => {
                terminate_child(&mut child);
                return Err(AdminKeychainErrorV1::Read);
            }
        }
    }
}

fn set_nonblocking(fd: RawFd) -> Result<(), AdminKeychainErrorV1> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(AdminKeychainErrorV1::Read);
    }
    Ok(())
}

fn drain_available(
    reader: &mut impl Read,
    bytes: &mut Zeroizing<Vec<u8>>,
    buffer: &mut Zeroizing<[u8; 1024]>,
    overflowed: &mut bool,
) -> Result<(), AdminKeychainErrorV1> {
    for _ in 0..MAX_DRAIN_READS {
        match reader.read(&mut **buffer) {
            Ok(0) => return Ok(()),
            Ok(read) => {
                let remaining = MAX_CAPTURE_BYTES.saturating_sub(bytes.len());
                let retained = remaining.min(read);
                bytes.extend_from_slice(&buffer[..retained]);
                *overflowed |= retained != read;
                buffer[..read].zeroize();
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(()),
            Err(_) => return Err(AdminKeychainErrorV1::Read),
        }
    }
    Ok(())
}

fn terminate_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn decode_output(
    mut captured: CapturedOutputV1,
) -> Result<Zeroizing<String>, AdminKeychainErrorV1> {
    if !captured.status.success() {
        return Err(AdminKeychainErrorV1::Read);
    }
    if captured.overflowed {
        return Err(AdminKeychainErrorV1::OutputTooLarge);
    }
    if captured.bytes.last() == Some(&b'\n') {
        captured.bytes.pop();
        if captured.bytes.last() == Some(&b'\r') {
            captured.bytes.pop();
        }
    }
    if captured.bytes.is_empty()
        || captured
            .bytes
            .iter()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
        || std::str::from_utf8(&captured.bytes).is_err()
    {
        return Err(AdminKeychainErrorV1::InvalidValue);
    }
    let bytes = std::mem::take(&mut *captured.bytes);
    let value = String::from_utf8(bytes).map_err(|_| AdminKeychainErrorV1::InvalidValue)?;
    Ok(Zeroizing::new(value))
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;

    use super::*;

    fn status(code: i32) -> ExitStatus {
        ExitStatus::from_raw(code << 8)
    }

    fn captured(status_code: i32, bytes: &[u8], overflowed: bool) -> CapturedOutputV1 {
        CapturedOutputV1 {
            status: status(status_code),
            bytes: Zeroizing::new(bytes.to_vec()),
            overflowed,
        }
    }

    #[test]
    fn security_command_shape_is_fixed_and_noninteractive() {
        let command = security_command();
        assert_eq!(command.get_program(), SECURITY_PATH);
        assert_eq!(
            command
                .get_args()
                .map(|argument| argument.to_str().unwrap())
                .collect::<Vec<_>>(),
            [
                "find-generic-password",
                "-w",
                "-s",
                "starring.postgres.staging",
                "-a",
                "database.cluster-admin",
            ]
        );
        assert!(command.get_envs().next().is_none());
    }

    #[test]
    fn configured_child_receives_no_inherited_or_explicit_environment() {
        let mut command = Command::new("/usr/bin/env");
        command.env("STARRING_DB_BOOTSTRAP_TEST_SECRET", "must-not-cross");
        configure_child(&mut command);
        let captured = run_with_capture(command, Duration::from_secs(1)).unwrap();
        assert!(captured.status.success());
        assert!(captured.bytes.is_empty());
        assert!(!captured.overflowed);
    }

    #[test]
    fn bounded_capture_retains_only_the_limit_and_marks_overflow() {
        let mut command = Command::new("/usr/bin/printf");
        command.arg("x".repeat(MAX_CAPTURE_BYTES + 1));
        configure_child(&mut command);
        let captured = run_with_capture(command, Duration::from_secs(1)).unwrap();
        assert_eq!(captured.bytes.len(), MAX_CAPTURE_BYTES);
        assert!(captured.overflowed);
        assert_eq!(
            decode_output(captured),
            Err(AdminKeychainErrorV1::OutputTooLarge)
        );
    }

    #[test]
    fn command_status_and_deadline_are_bounded_and_redacted() {
        assert_eq!(
            decode_output(captured(23, b"postgresql://sensitive", false)),
            Err(AdminKeychainErrorV1::Read)
        );
        let mut command = Command::new("/bin/sleep");
        command.arg("1");
        configure_child(&mut command);
        assert!(matches!(
            run_with_capture(command, Duration::from_millis(20)),
            Err(AdminKeychainErrorV1::Timeout)
        ));
    }

    #[test]
    fn inherited_stdout_cannot_extend_the_capture_deadline() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "(/bin/sleep 1) & /usr/bin/printf late-secret-value"]);
        configure_child(&mut command);
        let started_at = Instant::now();
        let captured = run_with_capture(command, Duration::from_millis(100)).unwrap();
        assert!(started_at.elapsed() < Duration::from_millis(500));
        assert_eq!(
            decode_output(captured).unwrap().as_str(),
            "late-secret-value"
        );
    }

    #[test]
    fn continuous_output_cannot_extend_the_process_deadline() {
        let mut command = Command::new("/usr/bin/yes");
        configure_child(&mut command);
        let started_at = Instant::now();
        assert!(matches!(
            run_with_capture(command, Duration::from_millis(30)),
            Err(AdminKeychainErrorV1::Timeout)
        ));
        assert!(started_at.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn output_decoder_accepts_one_line_ending_and_rejects_malformed_values() {
        assert_eq!(
            decode_output(captured(0, b"postgresql://value\r\n", false))
                .unwrap()
                .as_str(),
            "postgresql://value"
        );
        for value in [
            b"".as_slice(),
            b"\n".as_slice(),
            b"postgresql://value\nextra".as_slice(),
            b"postgresql://value ".as_slice(),
            b"postgresql://value\0".as_slice(),
            &[0xff],
        ] {
            assert_eq!(
                decode_output(captured(0, value, false)),
                Err(AdminKeychainErrorV1::InvalidValue)
            );
        }
    }

    #[test]
    fn errors_are_stable_and_never_include_captured_values() {
        let sensitive = "postgresql://starring_cluster_admin:secret@127.0.0.1/postgres";
        for error in [
            AdminKeychainErrorV1::UnsupportedPlatform,
            AdminKeychainErrorV1::Read,
            AdminKeychainErrorV1::Timeout,
            AdminKeychainErrorV1::OutputTooLarge,
            AdminKeychainErrorV1::InvalidValue,
        ] {
            assert_eq!(error.to_string(), error.code());
            assert!(!error.to_string().contains(sensitive));
        }
    }
}
