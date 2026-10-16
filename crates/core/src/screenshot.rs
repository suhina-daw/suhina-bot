// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Screenshots a moderator selected for an issue are copied into the bot's R2 bucket, because
//! GitHub has no API for attaching a file to an issue and Discord's links expire after about a
//! day. Only images are copied: the content type and the file's first bytes must agree, and the
//! size is capped. The stored name is random and holds nothing from the report.

/// Largest file copied.
pub const MAX_BYTES: usize = 10 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Image {
    Png,
    Jpeg,
    Gif,
    WebP,
}

impl Image {
    pub fn extension(self) -> &'static str {
        match self {
            Image::Png => "png",
            Image::Jpeg => "jpg",
            Image::Gif => "gif",
            Image::WebP => "webp",
        }
    }

    pub fn content_type(self) -> &'static str {
        match self {
            Image::Png => "image/png",
            Image::Jpeg => "image/jpeg",
            Image::Gif => "image/gif",
            Image::WebP => "image/webp",
        }
    }

    fn from_content_type(content_type: &str) -> Option<Self> {
        let base = content_type.split(';').next().unwrap_or_default().trim();
        match base.to_ascii_lowercase().as_str() {
            "image/png" => Some(Image::Png),
            "image/jpeg" | "image/jpg" => Some(Image::Jpeg),
            "image/gif" => Some(Image::Gif),
            "image/webp" => Some(Image::WebP),
            _ => None,
        }
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
            Some(Image::Png)
        } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
            Some(Image::Jpeg)
        } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
            Some(Image::Gif)
        } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
            Some(Image::WebP)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    NotAnImage,
    Mismatch,
    TooLarge,
    Empty,
}

/// What the file is, if it may be copied.
pub fn check(content_type: Option<&str>, bytes: &[u8]) -> Result<Image, Refused> {
    if bytes.is_empty() {
        return Err(Refused::Empty);
    }
    if bytes.len() > MAX_BYTES {
        return Err(Refused::TooLarge);
    }
    let claimed = content_type
        .and_then(Image::from_content_type)
        .ok_or(Refused::NotAnImage)?;
    let real = Image::from_bytes(bytes).ok_or(Refused::NotAnImage)?;
    if claimed != real {
        return Err(Refused::Mismatch);
    }
    Ok(real)
}

/// The object's name in the bucket, from a random UUID: `screenshots/<uuid>.<ext>`.
pub fn key(uuid: &str, image: Image) -> Option<String> {
    let valid = uuid.len() == 36 && uuid.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    valid.then(|| {
        format!(
            "screenshots/{}.{}",
            uuid.to_ascii_lowercase(),
            image.extension()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0];

    #[test]
    fn accepts_images_whose_bytes_match() {
        assert_eq!(check(Some("image/png"), PNG), Ok(Image::Png));
        assert_eq!(
            check(Some("image/jpeg"), &[0xff, 0xd8, 0xff, 0xe0]),
            Ok(Image::Jpeg)
        );
        assert_eq!(check(Some("image/gif"), b"GIF89a..."), Ok(Image::Gif));
        assert_eq!(
            check(Some("image/webp; charset=x"), b"RIFF\0\0\0\0WEBPVP8 "),
            Ok(Image::WebP)
        );
    }

    #[test]
    fn refuses_everything_else() {
        assert_eq!(check(Some("image/png"), b""), Err(Refused::Empty));
        assert_eq!(check(Some("text/html"), PNG), Err(Refused::NotAnImage));
        assert_eq!(check(None, PNG), Err(Refused::NotAnImage));
        assert_eq!(check(Some("image/png"), b"<svg>"), Err(Refused::NotAnImage));
        assert_eq!(check(Some("image/jpeg"), PNG), Err(Refused::Mismatch));
        assert_eq!(
            check(Some("image/svg+xml"), b"<svg/>"),
            Err(Refused::NotAnImage)
        );
        let mut big = PNG.to_vec();
        big.resize(MAX_BYTES + 1, 0);
        assert_eq!(check(Some("image/png"), &big), Err(Refused::TooLarge));
    }

    #[test]
    fn keys_are_random_names_only() {
        let uuid = "0F8FAD5B-D9CB-469F-A165-70867728950E";
        assert_eq!(
            key(uuid, Image::Png).as_deref(),
            Some("screenshots/0f8fad5b-d9cb-469f-a165-70867728950e.png")
        );
        assert_eq!(key("../../etc/passwd", Image::Png), None);
        assert_eq!(key("", Image::Png), None);
    }
}
