//! The `urn:nvpair:node:<uuid>` SAN URI scheme carried in node certificates.
//! Byte-confirmed prefix from the reference identity code.

/// SAN URI prefix that carries a node UUID.
pub const NODE_URN_PREFIX: &str = "urn:nvpair:node:";

/// Build the SAN URI for a node UUID.
pub fn node_urn(uuid: &str) -> String {
    format!("{NODE_URN_PREFIX}{uuid}")
}

/// Extract the UUID from a `urn:nvpair:node:<uuid>` URI, if it matches.
pub fn uuid_from_urn(uri: &str) -> Option<String> {
    uri.strip_prefix(NODE_URN_PREFIX).map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn urn_roundtrip() {
        let u = "abcd";
        assert_eq!(node_urn(u), "urn:nvpair:node:abcd");
        assert_eq!(
            uuid_from_urn("urn:nvpair:node:abcd").as_deref(),
            Some("abcd")
        );
        assert_eq!(uuid_from_urn("urn:other:xyz"), None);
    }
}
