// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The pieces of GitHub App authentication that need no network: the App's JWT (signed by the
//! worker with WebCrypto, RS256) and the private key in the form WebCrypto imports.
//!
//! GitHub hands out the key as PKCS#1 (`BEGIN RSA PRIVATE KEY`); WebCrypto imports only PKCS#8,
//! so `pkcs8_der` wraps it. A key already in PKCS#8 (`BEGIN PRIVATE KEY`) is taken as it is.

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use serde::Deserialize;
use serde_json::json;

use crate::Error;

/// The App's JWT is valid from a minute ago (GitHub's clock may run behind) for nine minutes;
/// GitHub refuses one that lives longer than ten.
const JWT_BACKDATE_SECONDS: u64 = 60;
const JWT_LIFETIME_SECONDS: u64 = 540;

/// `header.claims` of the App's JWT, base64url-encoded, ready to be signed with RS256.
pub fn jwt_signing_input(app_id: &str, now_seconds: u64) -> String {
    let header = json!({ "alg": "RS256", "typ": "JWT" });
    let claims = json!({
        "iat": now_seconds.saturating_sub(JWT_BACKDATE_SECONDS),
        "exp": now_seconds.saturating_add(JWT_LIFETIME_SECONDS),
        "iss": app_id,
    });
    format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header.to_string()),
        URL_SAFE_NO_PAD.encode(claims.to_string())
    )
}

/// The finished JWT from its signing input and the RS256 signature over it.
pub fn jwt(signing_input: &str, signature: &[u8]) -> String {
    format!("{signing_input}.{}", URL_SAFE_NO_PAD.encode(signature))
}

/// The private key as PKCS#8 DER, from the PEM text GitHub gave (PKCS#1 or PKCS#8).
pub fn pkcs8_der(pem: &str) -> Result<Vec<u8>, Error> {
    let (label, der) = pem_body(pem)?;
    match label.as_str() {
        "PRIVATE KEY" => Ok(der),
        "RSA PRIVATE KEY" => Ok(wrap_pkcs1(&der)),
        _ => Err(Error::BadKey(
            "private key is neither PKCS#1 nor PKCS#8 PEM",
        )),
    }
}

/// The label and decoded body of the first PEM block. Secret values have `\n` written as two
/// characters when pasted into some tools, so those are read as line breaks too.
fn pem_body(pem: &str) -> Result<(String, Vec<u8>), Error> {
    let text = pem.replace("\\n", "\n");
    let begin = text
        .find("-----BEGIN ")
        .ok_or(Error::BadKey("private key has no BEGIN line"))?;
    let after_begin = text
        .get(begin + "-----BEGIN ".len()..)
        .ok_or(Error::BadKey("private key is cut"))?;
    let label_end = after_begin
        .find("-----")
        .ok_or(Error::BadKey("private key has a bad BEGIN line"))?;
    let label = after_begin
        .get(..label_end)
        .ok_or(Error::BadKey("private key is cut"))?
        .to_owned();
    let body_start = label_end + "-----".len();
    let rest = after_begin
        .get(body_start..)
        .ok_or(Error::BadKey("private key is cut"))?;
    let end = rest
        .find("-----END ")
        .ok_or(Error::BadKey("private key has no END line"))?;
    let base64_text: String = rest
        .get(..end)
        .ok_or(Error::BadKey("private key is cut"))?
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let der = STANDARD
        .decode(base64_text)
        .map_err(|_| Error::BadKey("private key is not base64"))?;
    Ok((label, der))
}

/// PKCS#8 `PrivateKeyInfo` around a PKCS#1 `RSAPrivateKey`:
/// `SEQUENCE { INTEGER 0, SEQUENCE { OID rsaEncryption, NULL }, OCTET STRING pkcs1 }`.
fn wrap_pkcs1(pkcs1: &[u8]) -> Vec<u8> {
    const VERSION: [u8; 3] = [0x02, 0x01, 0x00];
    const RSA_ALGORITHM: [u8; 15] = [
        0x30, 0x0d, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01, 0x05, 0x00,
    ];
    let mut octet_string = vec![0x04];
    octet_string.extend(der_length(pkcs1.len()));
    octet_string.extend_from_slice(pkcs1);

    let content_length = VERSION.len() + RSA_ALGORITHM.len() + octet_string.len();
    let mut out = vec![0x30];
    out.extend(der_length(content_length));
    out.extend_from_slice(&VERSION);
    out.extend_from_slice(&RSA_ALGORITHM);
    out.extend(octet_string);
    out
}

/// A DER length: one byte below 128, otherwise 0x80 + the count of big-endian bytes that follow.
fn der_length(length: usize) -> Vec<u8> {
    if length < 0x80 {
        return vec![length as u8];
    }
    let bytes: Vec<u8> = length
        .to_be_bytes()
        .into_iter()
        .skip_while(|b| *b == 0)
        .collect();
    let mut out = vec![0x80 | bytes.len() as u8];
    out.extend(bytes);
    out
}

/// GitHub's answer to `POST /app/installations/{id}/access_tokens`.
#[derive(Debug, Deserialize)]
pub struct InstallationToken {
    pub token: String,
    pub expires_at: String,
}

/// GitHub's answer to `GET /repos/{owner}/{repo}/installation`.
#[derive(Debug, Deserialize)]
pub struct Installation {
    pub id: u64,
}

pub fn parse_installation_token(body: &[u8]) -> Result<InstallationToken, Error> {
    serde_json::from_slice(body).map_err(|_| Error::GitHub("installation token answer".into()))
}

pub fn parse_installation(body: &[u8]) -> Result<Installation, Error> {
    serde_json::from_slice(body).map_err(|_| Error::GitHub("installation answer".into()))
}

/// The project board: its id and its single-select fields (Status, Priority, Phase) with their
/// options, from `organization { projectV2 { fields } }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Board {
    pub id: String,
    pub fields: Vec<BoardField>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardField {
    pub id: String,
    pub name: String,
    /// `(name, id)`
    pub options: Vec<(String, String)>,
}

pub const BOARD_QUERY: &str = "query($owner:String!,$number:Int!){organization(login:$owner){projectV2(number:$number){id fields(first:50){nodes{... on ProjectV2SingleSelectField{id name options{id name}}}}}}}";

impl Board {
    pub fn parse(data: &serde_json::Value) -> Option<Self> {
        let project = data.get("organization")?.get("projectV2")?;
        let fields = project
            .get("fields")?
            .get("nodes")?
            .as_array()?
            .iter()
            .filter_map(|node| {
                let options = node
                    .get("options")?
                    .as_array()?
                    .iter()
                    .filter_map(|o| {
                        Some((
                            o.get("name")?.as_str()?.to_owned(),
                            o.get("id")?.as_str()?.to_owned(),
                        ))
                    })
                    .collect();
                Some(BoardField {
                    id: node.get("id")?.as_str()?.to_owned(),
                    name: node.get("name")?.as_str()?.to_owned(),
                    options,
                })
            })
            .collect();
        Some(Self {
            id: project.get("id")?.as_str()?.to_owned(),
            fields,
        })
    }

    /// `(field id, option id)` of a field's option, both by name, ignoring case.
    pub fn option(&self, field: &str, option: &str) -> Option<(&str, &str)> {
        let field = self
            .fields
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case(field))?;
        let (_, id) = field
            .options
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(option))?;
        Some((field.id.as_str(), id.as_str()))
    }

    /// The option names of a field, in the board's order.
    pub fn options(&self, field: &str) -> Vec<String> {
        self.fields
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case(field))
            .map(|f| f.options.iter().map(|(name, _)| name.clone()).collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signing_input_holds_the_app_and_a_short_lifetime() {
        let input = jwt_signing_input("<app-id>", 1_000_000);
        let (header, claims) = input.split_once('.').unwrap();
        let header: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(header).unwrap()).unwrap();
        let claims: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(claims).unwrap()).unwrap();
        assert_eq!(header["alg"], "RS256");
        assert_eq!(claims["iss"], "<app-id>");
        assert_eq!(claims["iat"], 999_940);
        assert_eq!(claims["exp"], 1_000_540);
        assert_eq!(jwt(&input, &[1, 2, 3]), format!("{input}.AQID"));
    }

    #[test]
    fn der_lengths() {
        assert_eq!(der_length(5), [5]);
        assert_eq!(der_length(127), [127]);
        assert_eq!(der_length(128), [0x81, 128]);
        assert_eq!(der_length(1190), [0x82, 0x04, 0xa6]);
    }

    #[test]
    fn wraps_pkcs1_in_pkcs8() {
        let pkcs1 = vec![0xAB; 1190];
        let wrapped = wrap_pkcs1(&pkcs1);
        // SEQUENCE, long length, then version and the rsaEncryption algorithm.
        assert_eq!(&wrapped[..4], &[0x30, 0x82, 0x04, 0xbc]);
        assert_eq!(&wrapped[4..7], &[0x02, 0x01, 0x00]);
        assert_eq!(wrapped[7], 0x30);
        assert_eq!(&wrapped[22..26], &[0x04, 0x82, 0x04, 0xa6]);
        assert_eq!(&wrapped[26..], pkcs1.as_slice());
        assert_eq!(wrapped.len(), 4 + 0x04bc);
    }

    fn pem(label: &str, der: &[u8]) -> String {
        format!(
            "-----BEGIN {label}-----\n{}\n-----END {label}-----\n",
            STANDARD.encode(der)
        )
    }

    #[test]
    fn reads_pkcs1_and_pkcs8_pem() {
        let der = vec![1, 2, 3, 4];
        assert_eq!(pkcs8_der(&pem("PRIVATE KEY", &der)).unwrap(), der);
        assert_eq!(
            pkcs8_der(&pem("RSA PRIVATE KEY", &der)).unwrap(),
            wrap_pkcs1(&der)
        );
        // Written on one line with literal \n, as some secret stores keep it.
        let one_line = pem("RSA PRIVATE KEY", &der).replace('\n', "\\n");
        assert_eq!(pkcs8_der(&one_line).unwrap(), wrap_pkcs1(&der));
    }

    #[test]
    fn refuses_what_is_not_a_private_key() {
        assert!(pkcs8_der("").is_err());
        assert!(pkcs8_der(&pem("PUBLIC KEY", &[1])).is_err());
        assert!(
            pkcs8_der("-----BEGIN RSA PRIVATE KEY-----\n!!!\n-----END RSA PRIVATE KEY-----")
                .is_err()
        );
        assert!(pkcs8_der("-----BEGIN RSA PRIVATE KEY-----\nAAAA").is_err());
    }

    #[test]
    fn reads_github_answers() {
        let token = parse_installation_token(
            br#"{"token":"ghs_x","expires_at":"2026-10-16T20:00:00Z","permissions":{}}"#,
        )
        .unwrap();
        assert_eq!(token.token, "ghs_x");
        assert_eq!(
            parse_installation(br#"{"id":<installation-id>,"account":{}}"#)
                .unwrap()
                .id,
            <installation-id>
        );
        assert!(parse_installation(b"{}").is_err());
    }

    #[test]
    fn reads_the_board() {
        let data = serde_json::json!({ "organization": { "projectV2": { "id": "P", "fields": { "nodes": [
            {},
            { "id": "F1", "name": "Status", "options": [{ "id": "o1", "name": "Backlog" }, { "id": "o2", "name": "Ready" }] },
            { "id": "F2", "name": "Phase", "options": [{ "id": "o3", "name": "M8 UI" }] },
        ]}}}});
        let board = Board::parse(&data).unwrap();
        assert_eq!(board.id, "P");
        assert_eq!(board.option("status", "backlog"), Some(("F1", "o1")));
        assert_eq!(board.option("Status", "Done"), None);
        assert_eq!(board.options("Phase"), ["M8 UI"]);
        assert!(board.options("Priority").is_empty());
        assert_eq!(Board::parse(&serde_json::json!({})), None);
    }
}
