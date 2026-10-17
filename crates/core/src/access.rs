// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Who may do what. Anyone may report a bug; turning a report into a GitHub issue is for staff
//! (the roles `Mod`, `github-mod` and `github-contributor`), and only triagers (`github-mod`) set
//! the board's Priority and Phase. A server administrator counts as both. The role IDs are the
//! Worker's settings `STAFF_ROLE_IDS` and `TRIAGE_ROLE_IDS`, comma-separated.
//!
//! All of it holds in Suhina's own server only, `DISCORD_GUILD_ID`: anywhere else (another server
//! the bot was added to, a direct message) no one is staff, and no command that writes runs. An
//! administrator of another server is nobody here. Without the setting, nothing writes.

use crate::interaction::Interaction;

/// Discord's ADMINISTRATOR permission bit.
const ADMINISTRATOR: u64 = 1 << 3;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Roles {
    staff: Vec<String>,
    triage: Vec<String>,
}

impl Roles {
    /// From the two settings; blanks and spaces are ignored. Triage roles are staff roles too.
    pub fn from_settings(staff: &str, triage: &str) -> Self {
        let ids = |text: &str| -> Vec<String> {
            text.split(',')
                .map(str::trim)
                .filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()))
                .map(str::to_owned)
                .collect()
        };
        Self {
            staff: ids(staff),
            triage: ids(triage),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Access {
    /// The interaction comes from Suhina's own server.
    pub home: bool,
    /// May turn reports into issues.
    pub staff: bool,
    /// May also set Priority and Phase.
    pub triage: bool,
}

impl Access {
    /// `home_guild` is the setting `DISCORD_GUILD_ID`; `None` (not set) is nowhere.
    pub fn of(interaction: &Interaction, roles: &Roles, home_guild: Option<&str>) -> Self {
        let home = home_guild.is_some() && interaction.guild_id.as_deref() == home_guild;
        if !home {
            return Self::default();
        }
        if interaction.permissions() & ADMINISTRATOR != 0 {
            return Self {
                home,
                staff: true,
                triage: true,
            };
        }
        let has = |list: &[String]| interaction.roles().iter().any(|role| list.contains(role));
        let triage = has(&roles.triage);
        Self {
            home,
            staff: triage || has(&roles.staff),
            triage,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(roles: &str, permissions: &str) -> Interaction {
        let body = format!(
            r#"{{"type":2,"guild_id":"g1","member":{{"user":{{"id":"1"}},"roles":[{roles}],"permissions":"{permissions}"}}}}"#
        );
        Interaction::parse(body.as_bytes()).unwrap()
    }

    #[test]
    fn roles_decide_access() {
        let roles = Roles::from_settings("10, 11 ,", "12");
        assert_eq!(
            Access::of(&member(r#""10""#, "0"), &roles, Some("g1")),
            Access {
                home: true,
                staff: true,
                triage: false
            }
        );
        assert_eq!(
            Access::of(&member(r#""12""#, "0"), &roles, Some("g1")),
            Access {
                home: true,
                staff: true,
                triage: true
            }
        );
        let at_home = Access {
            home: true,
            staff: false,
            triage: false,
        };
        assert_eq!(
            Access::of(&member(r#""99""#, "0"), &roles, Some("g1")),
            at_home
        );
        assert_eq!(Access::of(&member("", "0"), &roles, Some("g1")), at_home);
    }

    #[test]
    fn an_administrator_may_do_everything() {
        let roles = Roles::default();
        assert_eq!(
            Access::of(&member("", "8"), &roles, Some("g1")),
            Access {
                home: true,
                staff: true,
                triage: true
            }
        );
        // Other permissions alone give nothing.
        assert!(!Access::of(&member("", "8192"), &roles, Some("g1")).staff);
    }

    #[test]
    fn bad_settings_give_no_roles() {
        let roles = Roles::from_settings("abc, ,", "");
        assert!(!Access::of(&member(r#""abc""#, "0"), &roles, Some("g1")).staff);
    }

    #[test]
    fn another_servers_administrator_is_nobody() {
        let body = r#"{"type":2,"guild_id":"other","member":{"user":{"id":"1"},"roles":["10"],"permissions":"8"}}"#;
        let elsewhere = Interaction::parse(body.as_bytes()).unwrap();
        let roles = Roles::from_settings("10", "10");
        assert_eq!(
            Access::of(&elsewhere, &roles, Some("g1")),
            Access::default()
        );
        // Without the setting, no server is home.
        assert_eq!(
            Access::of(&member(r#""10""#, "8"), &roles, None),
            Access::default()
        );
    }

    #[test]
    fn outside_a_server_no_one_is_staff() {
        let dm = Interaction::parse(br#"{"type":2,"user":{"id":"1"}}"#).unwrap();
        assert_eq!(
            Access::of(&dm, &Roles::from_settings("1", "1"), Some("g1")),
            Access::default()
        );
    }
}
