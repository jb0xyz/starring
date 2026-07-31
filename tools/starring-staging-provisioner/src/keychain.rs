use std::io::{Read, Write};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use subtle::ConstantTimeEq;
use zeroize::{Zeroize, Zeroizing};

use crate::crypto::SecretItemRefV1;
use crate::identity::{
    KeychainIdentityV1, DISCORD_PREFLIGHT_IDENTITIES, INTERACTION_TOKEN_ENVELOPE_KEYRING_IDENTITY,
};
use crate::ProvisionerErrorV1;

const SECURITY_PATH: &str = "/usr/bin/security";
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
const COMMAND_POLL: Duration = Duration::from_millis(10);
const MAX_CAPTURE_BYTES: usize = 16 * 1024;
const KEYCHAIN_ITEM_NOT_FOUND_EXIT: i32 = 44;

pub struct KeychainClientV1;

impl KeychainClientV1 {
    pub fn new() -> Result<Self, ProvisionerErrorV1> {
        if !cfg!(target_os = "macos") {
            return Err(ProvisionerErrorV1::UnsupportedPlatform);
        }
        Ok(Self)
    }

    pub fn preflight_discord(&self) -> Result<(), ProvisionerErrorV1> {
        let oauth = self
            .read_required(DISCORD_PREFLIGHT_IDENTITIES[0])
            .map_err(|_| ProvisionerErrorV1::DiscordPreflight)?;
        let api_bot = self
            .read_required(DISCORD_PREFLIGHT_IDENTITIES[1])
            .map_err(|_| ProvisionerErrorV1::DiscordPreflight)?;
        let runtime_bot = self
            .read_required(DISCORD_PREFLIGHT_IDENTITIES[2])
            .map_err(|_| ProvisionerErrorV1::DiscordPreflight)?;
        if oauth.is_empty()
            || api_bot.is_empty()
            || runtime_bot.is_empty()
            || api_bot.as_slice() != runtime_bot.as_slice()
        {
            return Err(ProvisionerErrorV1::DiscordPreflight);
        }
        Ok(())
    }

    pub fn read_required(
        &self,
        identity: KeychainIdentityV1,
    ) -> Result<Zeroizing<Vec<u8>>, ProvisionerErrorV1> {
        match self.read_optional(identity)? {
            Some(value) if !value.is_empty() => Ok(value),
            _ => Err(ProvisionerErrorV1::KeychainRead),
        }
    }

    pub fn begin_update(
        &self,
        items: &[SecretItemRefV1<'_>],
    ) -> Result<KeychainUpdateV1, ProvisionerErrorV1> {
        let mut backups = Vec::with_capacity(items.len());
        for item in items {
            backups.push(KeychainBackupEntryV1 {
                identity: item.identity,
                previous: self.read_optional(item.identity)?,
                created: None,
            });
        }
        for (written, item) in items.iter().enumerate() {
            if self.write_and_verify(item.identity, item.value).is_err() {
                if self.restore_prefix(&backups, written + 1).is_err() {
                    return Err(ProvisionerErrorV1::KeychainRollback);
                }
                return Err(ProvisionerErrorV1::KeychainWrite);
            }
        }
        Ok(KeychainUpdateV1 {
            backups,
            client: Self,
            completed: false,
        })
    }

    pub(crate) fn begin_create(
        &self,
        item: SecretItemRefV1<'_>,
    ) -> Result<KeychainUpdateV1, ProvisionerErrorV1> {
        self.begin_create_with_conflict(
            item,
            ProvisionerErrorV1::IncrementalAuthoringWriterPartialState,
        )
    }

    pub(crate) fn begin_create_interaction_token_keyring(
        &self,
        item: SecretItemRefV1<'_>,
    ) -> Result<KeychainUpdateV1, ProvisionerErrorV1> {
        if item.identity != INTERACTION_TOKEN_ENVELOPE_KEYRING_IDENTITY {
            return Err(ProvisionerErrorV1::KeyringContract);
        }
        self.begin_create_with_conflict(
            item,
            ProvisionerErrorV1::IncrementalInteractionTokenKeyringBusy,
        )
    }

    fn begin_create_with_conflict(
        &self,
        item: SecretItemRefV1<'_>,
        conflict: ProvisionerErrorV1,
    ) -> Result<KeychainUpdateV1, ProvisionerErrorV1> {
        if self.read_optional(item.identity)?.is_some() {
            return Err(conflict);
        }
        self.write_new_and_verify(item.identity, item.value)?;
        Ok(KeychainUpdateV1 {
            backups: vec![KeychainBackupEntryV1 {
                identity: item.identity,
                previous: None,
                created: Some(Zeroizing::new(item.value.to_vec())),
            }],
            client: Self,
            completed: false,
        })
    }

    pub(crate) fn read_optional(
        &self,
        identity: KeychainIdentityV1,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, ProvisionerErrorV1> {
        let mut command = Command::new(SECURITY_PATH);
        command
            .args([
                "find-generic-password",
                "-w",
                "-s",
                identity.service,
                "-a",
                identity.account,
            ])
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let (status, mut output) = run_with_capture(command)?;
        if status.success() {
            if output.last() == Some(&b'\n') {
                output.pop();
                if output.last() == Some(&b'\r') {
                    output.pop();
                }
            }
            if output.is_empty() {
                return Err(ProvisionerErrorV1::KeychainRead);
            }
            return Ok(Some(output));
        }
        if status.code() == Some(KEYCHAIN_ITEM_NOT_FOUND_EXIT) {
            return Ok(None);
        }
        Err(ProvisionerErrorV1::KeychainRead)
    }

    fn write_and_verify(
        &self,
        identity: KeychainIdentityV1,
        value: &[u8],
    ) -> Result<(), ProvisionerErrorV1> {
        self.write_with_mode_and_verify(identity, value, true)
    }

    fn write_new_and_verify(
        &self,
        identity: KeychainIdentityV1,
        value: &[u8],
    ) -> Result<(), ProvisionerErrorV1> {
        self.write_with_mode_and_verify(identity, value, false)
    }

    fn write_with_mode_and_verify(
        &self,
        identity: KeychainIdentityV1,
        value: &[u8],
        update: bool,
    ) -> Result<(), ProvisionerErrorV1> {
        if value.is_empty()
            || value.len() > MAX_CAPTURE_BYTES
            || value.iter().any(|byte| byte.is_ascii_control())
        {
            return Err(ProvisionerErrorV1::KeychainWrite);
        }
        let mut command = Command::new(SECURITY_PATH);
        command
            .arg("-i")
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = command
            .spawn()
            .map_err(|_| ProvisionerErrorV1::KeychainWrite)?;
        let mut input = Zeroizing::new(Vec::with_capacity(
            48 + identity.service.len() + identity.account.len() + value.len() * 2,
        ));
        input.extend_from_slice(b"add-generic-password ");
        if update {
            input.extend_from_slice(b"-U ");
        }
        input.extend_from_slice(b"-s ");
        input.extend_from_slice(identity.service.as_bytes());
        input.extend_from_slice(b" -a ");
        input.extend_from_slice(identity.account.as_bytes());
        input.extend_from_slice(b" -X ");
        for byte in value {
            input.push(b"0123456789abcdef"[(byte >> 4) as usize]);
            input.push(b"0123456789abcdef"[(byte & 0x0f) as usize]);
        }
        input.push(b'\n');
        let write_result = child
            .stdin
            .take()
            .ok_or(ProvisionerErrorV1::KeychainWrite)
            .and_then(|mut stdin| {
                stdin
                    .write_all(&input)
                    .map_err(|_| ProvisionerErrorV1::KeychainWrite)
            });
        let status = wait_child(&mut child)?;
        write_result?;
        if !status.success() {
            return Err(ProvisionerErrorV1::KeychainWrite);
        }
        let readback = match self.read_required(identity) {
            Ok(readback) => readback,
            Err(_) if !update => return Err(ProvisionerErrorV1::KeychainWrite),
            Err(error) => return Err(error),
        };
        if readback.len() != value.len() || !bool::from(readback.as_slice().ct_eq(value)) {
            return Err(ProvisionerErrorV1::KeychainWrite);
        }
        Ok(())
    }

    fn delete(&self, identity: KeychainIdentityV1) -> Result<(), ProvisionerErrorV1> {
        let mut command = Command::new(SECURITY_PATH);
        command
            .args([
                "delete-generic-password",
                "-s",
                identity.service,
                "-a",
                identity.account,
            ])
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = command
            .spawn()
            .map_err(|_| ProvisionerErrorV1::KeychainRollback)?;
        let status = wait_child(&mut child)?;
        if status.success() || status.code() == Some(KEYCHAIN_ITEM_NOT_FOUND_EXIT) {
            Ok(())
        } else {
            Err(ProvisionerErrorV1::KeychainRollback)
        }
    }

    fn restore_prefix(
        &self,
        backups: &[KeychainBackupEntryV1],
        written: usize,
    ) -> Result<(), ProvisionerErrorV1> {
        let mut failed = false;
        for backup in backups[..written].iter().rev() {
            let result = match &backup.previous {
                Some(value) => self.write_and_verify(backup.identity, value),
                None => self.delete_created(backup),
            };
            failed |= result.is_err();
        }
        if failed {
            Err(ProvisionerErrorV1::KeychainRollback)
        } else {
            Ok(())
        }
    }

    fn delete_created(&self, backup: &KeychainBackupEntryV1) -> Result<(), ProvisionerErrorV1> {
        let Some(expected) = &backup.created else {
            return self.delete(backup.identity);
        };
        match self.read_optional(backup.identity)? {
            None => Ok(()),
            Some(current)
                if current.len() == expected.len()
                    && bool::from(current.as_slice().ct_eq(expected.as_slice())) =>
            {
                self.delete(backup.identity)
            }
            Some(_) => Err(ProvisionerErrorV1::KeychainRollback),
        }
    }
}

struct KeychainBackupEntryV1 {
    identity: KeychainIdentityV1,
    previous: Option<Zeroizing<Vec<u8>>>,
    created: Option<Zeroizing<Vec<u8>>>,
}

pub struct KeychainUpdateV1 {
    backups: Vec<KeychainBackupEntryV1>,
    client: KeychainClientV1,
    completed: bool,
}

impl KeychainUpdateV1 {
    pub fn commit(mut self) {
        self.completed = true;
    }

    pub fn rollback(mut self) -> Result<(), ProvisionerErrorV1> {
        let result = self
            .client
            .restore_prefix(&self.backups, self.backups.len());
        self.completed = true;
        result
    }
}

impl Drop for KeychainUpdateV1 {
    fn drop(&mut self) {
        if !self.completed {
            let _ = self
                .client
                .restore_prefix(&self.backups, self.backups.len());
        }
    }
}

struct BoundedCaptureV1 {
    bytes: Zeroizing<Vec<u8>>,
    overflowed: bool,
}

fn run_with_capture(
    mut command: Command,
) -> Result<(ExitStatus, Zeroizing<Vec<u8>>), ProvisionerErrorV1> {
    let mut child = command
        .spawn()
        .map_err(|_| ProvisionerErrorV1::KeychainRead)?;
    let stdout = child
        .stdout
        .take()
        .ok_or(ProvisionerErrorV1::KeychainRead)?;
    let capture = thread::Builder::new()
        .name("starring-provisioner-keychain".to_string())
        .spawn(move || capture_bounded(stdout))
        .map_err(|_| ProvisionerErrorV1::KeychainRead)?;
    let status = wait_child(&mut child)?;
    let captured = capture
        .join()
        .map_err(|_| ProvisionerErrorV1::KeychainRead)??;
    if captured.overflowed {
        return Err(ProvisionerErrorV1::KeychainRead);
    }
    Ok((status, captured.bytes))
}

fn capture_bounded(mut reader: impl Read) -> Result<BoundedCaptureV1, ProvisionerErrorV1> {
    let mut bytes = Zeroizing::new(Vec::with_capacity(1024));
    let mut buffer = Zeroizing::new([0_u8; 1024]);
    let mut overflowed = false;
    loop {
        let read = reader
            .read(&mut *buffer)
            .map_err(|_| ProvisionerErrorV1::KeychainRead)?;
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

fn wait_child(child: &mut Child) -> Result<ExitStatus, ProvisionerErrorV1> {
    let deadline = Instant::now()
        .checked_add(COMMAND_TIMEOUT)
        .ok_or(ProvisionerErrorV1::KeychainTimeout)?;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(COMMAND_POLL),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ProvisionerErrorV1::KeychainTimeout);
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ProvisionerErrorV1::KeychainRead);
            }
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    #[ignore]
    fn temporary_keychain_item_round_trips_over_stdin_and_is_deleted() {
        let service = Box::leak(
            format!("starring-staging-provisioner.e2e.{}", std::process::id()).into_boxed_str(),
        );
        let identity = KeychainIdentityV1 {
            service,
            account: "temporary.test-only",
        };
        let client = KeychainClientV1::new().unwrap();
        let _ = client.delete(identity);
        let secret = b"postgresql://starring_identity_oauth:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA@127.0.0.1:5432/starring_runtime_staging?sslmode=disable";
        assert!(secret.len() > 128);
        client.write_and_verify(identity, secret).unwrap();
        let readback = client.read_required(identity).unwrap();
        assert_eq!(readback.as_slice(), secret);
        client.delete(identity).unwrap();
        assert!(client.read_optional(identity).unwrap().is_none());
        let update = client
            .begin_create(SecretItemRefV1 {
                identity,
                value: secret,
            })
            .unwrap();
        assert_eq!(
            client
                .begin_create(SecretItemRefV1 {
                    identity,
                    value: b"different",
                })
                .err(),
            Some(ProvisionerErrorV1::IncrementalAuthoringWriterPartialState)
        );
        assert_eq!(client.read_required(identity).unwrap().as_slice(), secret);
        update.rollback().unwrap();
        assert!(client.read_optional(identity).unwrap().is_none());
        let update = client
            .begin_create(SecretItemRefV1 {
                identity,
                value: secret,
            })
            .unwrap();
        client.write_and_verify(identity, b"different").unwrap();
        assert_eq!(update.rollback(), Err(ProvisionerErrorV1::KeychainRollback));
        assert_eq!(
            client.read_required(identity).unwrap().as_slice(),
            b"different"
        );
        client.delete(identity).unwrap();
    }
}
