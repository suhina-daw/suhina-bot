// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Discord signs every interaction it sends with the application's Ed25519 key: the signature
//! covers the `X-Signature-Timestamp` header followed by the raw body. A request that fails the
//! check is answered with 401, which is also what Discord tests when the endpoint is saved.

use ed25519_dalek::{Signature, VerifyingKey};

use crate::Error;

/// The application's public key, as the Developer Portal shows it (64 hex characters).
#[derive(Debug, Clone)]
pub struct DiscordKey(VerifyingKey);

impl DiscordKey {
    pub fn from_hex(hex_key: &str) -> Result<Self, Error> {
        let bytes: [u8; 32] = hex::decode(hex_key.trim())
            .map_err(|_| Error::BadKey("Discord public key is not hex"))?
            .try_into()
            .map_err(|_| Error::BadKey("Discord public key is not 32 bytes"))?;
        VerifyingKey::from_bytes(&bytes)
            .map(DiscordKey)
            .map_err(|_| Error::BadKey("Discord public key is not a valid Ed25519 key"))
    }

    /// Checks `signature_hex` (the `X-Signature-Ed25519` header) over `timestamp` and `body`.
    pub fn verify(&self, signature_hex: &str, timestamp: &str, body: &[u8]) -> Result<(), Error> {
        let bytes: [u8; 64] = hex::decode(signature_hex.trim())
            .map_err(|_| Error::BadSignature)?
            .try_into()
            .map_err(|_| Error::BadSignature)?;
        let signature = Signature::from_bytes(&bytes);
        let mut message = Vec::with_capacity(timestamp.len() + body.len());
        message.extend_from_slice(timestamp.as_bytes());
        message.extend_from_slice(body);
        self.0
            .verify_strict(&message, &signature)
            .map_err(|_| Error::BadSignature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn keys() -> (SigningKey, DiscordKey) {
        let signing = SigningKey::from_bytes(&[7; 32]);
        let public = hex::encode(signing.verifying_key().to_bytes());
        (signing, DiscordKey::from_hex(&public).unwrap())
    }

    fn sign(signing: &SigningKey, timestamp: &str, body: &[u8]) -> String {
        let mut message = timestamp.as_bytes().to_vec();
        message.extend_from_slice(body);
        hex::encode(signing.sign(&message).to_bytes())
    }

    #[test]
    fn accepts_a_correct_signature() {
        let (signing, key) = keys();
        let signature = sign(&signing, "1700000000", br#"{"type":1}"#);
        assert_eq!(
            key.verify(&signature, "1700000000", br#"{"type":1}"#),
            Ok(())
        );
    }

    #[test]
    fn refuses_a_changed_body_or_timestamp() {
        let (signing, key) = keys();
        let signature = sign(&signing, "1700000000", br#"{"type":1}"#);
        assert_eq!(
            key.verify(&signature, "1700000000", br#"{"type":2}"#),
            Err(Error::BadSignature)
        );
        assert_eq!(
            key.verify(&signature, "1700000001", br#"{"type":1}"#),
            Err(Error::BadSignature)
        );
    }

    #[test]
    fn refuses_malformed_signatures() {
        let (_, key) = keys();
        assert_eq!(key.verify("", "1", b"{}"), Err(Error::BadSignature));
        assert_eq!(key.verify("zz", "1", b"{}"), Err(Error::BadSignature));
        assert_eq!(
            key.verify(&"ab".repeat(63), "1", b"{}"),
            Err(Error::BadSignature)
        );
    }

    #[test]
    fn refuses_bad_public_keys() {
        assert!(DiscordKey::from_hex("not hex").is_err());
        assert!(DiscordKey::from_hex("abcd").is_err());
    }
}
