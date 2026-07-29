use sha2::{Digest, Sha256};

pub(crate) struct LengthFramedSha256 {
    hasher: Sha256,
}

impl LengthFramedSha256 {
    pub(crate) fn new(domain: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        update_length_framed(&mut hasher, domain);
        Self { hasher }
    }

    pub(crate) fn update(&mut self, field: &[u8]) {
        update_length_framed(&mut self.hasher, field);
    }

    pub(crate) fn finalize(self) -> String {
        lower_hex(self.hasher.finalize())
    }
}

fn update_length_framed(hasher: &mut Sha256, bytes: &[u8]) {
    let length = u64::try_from(bytes.len()).expect("digest field exceeds u64::MAX");
    hasher.update(length.to_be_bytes());
    hasher.update(bytes);
}

fn lower_hex(bytes: impl AsRef<[u8]>) -> String {
    let bytes = bytes.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}
