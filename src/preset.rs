//! Community preset format and its safety check.
//!
//! A preset is a plain Equalizer APO file with `# key: value` metadata comments on top.
//! Only EQ lines are allowed: `Preamp`, `Filter` (PK/LS/HS) and `GraphicEQ`. Anything that could
//! load code or other files (`VSTPlugin`, `Include`, `Device`, `Eval`, ...) is rejected.

pub const MAGIC: &str = "# eqloadout-preset: 1";
const MAX_BYTES: usize = 8 * 1024;
const MAX_FILTERS: usize = 32;
const MAX_GAIN_DB: f64 = 12.0;
/// Preamp must cover the largest boost, allowing this much slack.
const HEADROOM_SLACK_DB: f64 = 0.5;

#[derive(Debug, Default, PartialEq)]
pub struct Preset {
    pub name: String,
    pub game: Option<String>,
    pub exes: Vec<String>,
    pub steam_appid: Option<u32>,
    pub notes: String,
}

/// Parses and validates a preset. The text must be written to disk exactly as validated.
pub fn parse(text: &str) -> Result<Preset, String> {
    if text.len() > MAX_BYTES {
        return Err(format!("larger than {MAX_BYTES} bytes"));
    }
    if let Some(c) = text.chars().find(|c| !(c.is_ascii_graphic() || *c == ' ' || *c == '\n' || *c == '\r' || *c == '\t')) {
        return Err(format!("character {c:?} is not allowed (ASCII only)"));
    }
    let mut lines = text.lines().map(str::trim);
    if lines.next() != Some(MAGIC) {
        return Err(format!("first line must be \"{MAGIC}\""));
    }

    let mut p = Preset::default();
    let (mut preamp, mut max_boost, mut filters) = (None, 0.0f64, 0);
    for (n, line) in lines.enumerate().map(|(i, l)| (i + 2, l)) {
        let err = |m: &str| format!("line {n}: {m}");
        if line.is_empty() {
            continue;
        }
        if let Some(comment) = line.strip_prefix('#') {
            if let Some((k, v)) = comment.split_once(':') {
                let v = v.trim().to_string();
                match k.trim() {
                    "name" => p.name = v,
                    "game" => p.game = (v != "(any)").then_some(v),
                    "exe" => p.exes = v.split(',').map(|e| e.trim().to_string()).filter(|e| !e.is_empty()).collect(),
                    "steam_appid" => p.steam_appid = Some(v.parse().map_err(|_| err("steam_appid must be a number"))?),
                    "notes" => p.notes = v,
                    _ => {}
                }
            }
            continue;
        }
        let (kind, rest) = line.split_once(':').ok_or_else(|| err("expected \"Preamp:\", \"Filter:\" or \"GraphicEQ:\""))?;
        match kind.trim() {
            "Preamp" => {
                if preamp.is_some() {
                    return Err(err("only one Preamp line is allowed"));
                }
                let db = parse_db(rest.trim()).ok_or_else(|| err("expected \"Preamp: -2.5 dB\""))?;
                if !(-30.0..=0.0).contains(&db) {
                    return Err(err("Preamp must be between -30 and 0 dB"));
                }
                preamp = Some(db);
            }
            "Filter" => {
                filters += 1;
                max_boost = max_boost.max(parse_filter(rest).map_err(|m| err(&m))?);
            }
            "GraphicEQ" => {
                filters += 1;
                max_boost = max_boost.max(parse_graphic_eq(rest).map_err(|m| err(&m))?);
            }
            other => return Err(err(&format!("\"{other}\" lines are not allowed in presets"))),
        }
    }

    if p.name.is_empty() {
        return Err("missing \"# name:\"".into());
    }
    if filters == 0 {
        return Err("no Filter or GraphicEQ lines".into());
    }
    if filters > MAX_FILTERS {
        return Err(format!("more than {MAX_FILTERS} filters"));
    }
    if p.exes.iter().any(|e| !e.to_ascii_lowercase().ends_with(".exe") || e.contains(['/', '\\', ':'])) {
        return Err("exe must be plain file names ending in .exe".into());
    }
    let preamp = preamp.unwrap_or(0.0);
    if preamp + max_boost > HEADROOM_SLACK_DB {
        return Err(format!("Preamp {preamp} dB does not cover the largest boost (+{max_boost} dB); it would clip"));
    }
    Ok(p)
}

/// True for names like `eql-cs2-footsteps.txt`, the form presets are installed under.
pub fn valid_file_name(name: &str) -> bool {
    name.strip_prefix("eql-")
        .and_then(|s| s.strip_suffix(".txt"))
        .is_some_and(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
}

fn parse_db(s: &str) -> Option<f64> {
    s.strip_suffix("dB")?.trim().parse().ok().filter(|v: &f64| v.is_finite())
}

/// `ON PK Fc 2000 Hz Gain 1.5 dB Q 1.00`; returns the gain.
fn parse_filter(s: &str) -> Result<f64, String> {
    let t: Vec<&str> = s.split_whitespace().collect();
    let shape = "expected \"Filter: ON PK|LS|HS Fc <Hz> Hz Gain <dB> dB Q <q>\"";
    let [on, kind, "Fc", fc, "Hz", "Gain", gain, "dB", "Q", q] = t.as_slice() else { return Err(shape.into()) };
    if *on != "ON" || !matches!(*kind, "PK" | "LS" | "HS") {
        return Err(shape.into());
    }
    let num = |v: &str| v.parse::<f64>().ok().filter(|v| v.is_finite()).ok_or_else(|| shape.to_string());
    let (fc, gain, q) = (num(fc)?, num(gain)?, num(q)?);
    check_freq(fc)?;
    check_gain(gain)?;
    if !(0.1..=10.0).contains(&q) {
        return Err("Q must be between 0.1 and 10".into());
    }
    Ok(gain)
}

/// `25 0; 40 -1.5; ...`; returns the largest gain.
fn parse_graphic_eq(s: &str) -> Result<f64, String> {
    let mut max = f64::MIN;
    for pair in s.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let shape = "expected \"GraphicEQ: <Hz> <dB>; <Hz> <dB>; ...\"";
        let mut it = pair.split_whitespace().map(|v| v.parse::<f64>().ok().filter(|v| v.is_finite()));
        let (Some(Some(f)), Some(Some(g)), None) = (it.next(), it.next(), it.next()) else { return Err(shape.into()) };
        check_freq(f)?;
        check_gain(g)?;
        max = max.max(g);
    }
    if max == f64::MIN { Err("GraphicEQ has no points".into()) } else { Ok(max) }
}

fn check_freq(f: f64) -> Result<(), String> {
    if (20.0..=20000.0).contains(&f) { Ok(()) } else { Err("frequency must be between 20 and 20000 Hz".into()) }
}

fn check_gain(g: f64) -> Result<(), String> {
    if g.abs() <= MAX_GAIN_DB { Ok(()) } else { Err(format!("gain must be within +/-{MAX_GAIN_DB} dB")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OK: &str = "# eqloadout-preset: 1\n# name: Footsteps\n# game: Counter-Strike 2\n# exe: cs2.exe\n# steam_appid: 730\n\
                      Preamp: -2.0 dB\nFilter: ON LS Fc 160 Hz Gain -2.5 dB Q 0.70\nFilter: ON PK Fc 4000 Hz Gain 2.0 dB Q 1.20\n";

    fn with(extra: &str) -> String {
        format!("{OK}{extra}\n")
    }

    #[test]
    fn accepts_valid_preset() {
        let p = parse(OK).unwrap();
        assert_eq!((p.name.as_str(), p.game.as_deref(), p.exes.as_slice(), p.steam_appid), ("Footsteps", Some("Counter-Strike 2"), &["cs2.exe".to_string()][..], Some(730)));
        assert!(parse(&with("GraphicEQ: 25 0; 1000 -1.5; 8000 2")).is_ok());
    }

    #[test]
    fn rejects_dangerous_lines() {
        for bad in [r"VSTPlugin: Library C:\evil.dll", "Include: other.txt", "Device: all", "Eval: x = 1", "Copy: L=R", "If: 1", "Channel: L"] {
            assert!(parse(&with(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn rejects_out_of_range_values() {
        for bad in [
            "Filter: ON PK Fc 10 Hz Gain 1 dB Q 1",
            "Filter: ON PK Fc 1000 Hz Gain 13 dB Q 1",
            "Filter: ON PK Fc 1000 Hz Gain 1 dB Q 0",
            "Filter: ON PK Fc NaN Hz Gain 1 dB Q 1",
            "Filter: ON LP Fc 9500 Hz",
            "Filter: OFF PK Fc 1000 Hz Gain 1 dB Q 1",
            "GraphicEQ: 1000",
        ] {
            assert!(parse(&with(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn requires_headroom_for_boosts() {
        let loud = OK.replace("Preamp: -2.0 dB", "Preamp: -0.5 dB");
        assert!(parse(&loud).unwrap_err().contains("clip"));
        assert!(parse(&OK.replace("Preamp: -2.0 dB\n", "")).is_err());
    }

    #[test]
    fn rejects_bad_headers() {
        assert!(parse(&OK.replace(MAGIC, "# something else")).is_err());
        assert!(parse(&OK.replace("# name: Footsteps\n", "")).is_err());
        assert!(parse(&OK.replace("cs2.exe", r"C:\games\cs2.exe")).is_err());
        assert!(parse(&OK.replace("Footsteps", "Füße")).is_err());
    }

    #[test]
    fn file_names() {
        assert!(valid_file_name("eql-cs2-footsteps.txt"));
        for bad in ["cs2.txt", "eql-.txt", "eql-CS2.txt", "eql-cs2.txt.exe", "eql-../x.txt"] {
            assert!(!valid_file_name(bad), "{bad}");
        }
    }

    /// Every preset shipped in the repo must pass the same check the app applies to downloads.
    #[test]
    fn repo_presets_are_valid() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("presets");
        let mut seen = std::collections::HashSet::new();
        let mut count = 0;
        for e in std::fs::read_dir(&dir).unwrap() {
            let name = e.unwrap().file_name().into_string().unwrap();
            if !name.ends_with(".txt") {
                continue;
            }
            assert!(valid_file_name(&name), "{name}: bad file name");
            let text = std::fs::read_to_string(dir.join(&name)).unwrap();
            let p = parse(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(name.starts_with("eql-genre-") || p.game.is_some(), "{name}: game presets need # game:");
            assert!(seen.insert(name.to_lowercase()), "{name}: duplicate");
            count += 1;
        }
        assert!(count > 0, "no presets found");
    }
}
