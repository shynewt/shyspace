//! A member's profile details: their role, pronouns, how to reach them and
//! when they work. Everyone in the workspace sees them.
//!
//! Details from others are hostile, so every device cleans them the same way
//! before keeping them: free text loses control and invisible characters and
//! is cut to its limit; a value that a cut would break or that could open
//! something unexpected (an email address, a phone number, a link, a time
//! zone) is cleared instead; working hours out of range are dropped. Your
//! own details are refused rather than changed, so what you typed is what
//! others see.

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use crate::ops::{cut_to, is_visible_in_name};

pub const MAX_TITLE_CHARS: usize = 80;
pub const MAX_PRONOUNS_CHARS: usize = 40;
pub const MAX_BIO_CHARS: usize = 300;
/// The longest address email allows (RFC 5321).
pub const MAX_EMAIL_CHARS: usize = 254;
pub const MAX_PHONE_CHARS: usize = 32;
pub const MAX_WEBSITE_CHARS: usize = 200;
pub const MAX_LOCATION_CHARS: usize = 80;
/// Real IANA names are under 32; this leaves room without inviting junk.
pub const MAX_TIME_ZONE_CHARS: usize = 64;
pub const MINUTES_PER_DAY: u16 = 24 * 60;
/// Monday to Sunday, bit 0 to bit 6.
const EVERY_DAY: u8 = 0b111_1111;

/// What a member tells others about themselves. Every field is optional: an
/// empty string is unset. The fields are part of the op format, which is
/// positional: never add, remove or reorder them; a new field needs a new op.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileDetails {
    /// Their role or position, like "Product designer".
    pub title: String,
    pub pronouns: String,
    /// Plain text; may hold line breaks.
    pub bio: String,
    pub email: String,
    pub phone: String,
    /// An http or https address.
    pub website: String,
    /// Where they work, like "Berlin office".
    pub location: String,
    /// An IANA name like "Europe/Lisbon". Only its shape is checked here; a
    /// name the reader's device doesn't know just shows no local time.
    pub time_zone: String,
    pub working_hours: Option<WorkingHours>,
}

/// One working interval, the same on each chosen day, in their time zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkingHours {
    /// Bit 0 is Monday and bit 6 Sunday: the days a working day starts on.
    pub days: u8,
    /// Minutes after midnight. When `end_minute` isn't after `start_minute`,
    /// the hours cross midnight into the next day.
    pub start_minute: u16,
    pub end_minute: u16,
}

impl WorkingHours {
    fn is_valid(&self) -> bool {
        (1..=EVERY_DAY).contains(&self.days)
            && self.start_minute < MINUTES_PER_DAY
            && self.end_minute <= MINUTES_PER_DAY
            && self.start_minute != self.end_minute
    }
}

impl ProfileDetails {
    /// These details as every device keeps them when they come from someone
    /// else.
    pub fn cleaned(&self) -> ProfileDetails {
        ProfileDetails {
            title: clean_line(&self.title, MAX_TITLE_CHARS),
            pronouns: clean_line(&self.pronouns, MAX_PRONOUNS_CHARS),
            bio: clean_lines(&self.bio, MAX_BIO_CHARS),
            email: kept_if(&self.email, is_valid_email),
            phone: kept_if(&self.phone, is_valid_phone),
            website: kept_if(&self.website, is_valid_website),
            location: clean_line(&self.location, MAX_LOCATION_CHARS),
            time_zone: kept_if(&self.time_zone, is_valid_time_zone),
            working_hours: self.working_hours.filter(WorkingHours::is_valid),
        }
    }

    /// Your own details, trimmed, or why they can't be saved as typed.
    pub fn checked(&self) -> Result<ProfileDetails> {
        let trimmed = ProfileDetails {
            title: self.title.trim().to_owned(),
            pronouns: self.pronouns.trim().to_owned(),
            bio: self.bio.trim().to_owned(),
            email: self.email.trim().to_owned(),
            phone: self.phone.trim().to_owned(),
            website: self.website.trim().to_owned(),
            location: self.location.trim().to_owned(),
            time_zone: self.time_zone.trim().to_owned(),
            working_hours: self.working_hours,
        };
        let cleaned = trimmed.cleaned();
        let plain = "characters, without invisible ones";
        ensure!(
            cleaned.title == trimmed.title,
            "a title has up to {MAX_TITLE_CHARS} {plain}"
        );
        ensure!(
            cleaned.pronouns == trimmed.pronouns,
            "pronouns have up to {MAX_PRONOUNS_CHARS} {plain}"
        );
        ensure!(
            cleaned.bio == trimmed.bio,
            "a bio has up to {MAX_BIO_CHARS} {plain}"
        );
        ensure!(
            cleaned.email == trimmed.email,
            "an email address looks like name@example.com (up to {MAX_EMAIL_CHARS} characters)"
        );
        ensure!(
            cleaned.phone == trimmed.phone,
            "a phone number uses digits, spaces and + ( ) - . (up to {MAX_PHONE_CHARS} characters)"
        );
        ensure!(
            cleaned.website == trimmed.website,
            "a website starts with https:// or http:// (up to {MAX_WEBSITE_CHARS} characters)"
        );
        ensure!(
            cleaned.location == trimmed.location,
            "a location has up to {MAX_LOCATION_CHARS} {plain}"
        );
        ensure!(
            cleaned.time_zone == trimmed.time_zone,
            "a time zone is a name like Europe/Lisbon"
        );
        ensure!(
            cleaned.working_hours == trimmed.working_hours,
            "working hours need at least one day, and a start and an end that differ"
        );
        Ok(trimmed)
    }
}

/// One line of text: visible characters only, trimmed, cut to `max`.
fn clean_line(raw: &str, max: usize) -> String {
    clean_text(raw, max, is_visible_in_name)
}

/// Like `clean_line`, keeping line breaks.
fn clean_lines(raw: &str, max: usize) -> String {
    clean_text(raw, max, |c| c == '\n' || is_visible_in_name(c))
}

fn clean_text(raw: &str, max: usize, is_kept: impl Fn(char) -> bool) -> String {
    let visible: String = raw.chars().filter(|c| is_kept(*c)).collect();
    cut_to(visible.trim(), max).trim_end().to_owned()
}

/// `raw` if it passes `is_valid` as it is, or nothing.
fn kept_if(raw: &str, is_valid: fn(&str) -> bool) -> String {
    match is_valid(raw) {
        true => raw.to_owned(),
        false => String::new(),
    }
}

/// A character an address may hold: visible by the rule for names, and
/// without the joiner names keep for emoji, which is invisible on its own.
fn is_plain_token_char(c: char) -> bool {
    !c.is_whitespace() && c != '\u{200D}' && is_visible_in_name(c)
}

/// Loose on purpose: `name@domain.tld`, nothing that could add to the
/// `mailto:` link it opens as (`?`, `#`, `%`) and no spaces or quoting.
fn is_valid_email(email: &str) -> bool {
    let Some((local, domain)) = email.split_once('@') else {
        return email.is_empty();
    };
    let is_allowed = |c: char| is_plain_token_char(c) && !"?#%/\\<>\"'()[],;:@".contains(c);
    email.chars().count() <= MAX_EMAIL_CHARS
        && !local.is_empty()
        && local.chars().all(is_allowed)
        && domain.chars().all(is_allowed)
        && domain.split('.').count() >= 2
        && domain.split('.').all(|label| !label.is_empty())
}

fn is_valid_phone(phone: &str) -> bool {
    phone.is_empty()
        || (phone.chars().count() <= MAX_PHONE_CHARS
            && phone.chars().any(|c| c.is_ascii_digit())
            && phone.chars().all(|c| c.is_ascii_digit() || " +()-.".contains(c)))
}

/// Only web addresses: `javascript:`, `data:`, `file:` and anything else
/// that isn't http(s) is cleared, so the link can only open a browser. A
/// user name before an `@` (or a `\\`, which browsers read as `/`) in the
/// address's host part is refused too: `https://bank.example@evil.example`
/// reads as one site and opens another.
fn is_valid_website(website: &str) -> bool {
    if website.is_empty() {
        return true;
    }
    let lowercase = website.to_ascii_lowercase();
    let Some(rest) = ["https://", "http://"]
        .into_iter()
        .find_map(|scheme| lowercase.strip_prefix(scheme))
    else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    website.chars().count() <= MAX_WEBSITE_CHARS
        && !authority.is_empty()
        && !authority.contains(['@', '\\'])
        && website.chars().all(is_plain_token_char)
}

fn is_valid_time_zone(time_zone: &str) -> bool {
    time_zone.chars().count() <= MAX_TIME_ZONE_CHARS
        && time_zone
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_+-/".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(change: impl FnOnce(&mut ProfileDetails)) -> ProfileDetails {
        let mut details = ProfileDetails::default();
        change(&mut details);
        details
    }

    fn full() -> ProfileDetails {
        ProfileDetails {
            title: "Product designer".into(),
            pronouns: "she/her".into(),
            bio: "Draws things.\nLikes tea.".into(),
            email: "ada@example.com".into(),
            phone: "+351 (21) 123-45.67".into(),
            website: "https://ada.example.com/about?x=1#me".into(),
            location: "Remote, Lisbon".into(),
            time_zone: "America/Argentina/Buenos_Aires".into(),
            working_hours: Some(WorkingHours {
                days: 0b001_1111,
                start_minute: 9 * 60,
                end_minute: 17 * 60,
            }),
        }
    }

    #[test]
    fn honest_details_stay_as_they_are() {
        assert_eq!(full().cleaned(), full());
        assert_eq!(full().checked().unwrap(), full());
        assert_eq!(ProfileDetails::default().cleaned(), ProfileDetails::default());
    }

    #[test]
    fn free_text_is_cut_to_its_limit_on_a_character_boundary() {
        let long = with(|details| {
            details.title = "é".repeat(500);
            details.pronouns = "x".repeat(500);
            details.bio = "🎉".repeat(500);
            details.location = "y".repeat(500);
        })
        .cleaned();
        assert_eq!(long.title, "é".repeat(MAX_TITLE_CHARS));
        assert_eq!(long.pronouns.chars().count(), MAX_PRONOUNS_CHARS);
        assert_eq!(long.bio, "🎉".repeat(MAX_BIO_CHARS));
        assert_eq!(long.location.chars().count(), MAX_LOCATION_CHARS);
    }

    #[test]
    fn structured_values_over_their_limit_are_cleared_not_cut() {
        let long = with(|details| {
            details.email = format!("{}@example.com", "a".repeat(MAX_EMAIL_CHARS));
            details.phone = "1".repeat(MAX_PHONE_CHARS + 1);
            details.website = format!("https://example.com/{}", "a".repeat(MAX_WEBSITE_CHARS));
            details.time_zone = "A".repeat(MAX_TIME_ZONE_CHARS + 1);
        })
        .cleaned();
        assert_eq!(long, ProfileDetails::default());
    }

    #[test]
    fn invisible_and_direction_characters_go_and_text_is_trimmed() {
        let sneaky = with(|details| {
            details.title = "  Ad\u{202E}min\u{200B} \u{0007}".into();
            details.bio = "line one\r\nline\u{2028} two\u{E0041}\n".into();
            details.pronouns = "👩‍💻 they/them".into();
        })
        .cleaned();
        assert_eq!(sneaky.title, "Admin");
        assert_eq!(sneaky.bio, "line one\nline two");
        assert_eq!(sneaky.pronouns, "👩‍💻 they/them", "emoji keep their joiners");
    }

    #[test]
    fn a_line_break_only_survives_in_the_bio() {
        let details = with(|details| details.title = "Lead\nDesigner".into()).cleaned();
        assert_eq!(details.title, "LeadDesigner");
    }

    #[test]
    fn only_web_addresses_are_kept_as_websites() {
        for bad in [
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "file:///etc/passwd",
            "shy://member/abc",
            "example.com",
            "https://",
            "https:///path",
            "https://exa mple.com",
            "https://example.com/\u{202E}",
            " https://example.com",
            "https://accounts.google.com@evil.example/login",
            "https://user:pass@example.com",
            "https://evil.example\\@google.com",
            "https://evil.example\\google.com",
            "https://example.com\u{E0041}/",
        ] {
            let details = with(|details| details.website = bad.into());
            assert_eq!(details.cleaned().website, "", "{bad}");
        }
        for good in [
            "http://example.com",
            "HTTPS://Example.com/a?b#c",
            "https://example.com/@ada",
            "https://example.com/?from=a@b.c",
        ] {
            let details = with(|details| details.website = good.into());
            assert_eq!(details.cleaned().website, good);
        }
    }

    #[test]
    fn emails_and_phones_that_could_open_something_else_are_cleared() {
        for bad in [
            "ada",
            "ada@",
            "@example.com",
            "ada@example",
            "ada@example..com",
            "ada@@example.com",
            "ada@example.com?body=hi",
            "ada@example.com#x",
            "a%3F@example.com",
            "ada smith@example.com",
            "\"ada\"@example.com",
            "ceo\u{E0041}@corp.com",
            "ceo\u{200D}@corp.com",
        ] {
            let details = with(|details| details.email = bad.into());
            assert_eq!(details.cleaned().email, "", "{bad}");
        }
        for bad in ["call me", "+1 555 0100 ext. 2", "()-.", "tel:+15550100", "١٢٣"] {
            let details = with(|details| details.phone = bad.into());
            assert_eq!(details.cleaned().phone, "", "{bad}");
        }
    }

    #[test]
    fn time_zones_need_only_the_shape_of_a_name() {
        for bad in ["Europe/Lisbon\u{202E}", "Europe Lisbon", "../../etc", "Zürich"] {
            let details = with(|details| details.time_zone = bad.into());
            assert_eq!(details.cleaned().time_zone, "", "{bad}");
        }
        for good in ["Etc/GMT+5", "Mars/Olympus_Mons", "UTC"] {
            let details = with(|details| details.time_zone = good.into());
            assert_eq!(details.cleaned().time_zone, good, "unknown names are kept");
        }
    }

    #[test]
    fn working_hours_out_of_range_are_dropped() {
        let hours = |days, start_minute, end_minute| WorkingHours {
            days,
            start_minute,
            end_minute,
        };
        for bad in [
            hours(0, 540, 1020),
            hours(0b1000_0000, 540, 1020),
            hours(0xFF, 540, 1020),
            hours(1, 1440, 60),
            hours(1, 540, 1441),
            hours(1, 540, 540),
            hours(1, u16::MAX, u16::MAX - 1),
        ] {
            let details = with(|details| details.working_hours = Some(bad));
            assert_eq!(details.cleaned().working_hours, None, "{bad:?}");
        }
        for good in [hours(EVERY_DAY, 0, 1440), hours(0b100_0000, 22 * 60, 6 * 60)] {
            let details = with(|details| details.working_hours = Some(good));
            assert_eq!(
                details.cleaned().working_hours,
                Some(good),
                "crossing midnight is fine"
            );
        }
    }

    #[test]
    fn your_own_details_are_refused_rather_than_changed() {
        let trimmed = with(|details| details.title = "  Designer  ".into());
        assert_eq!(trimmed.checked().unwrap().title, "Designer");
        let refused = [
            with(|details| details.title = "x".repeat(MAX_TITLE_CHARS + 1)),
            with(|details| details.pronouns = "she\u{202E}".into()),
            with(|details| details.bio = "b".repeat(MAX_BIO_CHARS + 1)),
            with(|details| details.email = "ada@example".into()),
            with(|details| details.phone = "call me".into()),
            with(|details| details.website = "javascript:alert(1)".into()),
            with(|details| details.website = "example.com".into()),
            with(|details| details.location = "\u{0007}".into()),
            with(|details| details.time_zone = "Europe Lisbon".into()),
            with(|details| {
                details.working_hours = Some(WorkingHours {
                    days: 0,
                    start_minute: 540,
                    end_minute: 1020,
                })
            }),
        ];
        for details in refused {
            assert!(details.checked().is_err(), "{details:?}");
        }
    }
}
