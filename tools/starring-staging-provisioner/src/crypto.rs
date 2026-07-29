use std::fmt::{Debug, Formatter};

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::identity::{
    database_url, validate_identity_manifest, DatabaseIdentityV1, KeychainIdentityV1,
    ADMIN_DATABASE_NAME, ADMIN_KEYCHAIN_IDENTITY, APPLICATION_DATABASE_IDENTITIES,
    CLUSTER_ADMIN_ROLE, DATABASE_NAME, PRODUCT_ACTION_KEYRING_IDENTITY,
    SNAPSHOT_ENVELOPE_KEYRING_IDENTITY,
};
use crate::ProvisionerErrorV1;

const PASSWORD_BYTES: usize = 32;
const SCRAM_SALT_BYTES: usize = 16;
const SCRAM_ITERATIONS: u32 = 4096;
const KEYRING_MATERIAL_BYTES: usize = 32;
const KEY_ID_RANDOM_BYTES: usize = 12;

type HmacSha256 = Hmac<Sha256>;

pub trait RandomSourceV1 {
    fn fill(&mut self, output: &mut [u8]) -> Result<(), ProvisionerErrorV1>;
}

pub struct SystemRandomSourceV1;

impl RandomSourceV1 for SystemRandomSourceV1 {
    fn fill(&mut self, output: &mut [u8]) -> Result<(), ProvisionerErrorV1> {
        getrandom::fill(output).map_err(|_| ProvisionerErrorV1::Random)
    }
}

pub struct DatabaseSecretV1 {
    identity: DatabaseIdentityV1,
    password: Zeroizing<String>,
    verifier: Zeroizing<String>,
    url: Zeroizing<String>,
}

impl DatabaseSecretV1 {
    pub fn identity(&self) -> DatabaseIdentityV1 {
        self.identity
    }

    pub fn password(&self) -> &str {
        self.password.as_str()
    }

    pub fn verifier(&self) -> &str {
        self.verifier.as_str()
    }

    pub fn url(&self) -> &[u8] {
        self.url.as_bytes()
    }
}

impl Debug for DatabaseSecretV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("DatabaseSecretV1(<redacted>)")
    }
}

pub struct AdminSecretV1 {
    verifier: Zeroizing<String>,
    url: Zeroizing<String>,
}

impl AdminSecretV1 {
    pub fn verifier(&self) -> &str {
        self.verifier.as_str()
    }

    pub fn url(&self) -> &[u8] {
        self.url.as_bytes()
    }
}

impl Debug for AdminSecretV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AdminSecretV1(<redacted>)")
    }
}

pub struct KeyringSecretV1 {
    identity: KeychainIdentityV1,
    active_key_id: String,
    payload: Zeroizing<String>,
}

impl KeyringSecretV1 {
    pub fn identity(&self) -> KeychainIdentityV1 {
        self.identity
    }

    pub fn active_key_id(&self) -> &str {
        &self.active_key_id
    }

    pub fn payload(&self) -> &[u8] {
        self.payload.as_bytes()
    }
}

impl Debug for KeyringSecretV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("KeyringSecretV1(<redacted>)")
    }
}

pub struct GeneratedSecretsV1 {
    database: Vec<DatabaseSecretV1>,
    admin: AdminSecretV1,
    product_action_keyring: KeyringSecretV1,
    snapshot_envelope_keyring: KeyringSecretV1,
}

impl GeneratedSecretsV1 {
    pub fn generate() -> Result<Self, ProvisionerErrorV1> {
        let mut source = SystemRandomSourceV1;
        Self::generate_with(&mut source)
    }

    pub fn generate_with(source: &mut impl RandomSourceV1) -> Result<Self, ProvisionerErrorV1> {
        validate_identity_manifest()?;
        let mut database: Vec<DatabaseSecretV1> =
            Vec::with_capacity(APPLICATION_DATABASE_IDENTITIES.len());
        for identity in APPLICATION_DATABASE_IDENTITIES {
            let password = unique_password(source, database.iter().map(|item| item.password()))?;
            let verifier = random_scram_verifier(source, password.as_bytes())?;
            let url = database_url(identity.role, password.as_str(), DATABASE_NAME);
            database.push(DatabaseSecretV1 {
                identity,
                password,
                verifier,
                url,
            });
        }
        let admin_password = unique_password(source, database.iter().map(|item| item.password()))?;
        let admin_verifier = random_scram_verifier(source, admin_password.as_bytes())?;
        let admin_url = database_url(
            CLUSTER_ADMIN_ROLE,
            admin_password.as_str(),
            ADMIN_DATABASE_NAME,
        );
        let admin = AdminSecretV1 {
            verifier: admin_verifier,
            url: admin_url,
        };
        let (product_action_keyring, product_material) = generate_keyring(
            source,
            PRODUCT_ACTION_KEYRING_IDENTITY,
            "product-action-v1",
            None,
        )?;
        let (snapshot_envelope_keyring, _) = generate_keyring(
            source,
            SNAPSHOT_ENVELOPE_KEYRING_IDENTITY,
            "snapshot-envelope-v1",
            Some(&product_material),
        )?;
        Ok(Self {
            database,
            admin,
            product_action_keyring,
            snapshot_envelope_keyring,
        })
    }

    pub fn database(&self) -> &[DatabaseSecretV1] {
        &self.database
    }

    pub fn admin(&self) -> &AdminSecretV1 {
        &self.admin
    }

    pub fn keychain_items(&self) -> Vec<SecretItemRefV1<'_>> {
        let mut items = self
            .database
            .iter()
            .map(|secret| SecretItemRefV1 {
                identity: KeychainIdentityV1 {
                    service: secret.identity.service,
                    account: secret.identity.account,
                },
                value: secret.url(),
            })
            .collect::<Vec<_>>();
        items.push(SecretItemRefV1 {
            identity: ADMIN_KEYCHAIN_IDENTITY,
            value: self.admin.url(),
        });
        items.push(SecretItemRefV1 {
            identity: self.product_action_keyring.identity(),
            value: self.product_action_keyring.payload(),
        });
        items.push(SecretItemRefV1 {
            identity: self.snapshot_envelope_keyring.identity(),
            value: self.snapshot_envelope_keyring.payload(),
        });
        items
    }

    pub fn product_action_key_id(&self) -> &str {
        self.product_action_keyring.active_key_id()
    }

    pub fn snapshot_envelope_key_id(&self) -> &str {
        self.snapshot_envelope_keyring.active_key_id()
    }

    pub fn product_action_keyring_payload(&self) -> &[u8] {
        self.product_action_keyring.payload()
    }

    pub fn snapshot_envelope_keyring_payload(&self) -> &[u8] {
        self.snapshot_envelope_keyring.payload()
    }
}

impl Debug for GeneratedSecretsV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("GeneratedSecretsV1(<redacted>)")
    }
}

#[derive(Clone, Copy)]
pub struct SecretItemRefV1<'a> {
    pub identity: KeychainIdentityV1,
    pub value: &'a [u8],
}

fn unique_password<'a>(
    source: &mut impl RandomSourceV1,
    existing: impl IntoIterator<Item = &'a str>,
) -> Result<Zeroizing<String>, ProvisionerErrorV1> {
    let existing = existing.into_iter().collect::<Vec<_>>();
    for _ in 0..8 {
        let mut random = Zeroizing::new([0_u8; PASSWORD_BYTES]);
        source.fill(&mut *random)?;
        let password = Zeroizing::new(URL_SAFE_NO_PAD.encode(random.as_slice()));
        if password.len() == 43
            && !existing
                .iter()
                .any(|candidate| *candidate == password.as_str())
        {
            return Ok(password);
        }
    }
    Err(ProvisionerErrorV1::Random)
}

fn generate_keyring(
    source: &mut impl RandomSourceV1,
    identity: KeychainIdentityV1,
    key_id_prefix: &str,
    forbidden_material: Option<&[u8; KEYRING_MATERIAL_BYTES]>,
) -> Result<(KeyringSecretV1, Zeroizing<[u8; KEYRING_MATERIAL_BYTES]>), ProvisionerErrorV1> {
    let mut key_id_random = Zeroizing::new([0_u8; KEY_ID_RANDOM_BYTES]);
    source.fill(&mut *key_id_random)?;
    let active_key_id = format!(
        "{key_id_prefix}-{}",
        URL_SAFE_NO_PAD.encode(key_id_random.as_slice())
    );
    let mut material = None;
    for _ in 0..8 {
        let mut candidate = Zeroizing::new([0_u8; KEYRING_MATERIAL_BYTES]);
        source.fill(&mut *candidate)?;
        let repetitive = candidate
            .iter()
            .all(|value| *value == candidate.first().copied().unwrap_or_default());
        let forbidden = forbidden_material.is_some_and(|forbidden| forbidden == &*candidate);
        if !repetitive && !forbidden {
            material = Some(candidate);
            break;
        }
    }
    let material = material.ok_or(ProvisionerErrorV1::Random)?;
    let encoded = Zeroizing::new(STANDARD.encode(material.as_slice()));
    let payload = Zeroizing::new(format!(
        "{{\"version\":1,\"active\":{{\"id\":\"{active_key_id}\",\"material\":\"{}\"}},\"retired\":[]}}",
        encoded.as_str()
    ));
    Ok((
        KeyringSecretV1 {
            identity,
            active_key_id,
            payload,
        },
        material,
    ))
}

fn random_scram_verifier(
    source: &mut impl RandomSourceV1,
    password: &[u8],
) -> Result<Zeroizing<String>, ProvisionerErrorV1> {
    let mut salt = Zeroizing::new([0_u8; SCRAM_SALT_BYTES]);
    source.fill(&mut *salt)?;
    scram_verifier(password, salt.as_slice())
}

pub fn scram_verifier(
    password: &[u8],
    salt: &[u8],
) -> Result<Zeroizing<String>, ProvisionerErrorV1> {
    if password.is_empty() || salt.is_empty() {
        return Err(ProvisionerErrorV1::Scram);
    }
    let salted_password = pbkdf2_sha256(password, salt, SCRAM_ITERATIONS)?;
    let client_key = Zeroizing::new(hmac_sha256(salted_password.as_slice(), b"Client Key")?);
    let stored_digest = Sha256::digest(client_key.as_slice());
    let mut stored_key = Zeroizing::new([0_u8; 32]);
    stored_key.copy_from_slice(&stored_digest);
    let server_key = Zeroizing::new(hmac_sha256(salted_password.as_slice(), b"Server Key")?);
    Ok(Zeroizing::new(format!(
        "SCRAM-SHA-256${SCRAM_ITERATIONS}:{}${}:{}",
        STANDARD.encode(salt),
        STANDARD.encode(stored_key.as_slice()),
        STANDARD.encode(server_key.as_slice())
    )))
}

fn pbkdf2_sha256(
    password: &[u8],
    salt: &[u8],
    iterations: u32,
) -> Result<Zeroizing<[u8; 32]>, ProvisionerErrorV1> {
    if iterations == 0 {
        return Err(ProvisionerErrorV1::Scram);
    }
    let mut first_input = Zeroizing::new(Vec::with_capacity(salt.len() + 4));
    first_input.extend_from_slice(salt);
    first_input.extend_from_slice(&1_u32.to_be_bytes());
    let mut current = Zeroizing::new(hmac_sha256(password, first_input.as_slice())?);
    let mut output = Zeroizing::new([0_u8; 32]);
    output.copy_from_slice(current.as_slice());
    for _ in 1..iterations {
        current = Zeroizing::new(hmac_sha256(password, current.as_slice())?);
        for (output, current) in output.iter_mut().zip(current.iter()) {
            *output ^= current;
        }
    }
    Ok(output)
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> Result<[u8; 32], ProvisionerErrorV1> {
    let mut mac = HmacSha256::new_from_slice(key).map_err(|_| ProvisionerErrorV1::Scram)?;
    mac.update(message);
    Ok(mac.finalize().into_bytes().into())
}

pub fn valid_scram_verifier(value: &str) -> bool {
    let Some((salt, keys)) = value
        .strip_prefix("SCRAM-SHA-256$4096:")
        .and_then(|value| value.split_once('$'))
    else {
        return false;
    };
    let Some((stored, server)) = keys.split_once(':') else {
        return false;
    };
    let valid_base64 = |candidate: &str, decoded_length: usize| {
        let mut decoded = Zeroizing::new([0_u8; 33]);
        STANDARD
            .decode_slice(candidate, &mut *decoded)
            .is_ok_and(|length| length == decoded_length)
    };
    valid_base64(salt, SCRAM_SALT_BYTES) && valid_base64(stored, 32) && valid_base64(server, 32)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DeterministicRandomV1 {
        state: u64,
    }

    impl RandomSourceV1 for DeterministicRandomV1 {
        fn fill(&mut self, output: &mut [u8]) -> Result<(), ProvisionerErrorV1> {
            for value in output {
                self.state ^= self.state << 13;
                self.state ^= self.state >> 7;
                self.state ^= self.state << 17;
                *value = self.state as u8;
            }
            Ok(())
        }
    }

    #[test]
    fn scram_matches_independent_rfc_7677_vector() {
        let salt = STANDARD.decode("W22ZaJ0SNY7soEsUEjb6gQ==").unwrap();
        let verifier = scram_verifier(b"pencil", &salt).unwrap();
        assert_eq!(
            verifier.as_str(),
            "SCRAM-SHA-256$4096:W22ZaJ0SNY7soEsUEjb6gQ==$WG5d8oPm3OtcPnkdi4Uo7BkeZkBFzpcXkuLmtbsT4qY=:wfPLwcE6nTWhTAmQ7tl2KeoiWGPlZqQxSrmfPwDl2dU="
        );
        assert!(valid_scram_verifier(&verifier));
    }

    #[test]
    fn generated_passwords_verifiers_and_keyrings_are_distinct_and_shaped() {
        let mut random = DeterministicRandomV1 { state: 1 };
        let secrets = GeneratedSecretsV1::generate_with(&mut random).unwrap();
        assert_eq!(secrets.database().len(), 20);
        assert_eq!(secrets.keychain_items().len(), 23);
        let mut passwords = secrets
            .database()
            .iter()
            .map(|secret| secret.password())
            .collect::<Vec<_>>();
        let admin_url = std::str::from_utf8(secrets.admin().url()).unwrap();
        let admin_password = admin_url
            .strip_prefix("postgresql://starring_cluster_admin:")
            .unwrap()
            .strip_suffix("@127.0.0.1:5432/postgres?sslmode=disable")
            .unwrap();
        passwords.push(admin_password);
        assert_eq!(
            passwords
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            21
        );
        assert!(passwords.iter().all(|password| password.len() == 43));
        assert!(secrets
            .database()
            .iter()
            .all(|secret| valid_scram_verifier(secret.verifier())));
        assert!(valid_scram_verifier(secrets.admin().verifier()));
        assert_ne!(
            secrets.product_action_key_id(),
            secrets.snapshot_envelope_key_id()
        );
        let keyrings = secrets
            .keychain_items()
            .into_iter()
            .filter(|item| item.identity.account.starts_with("keyring."))
            .map(|item| serde_json::from_slice::<serde_json::Value>(item.value).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(keyrings.len(), 2);
        for keyring in &keyrings {
            assert_eq!(keyring["version"], 1);
            assert!(keyring["retired"].as_array().unwrap().is_empty());
            let material = keyring["active"]["material"].as_str().unwrap();
            assert_eq!(material.len(), 44);
            assert_eq!(STANDARD.decode(material).unwrap().len(), 32);
        }
        assert_ne!(
            keyrings[0]["active"]["material"],
            keyrings[1]["active"]["material"]
        );
    }

    #[test]
    fn debug_and_error_surfaces_do_not_expose_generated_secret_values() {
        let mut random = DeterministicRandomV1 { state: 41 };
        let secrets = GeneratedSecretsV1::generate_with(&mut random).unwrap();
        let database_secret = secrets.database()[0].password();
        let verifier = secrets.database()[0].verifier();
        let keyring_payload = std::str::from_utf8(
            secrets
                .keychain_items()
                .into_iter()
                .find(|item| item.identity.account == "keyring.product-action")
                .unwrap()
                .value,
        )
        .unwrap();
        let rendered = format!(
            "{secrets:?} {:?} {:?} {}",
            secrets.database()[0],
            secrets.admin(),
            ProvisionerErrorV1::DatabaseMutation.code()
        );
        assert!(!rendered.contains(database_secret));
        assert!(!rendered.contains(verifier));
        assert!(!rendered.contains(keyring_payload));
        assert_eq!(rendered.matches("<redacted>").count(), 3);
    }
}
