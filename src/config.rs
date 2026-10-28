use std::{fs, io, path::PathBuf, ptr::null_mut};

use serde::{Deserialize, Serialize};
use windows_sys::Win32::{
    Foundation::ERROR_SUCCESS,
    System::Registry::{HKEY, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW},
};

use crate::{eapo, rules::Rule, wide};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "detect_eapo_dir")]
    pub eapo_config_dir: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_profile: Option<String>,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

impl Default for Config {
    fn default() -> Self {
        Config { eapo_config_dir: detect_eapo_dir(), default_profile: None, rules: Vec::new() }
    }
}

impl Config {
    pub fn parse(s: &str) -> Result<Config, String> {
        toml::from_str(s).map_err(|e| e.to_string())
    }

    /// Rules that can never match or point at an unusable profile name.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (i, r) in self.rules.iter().enumerate() {
            if r.exe.is_empty() && r.title.is_empty() {
                out.push(format!("rule {} has neither exe nor title", i + 1));
            }
            if !crate::rules::valid_profile(&r.profile) {
                out.push(format!("rule {} has invalid profile \"{}\"", i + 1, r.profile));
            }
        }
        out
    }
}

pub fn path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("eqswitch").join("config.toml"))
}

/// Never fails: on any problem it returns defaults plus a message for the UI.
/// A broken file is left untouched so the user can fix it.
pub fn load() -> (Config, Option<String>) {
    let Some(path) = path() else { return (Config::default(), Some("APPDATA is not set".into())) };
    match fs::read_to_string(&path) {
        Ok(s) => match Config::parse(&s) {
            Ok(c) => (c, None),
            Err(e) => (Config::default(), Some(format!("{} is invalid, using defaults: {e}", path.display()))),
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let c = first_run();
            let err = save(&c).err().map(|e| format!("could not create {}: {e}", path.display()));
            (c, err)
        }
        Err(e) => (Config::default(), Some(format!("could not read {}: {e}", path.display()))),
    }
}

pub fn save(c: &Config) -> io::Result<()> {
    let path = path().ok_or_else(|| io::Error::other("APPDATA is not set"))?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    eapo::write_atomic(&path, &toml::to_string(c).map_err(io::Error::other)?)
}

fn first_run() -> Config {
    Config {
        rules: vec![Rule { exe: "cs2.exe".into(), title: String::new(), profile: "cs2.txt".into() }],
        ..Config::default()
    }
}

fn detect_eapo_dir() -> PathBuf {
    reg_string(HKEY_LOCAL_MACHINE, r"SOFTWARE\EqualizerAPO", "ConfigPath")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files\EqualizerAPO\config"))
}

pub fn reg_string(root: HKEY, subkey: &str, value: &str) -> Option<String> {
    let (k, v) = (wide(subkey), wide(value));
    let mut buf = [0u16; 1024];
    let mut len = size_of_val(&buf) as u32;
    let rc = unsafe { RegGetValueW(root, k.as_ptr(), v.as_ptr(), RRF_RT_REG_SZ, null_mut(), buf.as_mut_ptr().cast(), &mut len) };
    // len is in bytes and includes the terminating NUL
    (rc == ERROR_SUCCESS).then(|| String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_config() {
        let c = Config::parse(
            r#"
            eapo_config_dir = 'E:\Audio\EqualizerAPO\config'
            default_profile = "flat.txt"
            [[rules]]
            exe = "cs2.exe"
            profile = "cs2.txt"
            [[rules]]
            title = "Minecraft"
            profile = "mc.txt"
            "#,
        )
        .unwrap();
        assert_eq!(c.eapo_config_dir, PathBuf::from(r"E:\Audio\EqualizerAPO\config"));
        assert_eq!(c.default_profile.as_deref(), Some("flat.txt"));
        assert_eq!(c.rules.len(), 2);
        assert_eq!(c.rules[1], Rule { exe: "".into(), title: "Minecraft".into(), profile: "mc.txt".into() });
        assert!(c.problems().is_empty());
    }

    #[test]
    fn missing_fields_get_defaults() {
        let c = Config::parse("").unwrap();
        assert!(c.default_profile.is_none() && c.rules.is_empty());
        assert!(!c.eapo_config_dir.as_os_str().is_empty());
    }

    #[test]
    fn broken_toml_is_an_error() {
        assert!(Config::parse("rules = [[[").is_err());
        assert!(Config::parse("rules = 5").is_err());
    }

    #[test]
    fn reports_bad_rules() {
        let c = Config::parse("[[rules]]\nprofile = \"a.txt\"\n[[rules]]\nexe = \"x.exe\"\nprofile = \"game.txt\"").unwrap();
        assert_eq!(c.problems().len(), 2);
    }

    #[test]
    fn round_trips() {
        let c = first_run();
        assert_eq!(Config::parse(&toml::to_string(&c).unwrap()).unwrap(), c);
    }
}
