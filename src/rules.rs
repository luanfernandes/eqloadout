use serde::{Deserialize, Serialize};

/// Empty `exe` / `title` means "not set".
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub exe: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default)]
    pub profile: String,
}

impl Rule {
    pub fn matches(&self, exe: &str, title: &str) -> bool {
        (!self.exe.is_empty() || !self.title.is_empty())
            && (self.exe.is_empty() || self.exe.to_lowercase() == exe.to_lowercase())
            && (self.title.is_empty() || title.to_lowercase().contains(&self.title.to_lowercase()))
    }

    /// Short description of what this rule keys on, for the tooltip and logs.
    pub fn label(&self) -> String {
        match (self.exe.is_empty(), self.title.is_empty()) {
            (false, true) => self.exe.clone(),
            (true, false) => format!("title \"{}\"", self.title),
            _ => format!("{}, title \"{}\"", self.exe, self.title),
        }
    }
}

pub fn find<'a>(rules: &'a [Rule], exe: &str, title: &str) -> Option<&'a Rule> {
    rules.iter().find(|r| r.matches(exe, title))
}

/// A profile name that is safe to put after "Include:" in game.txt.
pub fn valid_profile(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.len() > 4
        && lower.ends_with(".txt")
        && lower != "config.txt"
        && lower != "game.txt" // would include itself
        && name.trim() == name
        && !name.contains(['/', '\\', ':', '\r', '\n'])
}

pub const NO_PROFILE: &str = "# eqswitch: no profile active\n";

#[derive(Debug, PartialEq)]
pub struct Outcome {
    pub content: String,
    /// Profile actually included, if any.
    pub profile: Option<String>,
    pub warning: Option<String>,
}

/// Builds game.txt for the matched profile, falling back to the default when a file is missing.
pub fn game_txt(matched: Option<&str>, default: Option<&str>, exists: impl Fn(&str) -> bool) -> Outcome {
    let mut warnings = Vec::new();
    for p in [matched, default].into_iter().flatten() {
        if !valid_profile(p) {
            warnings.push(format!("invalid profile name \"{p}\""));
        } else if !exists(p) {
            warnings.push(format!("profile {p} not found"));
        } else {
            return Outcome {
                content: format!("Include: {p}\n"),
                profile: Some(p.to_string()),
                warning: (!warnings.is_empty()).then(|| warnings.join("; ")),
            };
        }
    }
    Outcome {
        content: NO_PROFILE.to_string(),
        profile: None,
        warning: (!warnings.is_empty()).then(|| warnings.join("; ")),
    }
}

/// Comments out every active line, used on quit so no profile stays applied.
pub fn comment_out(content: &str) -> String {
    content
        .lines()
        .map(|l| if l.trim().is_empty() || l.trim_start().starts_with('#') { l.to_string() } else { format!("# {l}") })
        .map(|l| l + "\n")
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(exe: &str, title: &str, profile: &str) -> Rule {
        Rule { exe: exe.into(), title: title.into(), profile: profile.into() }
    }

    #[test]
    fn exe_only_is_exact_and_case_insensitive() {
        let r = rule("cs2.exe", "", "cs2.txt");
        assert!(r.matches("CS2.EXE", "anything"));
        assert!(!r.matches("cs2.exe.bak", ""));
        assert!(!r.matches("xcs2.exe", ""));
    }

    #[test]
    fn title_only_is_case_insensitive_substring() {
        let r = rule("", "Counter-Strike", "cs2.txt");
        assert!(r.matches("whatever.exe", "counter-strike 2"));
        assert!(!r.matches("whatever.exe", "Dota 2"));
    }

    #[test]
    fn both_fields_must_match() {
        let r = rule("javaw.exe", "minecraft", "mc.txt");
        assert!(r.matches("javaw.exe", "Minecraft 1.21"));
        assert!(!r.matches("javaw.exe", "IntelliJ"));
        assert!(!r.matches("java.exe", "Minecraft"));
    }

    #[test]
    fn empty_rule_never_matches() {
        assert!(!rule("", "", "x.txt").matches("", ""));
    }

    #[test]
    fn first_match_wins() {
        let rules = [rule("", "minecraft", "a.txt"), rule("javaw.exe", "", "b.txt")];
        assert_eq!(find(&rules, "javaw.exe", "Minecraft").unwrap().profile, "a.txt");
        assert_eq!(find(&rules, "javaw.exe", "Other").unwrap().profile, "b.txt");
        assert!(find(&rules, "x.exe", "y").is_none());
    }

    #[test]
    fn profile_names() {
        assert!(valid_profile("cs2.txt"));
        assert!(valid_profile("My Headphones ParametricEq.txt"));
        for bad in ["", ".txt", "cs2", "game.txt", "Config.TXT", "..\\x.txt", "a/b.txt", "C:x.txt", "a.txt\nCopy: x", " a.txt"] {
            assert!(!valid_profile(bad), "{bad:?}");
        }
    }

    #[test]
    fn content_for_match() {
        let o = game_txt(Some("cs2.txt"), Some("d.txt"), |_| true);
        assert_eq!(o, Outcome { content: "Include: cs2.txt\n".into(), profile: Some("cs2.txt".into()), warning: None });
    }

    #[test]
    fn content_falls_back_to_default_with_warning() {
        let o = game_txt(Some("cs2.txt"), Some("d.txt"), |p| p == "d.txt");
        assert_eq!(o.content, "Include: d.txt\n");
        assert_eq!(o.warning.as_deref(), Some("profile cs2.txt not found"));
    }

    #[test]
    fn content_without_anything_is_comment() {
        assert_eq!(game_txt(None, None, |_| true).content, NO_PROFILE);
        let o = game_txt(Some("a.txt"), Some("b.txt"), |_| false);
        assert_eq!(o.content, NO_PROFILE);
        assert_eq!(o.warning.as_deref(), Some("profile a.txt not found; profile b.txt not found"));
    }

    #[test]
    fn comment_out_lines() {
        assert_eq!(comment_out("Include: cs2.txt\n"), "# Include: cs2.txt\n");
        assert_eq!(comment_out("# x\n\nInclude: a.txt"), "# x\n\n# Include: a.txt\n");
    }
}
