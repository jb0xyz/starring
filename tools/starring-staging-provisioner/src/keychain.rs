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

#[derive(Clone, Copy)]
pub(crate) struct DynamicSecretItemRefV1<'a> {
    pub service: &'a str,
    pub account: &'a str,
    pub value: &'a [u8],
}

#[derive(Clone, Copy)]
struct KeychainTargetV1<'a> {
    service: &'a str,
    account: &'a str,
}

impl KeychainTargetV1<'_> {
    fn validate(self) -> Result<(), ProvisionerErrorV1> {
        if valid_keychain_component(self.service) && valid_keychain_component(self.account) {
            Ok(())
        } else {
            Err(ProvisionerErrorV1::IdentityManifest)
        }
    }
}

impl From<KeychainIdentityV1> for KeychainTargetV1<'_> {
    fn from(identity: KeychainIdentityV1) -> Self {
        Self {
            service: identity.service,
            account: identity.account,
        }
    }
}

#[derive(Clone)]
struct OwnedKeychainTargetV1 {
    service: String,
    account: String,
}

impl OwnedKeychainTargetV1 {
    fn as_target(&self) -> KeychainTargetV1<'_> {
        KeychainTargetV1 {
            service: &self.service,
            account: &self.account,
        }
    }
}

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
        self.read_required_target(identity.into())
    }

    pub(crate) fn read_required_dynamic(
        &self,
        service: &str,
        account: &str,
    ) -> Result<Zeroizing<Vec<u8>>, ProvisionerErrorV1> {
        self.read_required_target(KeychainTargetV1 { service, account })
    }

    fn read_required_target(
        &self,
        target: KeychainTargetV1<'_>,
    ) -> Result<Zeroizing<Vec<u8>>, ProvisionerErrorV1> {
        match self.read_optional_target(target)? {
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
                target: owned_target(item.identity.into()),
                previous: self.read_optional(item.identity)?,
                created: None,
            });
        }
        for (written, item) in items.iter().enumerate() {
            if self
                .write_and_verify_target(item.identity.into(), item.value)
                .is_err()
            {
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
                target: owned_target(item.identity.into()),
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
        self.read_optional_target(identity.into())
    }

    pub(crate) fn read_optional_dynamic(
        &self,
        service: &str,
        account: &str,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, ProvisionerErrorV1> {
        self.read_optional_target(KeychainTargetV1 { service, account })
    }

    fn read_optional_target(
        &self,
        target: KeychainTargetV1<'_>,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, ProvisionerErrorV1> {
        target.validate()?;
        let mut command = Command::new(SECURITY_PATH);
        command
            .args([
                "find-generic-password",
                "-w",
                "-s",
                target.service,
                "-a",
                target.account,
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

    fn write_and_verify_target(
        &self,
        target: KeychainTargetV1<'_>,
        value: &[u8],
    ) -> Result<(), ProvisionerErrorV1> {
        self.write_with_mode_and_verify(target, value, true)
    }

    fn write_new_and_verify(
        &self,
        identity: KeychainIdentityV1,
        value: &[u8],
    ) -> Result<(), ProvisionerErrorV1> {
        self.write_new_and_verify_target(identity.into(), value)
    }

    fn write_new_and_verify_target(
        &self,
        target: KeychainTargetV1<'_>,
        value: &[u8],
    ) -> Result<(), ProvisionerErrorV1> {
        self.write_with_mode_and_verify(target, value, false)
    }

    fn write_with_mode_and_verify(
        &self,
        target: KeychainTargetV1<'_>,
        value: &[u8],
        update: bool,
    ) -> Result<(), ProvisionerErrorV1> {
        target.validate()?;
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
        let input = build_write_input(target, value, update);
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
        let readback = match self.read_required_target(target) {
            Ok(readback) => readback,
            Err(_) if !update => return Err(ProvisionerErrorV1::KeychainWrite),
            Err(error) => return Err(error),
        };
        if readback.len() != value.len() || !bool::from(readback.as_slice().ct_eq(value)) {
            return Err(ProvisionerErrorV1::KeychainWrite);
        }
        Ok(())
    }

    pub(crate) fn delete_dynamic(
        &self,
        service: &str,
        account: &str,
    ) -> Result<(), ProvisionerErrorV1> {
        self.delete_target(KeychainTargetV1 { service, account })
    }

    fn delete_target(&self, target: KeychainTargetV1<'_>) -> Result<(), ProvisionerErrorV1> {
        target.validate()?;
        let mut command = Command::new(SECURITY_PATH);
        command
            .args([
                "delete-generic-password",
                "-s",
                target.service,
                "-a",
                target.account,
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
                Some(value) => self.write_and_verify_target(backup.target.as_target(), value),
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
            return self.delete_target(backup.target.as_target());
        };
        match self.read_optional_target(backup.target.as_target())? {
            None => Ok(()),
            Some(current)
                if current.len() == expected.len()
                    && bool::from(current.as_slice().ct_eq(expected.as_slice())) =>
            {
                self.delete_target(backup.target.as_target())
            }
            Some(_) => Err(ProvisionerErrorV1::KeychainRollback),
        }
    }

    pub(crate) fn begin_create_dynamic(
        &self,
        items: &[DynamicSecretItemRefV1<'_>],
    ) -> Result<KeychainUpdateV1, ProvisionerErrorV1> {
        let mut targets = items
            .iter()
            .map(|item| (item.service, item.account))
            .collect::<Vec<_>>();
        targets.sort_unstable();
        if targets.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(ProvisionerErrorV1::IdentityManifest);
        }
        let mut backups = Vec::with_capacity(items.len());
        for item in items {
            let target = KeychainTargetV1 {
                service: item.service,
                account: item.account,
            };
            target.validate()?;
            if self.read_optional_target(target)?.is_some() {
                return Err(ProvisionerErrorV1::KeychainWrite);
            }
            backups.push(KeychainBackupEntryV1 {
                target: owned_target(target),
                previous: None,
                created: Some(Zeroizing::new(item.value.to_vec())),
            });
        }
        for (written, item) in items.iter().enumerate() {
            let target = KeychainTargetV1 {
                service: item.service,
                account: item.account,
            };
            if self
                .write_new_and_verify_target(target, item.value)
                .is_err()
            {
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
}

struct KeychainBackupEntryV1 {
    target: OwnedKeychainTargetV1,
    previous: Option<Zeroizing<Vec<u8>>>,
    created: Option<Zeroizing<Vec<u8>>>,
}

fn owned_target(target: KeychainTargetV1<'_>) -> OwnedKeychainTargetV1 {
    OwnedKeychainTargetV1 {
        service: target.service.to_owned(),
        account: target.account.to_owned(),
    }
}

fn valid_keychain_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 192
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && matches!(byte, b'.' | b'_' | b'-'))
        })
}

fn build_write_input(
    target: KeychainTargetV1<'_>,
    value: &[u8],
    update: bool,
) -> Zeroizing<Vec<u8>> {
    let mut input = Zeroizing::new(Vec::with_capacity(
        48 + target.service.len() + target.account.len() + value.len() * 2,
    ));
    input.extend_from_slice(b"add-generic-password ");
    if update {
        input.extend_from_slice(b"-U ");
    }
    input.extend_from_slice(b"-s ");
    input.extend_from_slice(target.service.as_bytes());
    input.extend_from_slice(b" -a ");
    input.extend_from_slice(target.account.as_bytes());
    input.extend_from_slice(b" -X ");
    for byte in value {
        input.push(b"0123456789abcdef"[(byte >> 4) as usize]);
        input.push(b"0123456789abcdef"[(byte & 0x0f) as usize]);
    }
    input.push(b'\n');
    input
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
        let _ = client.delete_target(identity.into());
        let secret = b"postgresql://starring_identity_oauth:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA@127.0.0.1:5432/starring_runtime_staging?sslmode=disable";
        assert!(secret.len() > 128);
        client
            .write_and_verify_target(identity.into(), secret)
            .unwrap();
        let readback = client.read_required(identity).unwrap();
        assert_eq!(readback.as_slice(), secret);
        client.delete_target(identity.into()).unwrap();
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
        client
            .write_and_verify_target(identity.into(), b"different")
            .unwrap();
        assert_eq!(update.rollback(), Err(ProvisionerErrorV1::KeychainRollback));
        assert_eq!(
            client.read_required(identity).unwrap().as_slice(),
            b"different"
        );
        client.delete_target(identity.into()).unwrap();
    }

    #[test]
    fn dynamic_write_uses_only_interactive_stdin_hex() {
        let target = KeychainTargetV1 {
            service: "starring.d2.0123456789ab.api",
            account: "database.session-api",
        };
        let secret = b"not-on-argv";
        let input = build_write_input(target, secret, false);
        let rendered = std::str::from_utf8(&input).unwrap();
        assert_eq!(
            rendered,
            "add-generic-password -s starring.d2.0123456789ab.api -a database.session-api -X 6e6f742d6f6e2d61726776\n"
        );
        assert!(!rendered.contains("not-on-argv"));
        assert!(!rendered.contains("-U"));
    }
}
