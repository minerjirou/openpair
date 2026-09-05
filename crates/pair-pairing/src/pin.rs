//! PIN <-> Noob encoding for the cluster pairing flow (§7.6).
//!
//! The six-digit human PIN is the low-entropy stand-in for a real out-of-band
//! nonce: it is encoded as a 16-byte big-endian `Noob`, left-padded with zeros,
//! exactly as the upstream `noobFromPIN` does (`big.Int.SetString` then
//! `FillBytes` into a 16-byte buffer). This is the value fed to
//! [`crate::Server::oob_output_with`] / [`crate::Peer::oob_input_noob`].

/// A well-formed pairing PIN is exactly six decimal digits (upstream
/// `^[0-9]{6}$`).
pub fn is_valid_pin(pin: &str) -> bool {
    pin.len() == 6 && pin.bytes().all(|b| b.is_ascii_digit())
}

/// Encode a six-digit PIN as a 16-byte big-endian `Noob`.
///
/// Mirrors upstream `noobFromPIN`: parse the decimal string into an integer and
/// serialize it big-endian into a fixed 16-byte buffer (left-padded with zeros).
/// A non-numeric PIN parses as zero, matching `big.Int.SetString` returning a
/// zero value — but callers should gate on [`is_valid_pin`] first.
pub fn noob_from_pin(pin: &str) -> [u8; 16] {
    // A six-digit PIN (max 999_999) fits comfortably in u128; u128::to_be_bytes
    // yields exactly 16 bytes, big-endian, left-padded with zeros -- identical to
    // Go's math/big FillBytes over a 16-byte slice.
    let v: u128 = pin.parse().unwrap_or(0);
    v.to_be_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_pin_shape() {
        assert!(is_valid_pin("000000"));
        assert!(is_valid_pin("123456"));
        assert!(!is_valid_pin("12345")); // too short
        assert!(!is_valid_pin("1234567")); // too long
        assert!(!is_valid_pin("12a456")); // non-digit
        assert!(!is_valid_pin(""));
    }

    #[test]
    fn noob_is_16_be_bytes_left_padded() {
        // 123456 = 0x01E240; occupies the last three bytes, rest zero.
        let n = noob_from_pin("123456");
        assert_eq!(n.len(), 16);
        assert_eq!(&n[..13], &[0u8; 13]);
        assert_eq!(&n[13..], &[0x01, 0xE2, 0x40]);
    }

    #[test]
    fn leading_zeros_preserved_as_value() {
        // "000123" is the integer 123, not a distinct byte pattern.
        assert_eq!(noob_from_pin("000123"), noob_from_pin("123"));
        let n = noob_from_pin("000001");
        assert_eq!(n[15], 1);
        assert_eq!(&n[..15], &[0u8; 15]);
    }

    #[test]
    fn max_pin() {
        // 999999 = 0x0F423F.
        let n = noob_from_pin("999999");
        assert_eq!(&n[13..], &[0x0F, 0x42, 0x3F]);
    }
}
