//! Localisation. The English text in the source is the key: `tr!("Settings")` shows the current
//! language's text for "Settings", or "Settings" itself when there is none. Translations live in
//! `assets/lang/<code>.json`: `{"English": "translation"}` pairs, grouped by purpose in nested
//! objects (`"lobby": {...}`, `"hud_orders": {...}`; the groups are only for the reader, a text is
//! found whatever group holds it, and a text shown in several places sits in `common`). Keys starting
//! with `_` are not texts (`_language` is the name shown in the language list).
//!
//! - `tr!("text")` -> `&str`; `tr!("{} of {}", a, b)` -> `String` (arguments are anything `Display`).
//!   Placeholders: `{}` (next argument), `{0}` (by number, for another word order), with an optional
//!   spec `{:.1}` / `{0:>4}` / `{:02}` (precision, width, alignment, zero padding).
//!   Plural forms: `{0|one|few|many}` picks a word by the number in argument 0 (English uses
//!   one/other, Ukrainian one/few/many: 1 / 2-4 / 5-20 ...); it does not print the number itself.
//! - `tx!("text")` is just the literal: it marks texts in `const` tables, translated where shown
//!   with `tr(TABLE[i])`.
//! - `tr(s)` translates a text that is not a literal (names and descriptions from the game data).
//!
//! The simulation never translates anything; only what is shown. `cargo test --lib i18n` checks
//! that every `tr!`/`tx!` text and every built-in unit name and description has a translation in
//! every language file (and no file holds texts nobody uses).
use std::collections::HashMap;
use std::fmt::Display;
use std::sync::RwLock;

struct Lang {
    code: String,
    texts: HashMap<&'static str, &'static str>,
}

static LANG: RwLock<Option<Lang>> = RwLock::new(None);

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

/// Every (English, translation) pair of a language file, the groups flattened, in file order.
/// Keys starting with `_` and values that are not text are left out.
fn flatten(v: &serde_json::Value, out: &mut Vec<(String, String)>) {
    if let Some(m) = v.as_object() {
        for (k, v) in m {
            if k.starts_with('_') {
                continue;
            }
            match v {
                serde_json::Value::String(t) => out.push((k.clone(), t.clone())),
                serde_json::Value::Object(_) => flatten(v, out),
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
    flatten(&v, &mut texts);
    Ok((name, texts))
}

/// Switches the language ("en", "uk"...; "" = `system_language`). English, an unknown code or a
/// file that does not read leave every text in English.
pub fn set_language(code: &str) {
    let code = if code.is_empty() { system_language() } else { code };
    let lang = if code == "en" {
        None
    } else {
        match read(code) {
            Ok((_, map)) => {
                let texts = map
                    .into_iter()
                    .filter(|(_, v)| !v.is_empty())
                    .map(|(k, v)| (&*Box::leak(k.into_boxed_str()), &*Box::leak(v.into_boxed_str())))
                    .collect();
                Some(Lang { code: code.to_string(), texts })
            }
            Err(e) => {
                eprintln!("language {code}: {e}");
                None
            }
        }
    };
    *LANG.write().unwrap() = lang;
}

/// The language in use ("en" when no translation is loaded).
pub fn language() -> String {
    LANG.read().unwrap().as_ref().map_or("en".into(), |l| l.code.clone())
}

/// Every language there is a file for, English first: (code, name in that language).
pub fn languages() -> Vec<(String, String)> {
    let mut out = vec![("en".to_string(), "English".to_string())];
    let mut files: Vec<_> = std::fs::read_dir(lang_dir())
        .map(|d| d.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect())
        .unwrap_or_default();
    files.sort();
    for p in files {
        let code = p.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        if let Ok((name, _)) = read(&code) {
            out.push((code, name));
        }
    }
    out
}

/// The current language's text for `s`, or `s` itself.
pub fn tr(s: &str) -> &str {
    match LANG.read().unwrap().as_ref().and_then(|l| l.texts.get(s).copied()) {
        Some(t) => t,
        None => s,
    }
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
    let code = LANG.read().unwrap().as_ref().map_or("en".to_string(), |l| l.code.clone());
    fill_in(&code, template, args)
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

/// `tr!("text")` -> `&str`, `tr!("text {}", arg, ...)` -> `String`. See `crate::i18n`.
#[macro_export]
macro_rules! tr {
    ($s:literal) => {
        $crate::i18n::tr($s)
    };
    ($s:literal, $($a:expr),+ $(,)?) => {
        $crate::i18n::fill($crate::i18n::tr($s), &[$(&$a as &dyn ::std::fmt::Display),+])
    };
}

/// Marks a text in a `const` table for translation; it is the literal itself. Show it with `tr(..)`.
#[macro_export]
macro_rules! tx {
    ($s:literal) => {
        $s
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

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

    /// Every text shown through `tr!` / `tx!`, plus the built-in game data's unit and weapon texts.
    fn keys() -> BTreeSet<String> {
        let mut keys = BTreeSet::new();
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
                        keys.insert(k);
                    }
                }
            }
        }
        let defs = crate::data::Defs::load(&crate::data::asset_root()).unwrap();
        for d in &defs.units {
            keys.insert(d.name.clone());
            if !d.desc.is_empty() {
                keys.insert(d.desc.clone());
            }
            for w in &d.weapons {
                keys.insert(w.name.clone());
            }
        }
        keys.retain(|k| !k.trim().is_empty());
        keys
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
        let mut problems = vec![];
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = e.path();
            let text = std::fs::read_to_string(&path).unwrap();
            let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let mut pairs = vec![];
            flatten(&v, &mut pairs);
            let mut map: HashMap<&str, &str> = HashMap::new();
            for (k, t) in &pairs {
                if map.insert(k, t).is_some() {
                    problems.push(format!("{name}: in two groups (keep one; texts shown in several places go in \"common\"): {}", serde_json::to_string(k).unwrap()));
                }
            }
            let missing: Vec<_> = keys.iter().filter(|k| map.get(k.as_str()).is_none_or(|v| v.is_empty())).collect();
            if !missing.is_empty() {
                problems.push(format!("{name}: {} texts without a translation (add them to the group they belong to):", missing.len()));
                for k in missing {
                    problems.push(format!("  {}: \"\",", serde_json::to_string(k).unwrap()));
                }
            }
            for (k, t) in &pairs {
                if !keys.contains(k) {
                    problems.push(format!("{name}: no longer used (remove it): {}", serde_json::to_string(k).unwrap()));
                } else if !t.is_empty() && holes(k) != holes(t) {
                    problems.push(format!("{name}: placeholders differ: {} -> {}", serde_json::to_string(k).unwrap(), serde_json::to_string(t).unwrap()));
                }
            }
        }
        assert!(problems.is_empty(), "\n{}", problems.join("\n"));
    }
}
