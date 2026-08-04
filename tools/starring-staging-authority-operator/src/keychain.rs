use std::io::Read;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use zeroize::{Zeroize, Zeroizing};

use crate::AuthorityOperatorErrorV1;

pub(crate) const ADMIN_KEYCHAIN_SERVICE: &str = "starring.postgres.staging";
pub(crate) const ADMIN_KEYCHAIN_ACCOUNT: &str = "database.cluster-admin";
const SECURITY_PATH: &str = "/usr/bin/security";
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
const COMMAND_POLL: Duration = Duration::from_millis(10);
const MAX_CAPTURE_BYTES: usize = 16 * 1024;

pub(crate) fn read_admin_database_url() -> Result<Zeroizing<Vec<u8>>, AuthorityOperatorErrorV1> {
    if !cfg!(target_os = "macos") {
        return Err(AuthorityOperatorErrorV1::UnsupportedPlatform);
    }
    let mut command = Command::new(SECURITY_PATH);
    command
        .args([
            "find-generic-password",
            "-w",
            "-s",
            ADMIN_KEYCHAIN_SERVICE,
            "-a",
            ADMIN_KEYCHAIN_ACCOUNT,
        ])
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let (status, mut output) = run_with_capture(command)?;
    if !status.success() {
        return Err(AuthorityOperatorErrorV1::KeychainRead);
    }
    if output.last() == Some(&b'\n') {
        output.pop();
        if output.last() == Some(&b'\r') {
            output.pop();
        }
    }
    if output.is_empty() {
        return Err(AuthorityOperatorErrorV1::KeychainRead);
    }
    Ok(output)
}

struct BoundedCaptureV1 {
    bytes: Zeroizing<Vec<u8>>,
    overflowed: bool,
}

fn run_with_capture(
    mut command: Command,
) -> Result<(ExitStatus, Zeroizing<Vec<u8>>), AuthorityOperatorErrorV1> {
    let mut child = command
        .spawn()
        .map_err(|_| AuthorityOperatorErrorV1::KeychainRead)?;
    let stdout = child
        .stdout
        .take()
        .ok_or(AuthorityOperatorErrorV1::KeychainRead)?;
    let capture = thread::Builder::new()
        .name("starring-authority-keychain".to_string())
        .spawn(move || capture_bounded(stdout))
        .map_err(|_| AuthorityOperatorErrorV1::KeychainRead)?;
    let status = wait_child(&mut child)?;
    let captured = capture
        .join()
        .map_err(|_| AuthorityOperatorErrorV1::KeychainRead)??;
    if captured.overflowed {
        return Err(AuthorityOperatorErrorV1::KeychainRead);
    }
    Ok((status, captured.bytes))
}

fn capture_bounded(mut reader: impl Read) -> Result<BoundedCaptureV1, AuthorityOperatorErrorV1> {
    let mut bytes = Zeroizing::new(Vec::with_capacity(1024));
    let mut buffer = Zeroizing::new([0_u8; 1024]);
    let mut overflowed = false;
    loop {
        let read = reader
            .read(&mut *buffer)
            .map_err(|_| AuthorityOperatorErrorV1::KeychainRead)?;
        if read == 0 {
            break;
        }
        let remaining = MAX_CAPTURE_BYTES.saturating_sub(bytes.len());
        let retained = remaining.min(read);
        bytes.extend_from_slice(&buffer[..retained]);
        overflowed |= retained != read;
        buffer[..read].zeroize();
    }
    Ok(BoundedCaptureV1 { bytes, overflowed })
}

fn wait_child(child: &mut Child) -> Result<ExitStatus, AuthorityOperatorErrorV1> {
    let deadline = Instant::now()
        .checked_add(COMMAND_TIMEOUT)
        .ok_or(AuthorityOperatorErrorV1::KeychainTimeout)?;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(COMMAND_POLL),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AuthorityOperatorErrorV1::KeychainTimeout);
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AuthorityOperatorErrorV1::KeychainRead);
            }
        }
    }
}
