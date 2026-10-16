// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! GitHub signs every webhook delivery with the App's webhook secret: `X-Hub-Signature-256` is
//! `sha256=` and the hex HMAC-SHA256 of the raw body. A delivery that fails the check is refused.

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

use crate::Error;

/// Checks the `X-Hub-Signature-256` header of a delivery. The comparison takes the same time
/// whatever the header holds.
pub fn verify_github(secret: &str, header: &str, body: &[u8]) -> Result<(), Error> {
    let hex_signature = header
        .trim()
        .strip_prefix("sha256=")
        .ok_or(Error::BadSignature)?;
    let signature = hex::decode(hex_signature).map_err(|_| Error::BadSignature)?;
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .map_err(|_| Error::BadKey("webhook secret"))?;
    mac.update(body);
    mac.verify_slice(&signature)
        .map_err(|_| Error::BadSignature)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sign(secret: &str, body: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
    }

    #[test]
    fn accepts_githubs_signature() {
        // The example from GitHub's documentation on validating webhook deliveries.
        assert_eq!(
            verify_github(
                "It's a Secret to Everybody",
                "sha256=757107ea0eb2509fc211221cce984b8a37570b6d7586c22c46f4379c8b043e17",
                b"Hello, World!",
            ),
            Ok(())
        );
        assert_eq!(verify_github("s", &sign("s", b"{}"), b"{}"), Ok(()));
    }

    #[test]
    fn refuses_a_wrong_or_missing_signature() {
        let header = sign("secret", b"{}");
        assert_eq!(
            verify_github("other", &header, b"{}"),
            Err(Error::BadSignature)
        );
        assert_eq!(
            verify_github("secret", &header, b"{ }"),
            Err(Error::BadSignature)
        );
        assert_eq!(verify_github("secret", "", b"{}"), Err(Error::BadSignature));
        assert_eq!(
            verify_github("secret", "sha1=abc", b"{}"),
            Err(Error::BadSignature)
        );
        assert_eq!(
            verify_github("secret", "sha256=zz", b"{}"),
            Err(Error::BadSignature)
        );
    }
}
