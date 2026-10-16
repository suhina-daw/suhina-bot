// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Forum tags, as Discord's channel objects hold them: a forum's `available_tags` (id and name)
//! and a post's `applied_tags` (ids).

use serde_json::Value;

/// A post holds at most five tags.
pub const MAX_TAGS: usize = 5;

/// The id of a forum's tag by name, ignoring case and surrounding spaces.
pub fn tag_id(forum: &Value, name: &str) -> Option<String> {
    forum
        .get("available_tags")?
        .as_array()?
        .iter()
        .find(|tag| {
            tag.get("name")
                .and_then(Value::as_str)
                .is_some_and(|n| n.trim().eq_ignore_ascii_case(name.trim()))
        })?
        .get("id")?
        .as_str()
        .map(str::to_owned)
}

/// The names of a post's tags.
pub fn tag_names(forum: &Value, post: &Value) -> Vec<String> {
    let available = forum
        .get("available_tags")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    applied(post)
        .iter()
        .filter_map(|id| {
            available
                .iter()
                .find(|tag| tag.get("id").and_then(Value::as_str) == Some(id.as_str()))
                .and_then(|tag| tag.get("name").and_then(Value::as_str))
                .map(|name| name.trim().to_owned())
        })
        .collect()
}

fn applied(post: &Value) -> Vec<String> {
    post.get("applied_tags")
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(|id| id.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// The post's tags with `tag` added. Already there: unchanged. Five already: the last one gives
/// way, so the new tag always shows.
pub fn with_tag(post: &Value, tag: &str) -> Vec<String> {
    let mut tags = applied(post);
    if tags.iter().any(|t| t == tag) {
        return tags;
    }
    tags.truncate(MAX_TAGS - 1);
    tags.push(tag.to_owned());
    tags
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn forum() -> Value {
        json!({ "available_tags": [
            { "id": "1", "name": "Mac" }, { "id": "2", "name": " UI " }, { "id": "9", "name": "Tracked" }
        ]})
    }

    #[test]
    fn finds_tags_by_name() {
        assert_eq!(tag_id(&forum(), "mac").as_deref(), Some("1"));
        assert_eq!(tag_id(&forum(), "UI").as_deref(), Some("2"));
        assert_eq!(tag_id(&forum(), "Linux"), None);
        assert_eq!(tag_id(&json!({}), "Mac"), None);
    }

    #[test]
    fn names_a_posts_tags() {
        let post = json!({ "applied_tags": ["2", "1", "404"] });
        assert_eq!(tag_names(&forum(), &post), ["UI", "Mac"]);
        assert!(tag_names(&forum(), &json!({})).is_empty());
    }

    #[test]
    fn adds_a_tag_within_the_limit() {
        assert_eq!(with_tag(&json!({ "applied_tags": ["1"] }), "9"), ["1", "9"]);
        assert_eq!(
            with_tag(&json!({ "applied_tags": ["1", "9"] }), "9"),
            ["1", "9"]
        );
        let full = json!({ "applied_tags": ["1", "2", "3", "4", "5"] });
        assert_eq!(with_tag(&full, "9"), ["1", "2", "3", "4", "9"]);
        assert_eq!(with_tag(&json!({}), "9"), ["9"]);
    }
}
