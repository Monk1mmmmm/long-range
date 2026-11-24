//! Localisation. Texts are looked up by key: `tr!("lobby.watch_instead")` shows the current
//! language's text, else the English one, else the key itself (so a missing text is easy to spot).
//! Each language is one file `assets/lang/<code>.json` (`en.json` is English, the reference every
//! other language follows); its texts are grouped by purpose in nested objects, and a key is the
//! path to its text: `{"lobby": {"watch_instead": "..."}}` -> `lobby.watch_instead`. Texts shown in
//! several screens sit in `common`. Keys starting with `_` are not texts (`_language` is the name
//! shown in the language list).
//!
//! - `tr!("key")` -> `&str`; `tr!("key", a, b)` -> `String` (arguments are anything `Display`).
//!   Placeholders in the texts: `{}` (next argument), `{0}` (by number, for another word order),
//!   with an optional spec `{:.1}` / `{0:>4}` / `{:02}` (precision, width, alignment, zero padding).
//!   Plural forms: `{0|one|few|many}` picks a word by the number in argument 0 (English uses
//!   one/other, Ukrainian one/few/many: 1 / 2-4 / 5-20 ...); it does not print the number itself.
//! - `tx!("key")` is just the literal: it marks keys in `const` tables, shown with `tr(TABLE[i])`.
//! - `tr(key)` translates a key that is not a literal.
//! - Texts from the game data keep their English in the data (presets edit it). `unit_name`,
//!   `unit_desc`, `weapon_name` look them up under `units.<id>.name` / `units.<id>.desc` /
//!   `weapons.<name as a slug>`, but only while the data's text is still the one `en.json` has: a
//!   preset that renamed a unit shows its own name. Terrain kinds, biomes and AI types work the same
//!   (`terrain.<kind id>`, `terrain.biome_<biome>`, `ai.<name>` / `ai.<name>_desc`).
//!
//! The simulation never translates anything; only what is shown. `cargo test --lib i18n` checks that
//! every `tr!`/`tx!` key and every data text is in `en.json` (data texts equal to the data), that every
//! other language has every key with the same placeholders, and that no file holds unused keys.
use std::collections::HashMap;
use std::fmt::Display;
use std::sync::{OnceLock, RwLock};

type Texts = HashMap<&'static str, &'static str>;

struct Lang {
    code: String,
    texts: Texts,
}

/// The chosen language (None: English).
static LANG: RwLock<Option<Lang>> = RwLock::new(None);
/// English, read on first use.
static EN: OnceLock<Texts> = OnceLock::new();

/// The language a fresh install starts in: Ukrainian when the system language says so, else English.
pub fn system_language() -> &'static str {
    for k in ["LC_ALL", "LC_MESSAGES", "LANG", "LANGUAGE"] {
        if let Ok(v) = std::env::var(k) {
            if v.to_lowercase().starts_with("uk") {
                return "uk";
            }
            if !v.is_empty() {
                break;
            }
        }
    }
    "en"
}

fn lang_dir() -> std::path::PathBuf {
    crate::data::asset_root().join("lang")
}

/// Every (key, text) pair of a language file in file order; groups make the dotted key.
/// Keys starting with `_` and values that are neither text nor group are left out.
fn flatten(v: &serde_json::Value, prefix: &str, out: &mut Vec<(String, String)>) {
    if let Some(m) = v.as_object() {
        for (k, v) in m {
            if k.starts_with('_') {
                continue;
            }
            let key = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
            match v {
                serde_json::Value::String(t) => out.push((key, t.clone())),
                serde_json::Value::Object(_) => flatten(v, &key, out),
                _ => {}
            }
        }
    }
}

/// Reads one language file into (language name, texts).
fn read(code: &str) -> Result<(String, Vec<(String, String)>), String> {
    let path = lang_dir().join(format!("{code}.json"));
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let name = v["_language"].as_str().map_or_else(|| code.to_string(), String::from);
    let mut texts = vec![];
    flatten(&v, "", &mut texts);
    Ok((name, texts))
}

fn load(code: &str) -> Texts {
    match read(code) {
        Ok((_, pairs)) => pairs.into_iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| (&*Box::leak(k.into_boxed_str()), &*Box::leak(v.into_boxed_str()))).collect(),
        Err(e) => {
            eprintln!("language {code}: {e}");
            Texts::new()
        }
    }
}

fn english() -> &'static Texts {
    EN.get_or_init(|| load("en"))
}

/// Switches the language ("en", "uk"...; "" = `system_language`). An unknown code or a file that
/// does not read leave every text in English.
pub fn set_language(code: &str) {
    let code = if code.is_empty() { system_language() } else { code };
    let lang = (code != "en").then(|| Lang { code: code.to_string(), texts: load(code) });
    *LANG.write().unwrap() = lang;
}

/// The language in use ("en" when no other is loaded).
pub fn language() -> String {
    LANG.read().unwrap().as_ref().map_or("en".into(), |l| l.code.clone())
}

/// Every language there is a file for, English first: (code, name in that language).
pub fn languages() -> Vec<(String, String)> {
    let mut files: Vec<_> = std::fs::read_dir(lang_dir())
        .map(|d| d.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect())
        .unwrap_or_default();
    files.sort();
    let mut out = vec![];
    for p in files {
        let code = p.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        if let Ok((name, _)) = read(&code) {
            out.push((code, name));
        }
    }
    out.sort_by_key(|(c, _)| c != "en");
    out
}

/// The text of `key` in the current language, else in English.
fn lookup(key: &str) -> Option<&'static str> {
    LANG.read().unwrap().as_ref().and_then(|l| l.texts.get(key).copied()).or_else(|| english().get(key).copied())
}

/// The current language's text for `key`, else the English one, else the key itself.
pub fn tr(key: &str) -> &str {
    lookup(key).unwrap_or(key)
}

/// A text from the game data (English, `text`): translated under `key` while `en.json` holds the
/// same English for it, else shown as the data has it.
pub fn data_text<'a>(key: &str, text: &'a str) -> &'a str {
    if english().get(key).copied() == Some(text) {
        lookup(key).unwrap_or(text)
    } else {
        text
    }
}

/// `weapons.<this>` key part of a weapon name: lower case, runs of other characters as one `_`.
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    out.trim_end_matches('_').to_string()
}

pub fn unit_name(d: &crate::data::UnitDef) -> &str {
    data_text(&format!("units.{}.name", d.id), &d.name)
}

pub fn unit_desc(d: &crate::data::UnitDef) -> &str {
    data_text(&format!("units.{}.desc", d.id), &d.desc)
}

pub fn weapon_name(w: &crate::data::WeaponDef) -> &str {
    data_text(&format!("weapons.{}", slug(&w.name)), &w.name)
}

pub fn kind_name(k: &crate::surface::Kind) -> &'static str {
    data_text(&format!("terrain.{}", k.id), k.name)
}

pub fn biome_name(b: crate::surface::Biome) -> &'static str {
    data_text(&format!("terrain.biome_{}", slug(b.name())), b.name())
}

pub fn sky_name(s: &crate::view::sky::Sky) -> &'static str {
    data_text(&format!("terrain.sky_{}", s.id), s.name)
}

pub fn ai_name(k: crate::ai::AiKind) -> &'static str {
    data_text(&format!("ai.{}", slug(k.name())), k.name())
}

pub fn ai_desc(k: crate::ai::AiKind) -> &'static str {
    data_text(&format!("ai.{}_desc", slug(k.name())), k.desc())
}

/// Plural form index of `n`: 0 one, 1 few, 2 many (English: 0 one, 2 other).
fn plural(code: &str, n: f64) -> usize {
    let whole = n.fract() == 0.0;
    let i = n.abs() as u64;
    if matches!(code, "uk" | "ru" | "be") {
        if !whole {
            1
        } else if i % 10 == 1 && i % 100 != 11 {
            0
        } else if (2..=4).contains(&(i % 10)) && !(12..=14).contains(&(i % 100)) {
            1
        } else {
            2
        }
    } else if whole && i == 1 {
        0
    } else {
        2
    }
}

/// Fills the placeholders of a (translated) template; see the module comment.
pub fn fill(template: &str, args: &[&dyn Display]) -> String {
    fill_in(&language(), template, args)
}

fn fill_in(code: &str, template: &str, args: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut next = 0;
    let mut rest = template;
    while let Some(i) = rest.find(['{', '}']) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            out.push(tail.as_bytes()[0] as char);
            rest = &tail[2..];
            continue;
        }
        if tail.starts_with('}') {
            out.push('}');
            rest = &tail[1..];
            continue;
        }
        let Some(end) = tail.find('}') else {
            out.push_str(tail);
            return out;
        };
        let inner = &tail[1..end];
        rest = &tail[end + 1..];
        // Plural: {N|one|few|many}
        if let Some((idx, forms)) = inner.split_once('|') {
            let n: f64 = idx.trim().parse::<usize>().ok().and_then(|k| args.get(k)).and_then(|a| a.to_string().trim().parse().ok()).unwrap_or(0.0);
            let forms: Vec<&str> = forms.split('|').collect();
            let k = plural(code, n).min(forms.len().saturating_sub(1));
            out.push_str(forms.get(k).copied().unwrap_or(""));
            continue;
        }
        let (idx, spec) = inner.split_once(':').unwrap_or((inner, ""));
        let k = if idx.is_empty() {
            next += 1;
            next - 1
        } else {
            idx.parse().unwrap_or(usize::MAX)
        };
        let Some(a) = args.get(k) else {
            out.push_str(&tail[..end + 1]);
            continue;
        };
        out.push_str(&format_spec(*a, spec));
    }
    out.push_str(rest);
    out
}

/// One argument with a spec `[[fill]align][0][width][.precision]`.
fn format_spec(a: &dyn Display, spec: &str) -> String {
    let mut s = spec;
    let mut fill = ' ';
    let mut align = None;
    let mut chars = s.chars();
    if let (Some(c0), Some(c1)) = (chars.next(), chars.next()) {
        if matches!(c1, '<' | '>' | '^') {
            fill = c0;
            align = Some(c1);
            s = &s[c0.len_utf8() + 1..];
        } else if matches!(c0, '<' | '>' | '^') {
            align = Some(c0);
            s = &s[1..];
        }
    } else if matches!(s, "<" | ">" | "^") {
        s = "";
    }
    let zero = s.starts_with('0') && !s.starts_with("0.") && s.len() > 1;
    if zero {
        s = &s[1..];
    }
    let (w, p) = s.split_once('.').unwrap_or((s, ""));
    let text = match p.parse::<usize>() {
        Ok(p) => format!("{:.*}", p, a),
        Err(_) => a.to_string(),
    };
    let width: usize = w.parse().unwrap_or(0);
    let len = text.chars().count();
    if len >= width {
        return text;
    }
    let pad = width - len;
    if zero {
        let (sign, digits) = if let Some(d) = text.strip_prefix('-') { ("-", d) } else { ("", text.as_str()) };
        return format!("{sign}{}{digits}", "0".repeat(pad));
    }
    let numeric = text.trim().parse::<f64>().is_ok();
    let p = |n: usize| fill.to_string().repeat(n);
    match align.unwrap_or(if numeric { '>' } else { '<' }) {
        '>' => format!("{}{text}", p(pad)),
        '^' => format!("{}{text}{}", p(pad / 2), p(pad - pad / 2)),
        _ => format!("{text}{}", p(pad)),
    }
}

/// `tr!("group.key")` -> `&str`, `tr!("group.key", arg, ...)` -> `String`. See `crate::i18n`.
#[macro_export]
macro_rules! tr {
    ($s:literal) => {
        $crate::i18n::tr($s)
    };
    ($s:literal, $($a:expr),+ $(,)?) => {
        $crate::i18n::fill($crate::i18n::tr($s), &[$(&$a as &dyn ::std::fmt::Display),+])
    };
}

/// Marks a key in a `const` table; it is the literal itself. Show it with `tr(..)`.
#[macro_export]
macro_rules! tx {
    ($s:literal) => {
        $s
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn placeholders() {
        assert_eq!(fill_in("en", "{} of {}", &[&1, &"two"]), "1 of two");
        assert_eq!(fill_in("en", "{1} {0}", &[&"a", &"b"]), "b a");
        assert_eq!(fill_in("en", "{:.1}s {:.0}", &[&2.25f32, &7.6f64]), "2.2s 8");
        assert_eq!(fill_in("en", "{:02}:{:02}", &[&5, &13]), "05:13");
        assert_eq!(fill_in("en", "[{:>4}|{:<3}]", &[&7, &"x"]), "[   7|x  ]");
        assert_eq!(fill_in("en", "{{x}} {}", &[&1]), "{x} 1");
        let uk = |n: i32| fill_in("uk", "{} {0|танк|танки|танків}", &[&n]);
        assert_eq!([uk(1), uk(3), uk(5), uk(11), uk(21), uk(22), uk(112)], ["1 танк", "3 танки", "5 танків", "11 танків", "21 танк", "22 танки", "112 танків"]);
        assert_eq!(fill_in("en", "{} {0|wreck|wrecks|wrecks}", &[&1]), "1 wreck");
        assert_eq!(fill_in("en", "{} {0|wreck|wrecks|wrecks}", &[&2]), "2 wrecks");
    }

    /// Reads one Rust string literal starting at `s[0] == '"'`; returns the value and its length.
    fn literal(s: &str) -> Option<(String, usize)> {
        let b: Vec<char> = s.chars().collect();
        if b.first() != Some(&'"') {
            return None;
        }
        let mut out = String::new();
        let mut i = 1;
        let mut bytes = 1;
        while i < b.len() {
            let c = b[i];
            bytes += c.len_utf8();
            i += 1;
            match c {
                '"' => return Some((out, bytes)),
                '\\' => {
                    let e = b[i];
                    bytes += e.len_utf8();
                    i += 1;
                    match e {
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        '0' => out.push('\0'),
                        '\\' | '"' | '\'' => out.push(e),
                        'u' => {
                            let close = b[i..].iter().position(|&c| c == '}').unwrap();
                            let hex: String = b[i + 1..i + close].iter().collect();
                            out.push(char::from_u32(u32::from_str_radix(&hex, 16).unwrap()).unwrap());
                            bytes += close + 1;
                            i += close + 1;
                        }
                        '\n' => {
                            while i < b.len() && b[i].is_whitespace() {
                                bytes += b[i].len_utf8();
                                i += 1;
                            }
                        }
                        _ => panic!("escape \\{e}"),
                    }
                }
                _ => out.push(c),
            }
        }
        None
    }

    fn rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                rust_files(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }

    /// Every key used through `tr!` / `tx!` (text unknown: None), and every data key with the
    /// English the built-in data has for it.
    fn keys() -> BTreeMap<String, Option<String>> {
        let mut keys = BTreeMap::new();
        let mut files = vec![];
        rust_files(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
        for f in files {
            if f.ends_with("i18n.rs") {
                continue;
            }
            let src = std::fs::read_to_string(&f).unwrap();
            for pat in ["tr!(", "tx!("] {
                for (at, _) in src.match_indices(pat) {
                    let rest = src[at + pat.len()..].trim_start();
                    if let Some((k, _)) = literal(rest) {
                        keys.insert(k, None);
                    }
                }
            }
        }
        let defs = crate::data::Defs::load(&crate::data::asset_root()).unwrap();
        let mut data = |k: String, t: &str| {
            keys.insert(k, Some(t.to_string()));
        };
        for d in &defs.units {
            data(format!("units.{}.name", d.id), &d.name);
            if !d.desc.is_empty() {
                data(format!("units.{}.desc", d.id), &d.desc);
            }
            for w in &d.weapons {
                data(format!("weapons.{}", slug(&w.name)), &w.name);
            }
        }
        for k in crate::surface::KINDS {
            data(format!("terrain.{}", k.id), k.name);
        }
        for b in crate::surface::BIOMES {
            data(format!("terrain.biome_{}", slug(b.name())), b.name());
        }
        for k in &crate::view::sky::SKIES {
            data(format!("terrain.sky_{}", k.id), k.name);
        }
        for k in crate::ai::AI_KINDS {
            data(format!("ai.{}", slug(k.name())), k.name());
            data(format!("ai.{}_desc", slug(k.name())), k.desc());
        }
        keys
    }

    fn pairs(path: &std::path::Path) -> Vec<(String, String)> {
        let text = std::fs::read_to_string(path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let mut out = vec![];
        flatten(&v, "", &mut out);
        out
    }

    /// Placeholders of a text: numbered ones by number, `{}` in order, plurals by their number.
    fn holes(s: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut next = 0;
        let mut rest = s.replace("{{", "").replace("}}", "");
        while let Some(i) = rest.find('{') {
            let end = rest[i..].find('}').map_or(rest.len(), |e| i + e);
            let inner = &rest[i + 1..end];
            let idx = inner.split([':', '|']).next().unwrap_or("");
            if !inner.contains('|') {
                let k = if idx.is_empty() {
                    next += 1;
                    next - 1
                } else {
                    idx.parse().unwrap_or(999)
                };
                out.insert(format!("{k}"));
            }
            rest = rest[(end + 1).min(rest.len())..].to_string();
        }
        out
    }

    #[test]
    fn translations_complete() {
        let keys = keys();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/lang");
        let q = |s: &str| serde_json::to_string(s).unwrap();
        let mut problems = vec![];
        let en: BTreeMap<String, String> = pairs(&dir.join("en.json")).into_iter().collect();
        for (k, want) in &keys {
            match (en.get(k), want) {
                (None, _) => problems.push(format!("en.json: missing {}", q(k))),
                (Some(t), Some(want)) if t != want => problems.push(format!("en.json: {} is {} in the game data (change it here and in every language)", q(k), q(want))),
                (Some(t), None) if t.is_empty() => problems.push(format!("en.json: empty {}", q(k))),
                _ => {}
            }
        }
        for k in en.keys().filter(|k| !keys.contains_key(*k)) {
            problems.push(format!("en.json: no longer used (remove it from every language): {}", q(k)));
        }
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = e.path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if name == "en.json" {
                continue;
            }
            let map: BTreeMap<String, String> = pairs(&path).into_iter().collect();
            for (k, t) in &en {
                match map.get(k) {
                    None => problems.push(format!("{name}: missing {} (English: {})", q(k), q(t))),
                    Some(v) if v.is_empty() => problems.push(format!("{name}: empty {}", q(k))),
                    Some(v) if holes(t) != holes(v) => problems.push(format!("{name}: placeholders differ from English: {} = {}", q(k), q(v))),
                    _ => {}
                }
            }
            for k in map.keys().filter(|k| !en.contains_key(*k)) {
                problems.push(format!("{name}: not in en.json: {}", q(k)));
            }
        }
        for k in keys.keys().filter(|k| !k.contains('.')) {
            problems.push(format!("key without a group: {}", q(k)));
        }
        assert!(problems.is_empty(), "\n{}", problems.join("\n"));
    }
}
