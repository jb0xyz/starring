pub(crate) const RUNTIME_IDENTITY_ENTROPY_BYTES: usize = 16;
const LOWER_HEX: &[u8; 16] = b"0123456789abcdef";

pub(crate) fn encode_runtime_identity_lower_hex_v1(
    bytes: [u8; RUNTIME_IDENTITY_ENTROPY_BYTES],
) -> String {
    let mut encoded = String::with_capacity(RUNTIME_IDENTITY_ENTROPY_BYTES * 2);
    for byte in bytes {
        encoded.push(LOWER_HEX[usize::from(byte >> 4)] as char);
        encoded.push(LOWER_HEX[usize::from(byte & 0x0f)] as char);
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_entropy_vectors_encode_exact_lower_hex() {
        for (bytes, expected) in [
            ([0_u8; 16], "00000000000000000000000000000000"),
            ([u8::MAX; 16], "ffffffffffffffffffffffffffffffff"),
            (
                [
                    0x00, 0x01, 0x0a, 0x0f, 0x10, 0x7f, 0x80, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba,
                    0x98, 0x76, 0x54,
                ],
                "00010a0f107f80abcdeffedcba987654",
            ),
        ] {
            let encoded = encode_runtime_identity_lower_hex_v1(bytes);

            assert_eq!(encoded, expected);
            assert_eq!(encoded.len(), 32);
            assert!(encoded
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
        }
    }
}
