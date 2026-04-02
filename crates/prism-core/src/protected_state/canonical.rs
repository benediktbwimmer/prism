use anyhow::{Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub(crate) fn canonical_json_bytes<T>(value: &T) -> Result<Vec<u8>>
where
    T: Serialize + ?Sized,
{
    serde_jcs::to_vec(value).context("failed to serialize protected-state JSON canonically")
}

pub(crate) fn sha256_prefixed(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{canonical_json_bytes, sha256_prefixed};

    #[test]
    fn canonical_json_orders_keys_without_whitespace() {
        let value = json!({
            "z": 1,
            "a": {
                "b": true,
                "a": false
            }
        });
        let bytes = canonical_json_bytes(&value).expect("canonical serialization should succeed");
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            r#"{"a":{"a":false,"b":true},"z":1}"#
        );
    }

    #[test]
    fn sha256_hash_uses_prefixed_hex_format() {
        assert_eq!(
            sha256_prefixed(br#"{"a":1}"#),
            "sha256:015abd7f5cc57a2dd94b7590f04ad8084273905ee33ec5cebeae62276a97f862"
        );
    }
}
