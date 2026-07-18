use std::fmt::{Debug, Formatter};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use zeroize::{Zeroize, Zeroizing};

const SECRET_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductSecretGeneratorError {
    #[error("product secret generation is unavailable")]
    Unavailable,
}

pub trait ProductSecretGenerator: Send + Sync {
    fn fill_secret(
        &self,
        destination: &mut [u8; SECRET_BYTES],
    ) -> Result<(), ProductSecretGeneratorError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct OperatingSystemSecretGenerator;

impl ProductSecretGenerator for OperatingSystemSecretGenerator {
    fn fill_secret(
        &self,
        destination: &mut [u8; SECRET_BYTES],
    ) -> Result<(), ProductSecretGeneratorError> {
        getrandom::fill(destination).map_err(|_| ProductSecretGeneratorError::Unavailable)
    }
}

pub struct ProductSecretV1 {
    encoded: Zeroizing<String>,
}

impl ProductSecretV1 {
    pub(crate) fn generate<G: ProductSecretGenerator>(
        generator: &G,
    ) -> Result<Self, ProductSecretGeneratorError> {
        let mut bytes = Zeroizing::new([0_u8; SECRET_BYTES]);
        generator.fill_secret(&mut bytes)?;
        let encoded = Zeroizing::new(URL_SAFE_NO_PAD.encode(bytes.as_slice()));
        bytes.zeroize();
        Ok(Self { encoded })
    }

    pub fn expose_secret(&self) -> &str {
        self.encoded.as_str()
    }
}

impl Debug for ProductSecretV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProductSecretV1(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU8, Ordering};

    use super::*;

    struct CountingGenerator(AtomicU8);

    impl ProductSecretGenerator for CountingGenerator {
        fn fill_secret(
            &self,
            destination: &mut [u8; SECRET_BYTES],
        ) -> Result<(), ProductSecretGeneratorError> {
            destination.fill(self.0.fetch_add(1, Ordering::SeqCst));
            Ok(())
        }
    }

    #[test]
    fn generated_secret_is_exactly_32_bytes_and_redacted() {
        let secret = ProductSecretV1::generate(&CountingGenerator(AtomicU8::new(1))).unwrap();
        assert_eq!(secret.expose_secret().len(), 43);
        assert_eq!(
            URL_SAFE_NO_PAD.decode(secret.expose_secret()).unwrap(),
            vec![1_u8; 32]
        );
        assert_eq!(format!("{secret:?}"), "ProductSecretV1(<redacted>)");
    }
}
