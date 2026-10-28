use std::{fs, io, path::Path};

use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW};

use crate::wide;

/// Writes a sibling temp file, then swaps it in so readers never see a partial file.
pub fn write_atomic(target: &Path, content: &str) -> io::Result<()> {
    let mut tmp = target.as_os_str().to_owned();
    tmp.push(".eqswitch.tmp");
    fs::write(&tmp, content)?;
    let ok = unsafe { MoveFileExW(wide(&tmp).as_ptr(), wide(target).as_ptr(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) };
    if ok == 0 {
        let e = io::Error::last_os_error();
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// True if config.txt has an active "Include:" line naming game.txt.
pub fn includes_game_txt(config_txt: &str) -> bool {
    config_txt.lines().any(|l| {
        l.trim()
            .split_once(':')
            .is_some_and(|(k, v)| k.trim().eq_ignore_ascii_case("include") && v.split(';').any(|f| f.trim().eq_ignore_ascii_case("game.txt")))
    })
}

/// Profile files in the config folder (valid .txt names except config.txt / game.txt), sorted.
pub fn list_profiles(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| crate::rules::valid_profile(n))
        .collect();
    out.sort_by_key(|n| n.to_lowercase());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_profiles() {
        let dir = std::env::temp_dir().join(format!("eqswitch-profiles-{}", std::process::id()));
        fs::create_dir_all(dir.join("sub.txt")).unwrap();
        for f in ["config.txt", "game.txt", "b.txt", "A.txt", "notes.md", "game.txt.eqswitch.tmp"] {
            fs::write(dir.join(f), "").unwrap();
        }
        assert_eq!(list_profiles(&dir), ["A.txt", "b.txt"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn atomic_write_creates_and_replaces() {
        let dir = std::env::temp_dir().join(format!("eqswitch-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("game.txt");
        write_atomic(&target, "one\n").unwrap();
        write_atomic(&target, "Include: cs2.txt\n").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "Include: cs2.txt\n");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1, "temp file left behind");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn atomic_write_reports_missing_dir() {
        let target = std::env::temp_dir().join("eqswitch-no-such-dir").join("game.txt");
        assert!(write_atomic(&target, "x").is_err());
    }

    #[test]
    fn detects_game_include() {
        let cfg = "Device:  Speakers {00000000-0000-0000-0000-000000000000}\nInclude: My Headphones ParametricEq.txt\nInclude: game.txt\n";
        assert!(includes_game_txt(cfg));
        assert!(includes_game_txt("include:GAME.TXT"));
        assert!(includes_game_txt("Include: a.txt; game.txt"));
        assert!(!includes_game_txt("# Include: game.txt\nInclude: other.txt"));
        assert!(!includes_game_txt("Include: mygame.txt"));
    }
}
