#![allow(clippy::missing_errors_doc)]

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use aho_corasick::{AhoCorasick, Input, MatchKind};
use anyhow::Result;
use clap::Parser;
use cliclack::{outro, progress_bar};
use serde_json::Value;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// path of files to obfuscate
    file: Vec<PathBuf>,

    /// additional values to obfuscate (can be repeated)
    #[arg(long, short = 'r')]
    replace: Vec<String>,

    /// additional field names whose values are obfuscated (can be repeated, case-insensitive).
    /// Built-in sensitive fields: contains password/secret/token/phone/email;
    /// ends with name/_id/-id/Id; exact match user/login/address/id.
    #[arg(long, short = 'f')]
    field: Vec<String>,

    /// disable built-in sensitive field detection (combine with --field to define an exact list)
    #[arg(long)]
    no_default_fields: bool,
}

pub fn main() -> Result<()> {
    let cli = Cli::parse();
    let count = cli.file.len() as u64;

    let extra_fields: HashSet<String> = cli.field.iter().map(|s| s.to_lowercase()).collect();
    let use_default = !cli.no_default_fields;

    // Step 1: read all files, collect all sensitive values globally
    let t = std::time::Instant::now();
    let bar = progress_bar(count);
    bar.start("Collecting values...");
    let mut all_texts: Vec<(PathBuf, String)> = Vec::with_capacity(cli.file.len());
    let mut all_values: Vec<String> = Vec::new();
    for json_file in &cli.file {
        let json_txt = std::fs::read_to_string(json_file)?;
        all_values.extend(collect_sensitive_values(
            serde_json::from_str(&json_txt)?,
            &extra_fields,
            use_default,
        ));
        all_texts.push((json_file.clone(), json_txt));
        bar.inc(1);
    }
    // Inject CLI-supplied values before building mapping
    all_values.extend(cli.replace.iter().cloned());
    let n_values = all_values.len();
    bar.stop(format!("Collected {n_values} values from {count} files in {:.1?}", t.elapsed()));

    // Step 2: build one deterministic mapping for all collected values
    let t = std::time::Instant::now();
    all_values.sort();
    all_values.dedup();
    let bar = progress_bar(all_values.len() as u64);
    bar.start("Computing replacements...");
    let mapping = build_mapping(all_values, || bar.inc(1));
    bar.stop(format!("Computed {} replacements in {:.1?}", mapping.len(), t.elapsed()));

    // Step 3: apply mapping to each file
    let t = std::time::Instant::now();
    let bar = progress_bar(count);
    bar.start("Obfuscating files...");
    // Build the matcher once and reuse across all files.
    let replacer = Replacer::new(&mapping)?;
    for (json_file, json_txt) in all_texts {
        let new_json = replacer.replace(&json_txt);
        std::fs::write(json_file, new_json)?;
        bar.inc(1);
    }
    bar.stop(format!("Obfuscated {count} files in {:.1?}", t.elapsed()));

    outro("Done!")?;
    Ok(())
}

/// Build the deterministic original→obfuscated mapping.
/// `sorted_unique` must already be sorted and deduplicated (the caller owns this
/// so it can size a progress bar). `on_value` is called once per processed value.
fn build_mapping(
    sorted_unique: Vec<String>,
    mut on_value: impl FnMut(),
) -> HashMap<String, String> {
    let mut used: HashSet<String> = HashSet::new();
    let mut mapping: HashMap<String, String> = HashMap::new();
    for value in sorted_unique {
        let mut obfuscated = obfuscate_str(&value);
        while used.contains(&obfuscated) {
            obfuscated = increment_obfuscated(&obfuscated);
        }
        used.insert(obfuscated.clone());
        mapping.insert(value, obfuscated);
        on_value();
    }
    mapping
}

#[cfg(test)]
fn obfuscate_jsontxt(json_txt: &str, mapping: &HashMap<String, String>) -> String {
    // Build a throwaway matcher; the multi-file path uses `Replacer::new` directly
    // and reuses one matcher across files.
    Replacer::new(mapping).map_or_else(|_| json_txt.to_string(), |r| r.replace(json_txt))
}

fn increment_obfuscated(s: &str) -> String {
    let mut chars: Vec<char> = s.chars().collect();
    let mut i = chars.len();
    loop {
        if i == 0 {
            // Carry overflow: prepend a char matching the first alphanumeric class
            let prefix = chars.iter().find(|c| c.is_alphanumeric()).map_or('1', |c| {
                if c.is_ascii_digit() {
                    '1'
                } else if c.is_ascii_uppercase() {
                    'A'
                } else {
                    'a'
                }
            });
            chars.insert(0, prefix);
            break;
        }
        i -= 1;
        let c = chars[i];
        if c.is_ascii_digit() {
            if c < '9' {
                chars[i] = (c as u8 + 1) as char;
                break;
            }
            chars[i] = '1'; // wrap, carry
        } else if c.is_ascii_lowercase() {
            if c < 'z' {
                chars[i] = (c as u8 + 1) as char;
                break;
            }
            chars[i] = 'a'; // wrap, carry
        } else if c.is_ascii_uppercase() {
            if c < 'Z' {
                chars[i] = (c as u8 + 1) as char;
                break;
            }
            chars[i] = 'A'; // wrap, carry
        }
        // non-alphanumeric: skip, carry continues left
    }
    chars.into_iter().collect()
}

fn obfuscate_str(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_digit() {
                '1'
            } else if c.is_alphabetic() {
                if c.is_ascii_uppercase() { 'A' } else { 'a' }
            } else {
                c
            }
        })
        .collect::<String>()
}

/// Collect all values that are sensitive (password, secret,...)
/// by length so that during replacement, the longest first (in case of overlap)
fn collect_sensitive_values(
    json: Value,
    extra_fields: &HashSet<String>,
    use_default: bool,
) -> Vec<String> {
    let mut values = Vec::new();

    match json {
        Value::Object(obj) => {
            for (key, value) in obj {
                let sensitive = (use_default && is_sensitive(&key))
                    || extra_fields.contains(&key.to_lowercase());
                if sensitive {
                    match value {
                        Value::String(s) => values.push(s),
                        Value::Number(n) => values.push(n.to_string()),
                        _ => {}
                    }
                } else {
                    values.extend(collect_sensitive_values(value, extra_fields, use_default));
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                values.extend(collect_sensitive_values(item, extra_fields, use_default));
            }
        }
        _ => {}
    }
    values.sort_by_key(|s| std::cmp::Reverse(s.len()));
    values
}

fn is_sensitive(key: &str) -> bool {
    let lower = key.to_lowercase();
    lower.contains("password")
        || lower.contains("secret")
        || lower.contains("token")
        || lower.contains("phone")
        || lower.ends_with("name")
        || (lower == "user")
        || (lower == "login")
        || (lower == "address")
        || lower.contains("email")
        || (lower == "id")
        || lower.ends_with("_id")
        || lower.ends_with("-id")
        || key.ends_with("Id")
}

fn is_pure_numeric(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn is_numeric_boundary_prefix(b: u8) -> bool {
    b == b'"' || b == b':' || b == b'/' || b.is_ascii_whitespace()
}

fn is_numeric_boundary_suffix(b: u8) -> bool {
    b == b'"' || b == b',' || b == b'/' || b == b'}' || b == b']' || b.is_ascii_whitespace()
}

fn is_word_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Per-pattern boundary rule + replacement text, indexed by Aho-Corasick pattern id.
struct ReplMeta {
    to: String,
    is_numeric: bool,
    starts_word: bool,
    ends_word: bool,
}

/// Single-pass, boundary-aware replacer.
///
/// Builds one Aho-Corasick automaton (leftmost-longest) over all `from` keys and
/// reuses it across every file, so the cost is `~ files × file_size` instead of
/// `files × unique_values × file_size`. The numeric / word boundary rules from
/// `replace_bounded_number` / `replace_whole_word` are applied at each candidate
/// match before it is accepted.
struct Replacer {
    ac: AhoCorasick,
    meta: Vec<ReplMeta>,
}

impl Replacer {
    fn new(mapping: &HashMap<String, String>) -> Result<Self> {
        let mut froms: Vec<String> = Vec::with_capacity(mapping.len());
        let mut meta: Vec<ReplMeta> = Vec::with_capacity(mapping.len());
        for (from, to) in mapping {
            // Empty keys would match everywhere; skip them.
            if from.is_empty() {
                continue;
            }
            let bytes = from.as_bytes();
            meta.push(ReplMeta {
                to: to.clone(),
                is_numeric: is_pure_numeric(from),
                starts_word: bytes.first().is_some_and(|&b| is_word_char(b)),
                ends_word: bytes.last().is_some_and(|&b| is_word_char(b)),
            });
            froms.push(from.clone());
        }
        // Leftmost-longest mirrors the old "longest first" ordering and picks the
        // longest key at each position.
        let ac = AhoCorasick::builder().match_kind(MatchKind::LeftmostLongest).build(&froms)?;
        Ok(Self { ac, meta })
    }

    fn boundary_ok(&self, pid: usize, bytes: &[u8], start: usize, end: usize) -> bool {
        let m = &self.meta[pid];
        if m.is_numeric {
            let prefix_ok = start == 0 || is_numeric_boundary_prefix(bytes[start - 1]);
            let suffix_ok = end >= bytes.len() || is_numeric_boundary_suffix(bytes[end]);
            prefix_ok && suffix_ok
        } else {
            let prefix_ok = !m.starts_word || start == 0 || !is_word_char(bytes[start - 1]);
            let suffix_ok = !m.ends_word || end >= bytes.len() || !is_word_char(bytes[end]);
            prefix_ok && suffix_ok
        }
    }

    /// Find the next accepted replacement at or after `from`: the leftmost start
    /// having any boundary-valid match, choosing the longest valid key at that
    /// start. Failed starts (longest match rejected, no shorter key valid) are
    /// skipped, mirroring the old chained "longest first then shorter" behaviour.
    fn find_replacement(
        &self,
        text: &str,
        bytes: &[u8],
        mut from: usize,
    ) -> Option<(usize, usize, usize)> {
        let len = text.len();
        while from < len {
            let mut win_end = len;
            let mut leftmost_start = None;
            while let Some(m) = self.ac.find(Input::new(text).span(from..win_end)) {
                let (start, end, pid) = (m.start(), m.end(), m.pattern().as_usize());
                leftmost_start = Some(start);
                if self.boundary_ok(pid, bytes, start, end) {
                    return Some((start, end, pid));
                }
                // Longest match here was rejected; shrink the window to try shorter
                // keys at the same start.
                win_end = end - 1;
                if win_end <= start {
                    break;
                }
            }
            match leftmost_start {
                Some(start) => from = start + 1,
                None => return None,
            }
        }
        None
    }

    fn replace(&self, text: &str) -> String {
        let bytes = text.as_bytes();
        let mut result = String::with_capacity(text.len());
        let mut cursor = 0;
        while let Some((start, end, pid)) = self.find_replacement(text, bytes, cursor) {
            result.push_str(&text[cursor..start]);
            result.push_str(&self.meta[pid].to);
            cursor = end;
        }
        result.push_str(&text[cursor..]);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use rstest::*;
    use similar_asserts::assert_eq;

    fn obfuscate_single(json_txt: &str) -> Result<String> {
        let mut values =
            collect_sensitive_values(serde_json::from_str(json_txt)?, &HashSet::new(), true);
        values.sort();
        values.dedup();
        let mapping = build_mapping(values, || {});
        Ok(obfuscate_jsontxt(json_txt, &mapping))
    }

    fn obfuscate_with_extra(json_txt: &str, extra: &[&str]) -> Result<String> {
        let mut values =
            collect_sensitive_values(serde_json::from_str(json_txt)?, &HashSet::new(), true);
        values.extend(extra.iter().map(std::string::ToString::to_string));
        values.sort();
        values.dedup();
        let mapping = build_mapping(values, || {});
        Ok(obfuscate_jsontxt(json_txt, &mapping))
    }

    #[rstest]
    fn test_collect_sensitive_values() {
        let input = indoc! {r#"
            {
                "a": "Hello",
                "id": 123456,
                "details": {
                    "user": "johnD",
                    "firstName": "John",
                    "fullName": "John Doe",
                    "url": "http://example.com/item/123456",
                    "welcome": "Welcome John Doe"
                }
            }
        "#};
        let expected = vec![
            "John Doe".to_string(),
            "123456".to_string(),
            "johnD".to_string(),
            "John".to_string(),
        ];
        assert_eq!(
            collect_sensitive_values(serde_json::from_str(input).unwrap(), &HashSet::new(), true),
            expected
        );
    }

    #[rstest]
    #[case::contains_password(r#"{"password_hash": "s3cr3t"}"#, vec!["s3cr3t"])]
    #[case::contains_secret(r#"{"client_secret": "abc"}"#, vec!["abc"])]
    #[case::contains_token(r#"{"access_token": "tok123"}"#, vec!["tok123"])]
    #[case::contains_phone(r#"{"phone_number": "555-1234"}"#, vec!["555-1234"])]
    #[case::ends_with_name(r#"{"firstName": "John"}"#, vec!["John"])]
    #[case::exact_user(r#"{"user": "alice"}"#, vec!["alice"])]
    #[case::exact_login(r#"{"login": "alice"}"#, vec!["alice"])]
    #[case::exact_address(r#"{"address": "123 Main St"}"#, vec!["123 Main St"])]
    #[case::contains_email(r#"{"email": "a@b.com"}"#, vec!["a@b.com"])]
    #[case::exact_id(r#"{"id": 42}"#, vec!["42"])]
    #[case::ends_with_underscore_id(r#"{"node_id": "abc123"}"#, vec!["abc123"])]
    #[case::ends_with_dash_id(r#"{"project-id": "p1"}"#, vec!["p1"])]
    #[case::ends_with_camel_id(r#"{"userId": "u1"}"#, vec!["u1"])]
    fn test_collect_field_patterns(#[case] json: &str, #[case] expected: Vec<&str>) {
        let expected: Vec<String> = expected.into_iter().map(String::from).collect();
        assert_eq!(
            collect_sensitive_values(serde_json::from_str(json).unwrap(), &HashSet::new(), true),
            expected
        );
    }

    #[rstest]
    #[case::depth_1(r#"{"id": "v1"}"#, vec!["v1"])]
    #[case::depth_2(r#"{"wrap": {"id": "v1"}}"#, vec!["v1"])]
    #[case::depth_3(r#"{"a": {"b": {"id": "v1"}}}"#, vec!["v1"])]
    fn test_collect_depths(#[case] json: &str, #[case] expected: Vec<&str>) {
        let expected: Vec<String> = expected.into_iter().map(String::from).collect();
        assert_eq!(
            collect_sensitive_values(serde_json::from_str(json).unwrap(), &HashSet::new(), true),
            expected
        );
    }

    #[rstest]
    #[case::string(r#"{"id": "str-val"}"#, vec!["str-val"])]
    #[case::number(r#"{"id": 42}"#, vec!["42"])]
    #[case::object(r#"{"id": {"nested": "x"}}"#, vec![])]
    #[case::null(r#"{"id": null}"#, vec![])]
    #[case::bool(r#"{"id": true}"#, vec![])]
    fn test_collect_value_types(#[case] json: &str, #[case] expected: Vec<&str>) {
        let expected: Vec<String> = expected.into_iter().map(String::from).collect();
        assert_eq!(
            collect_sensitive_values(serde_json::from_str(json).unwrap(), &HashSet::new(), true),
            expected
        );
    }

    // BUG: collect_sensitive_values doesn't recurse into arrays — these fail
    #[rstest]
    #[case::array_under_non_sensitive_key(r#"{"users": [{"login": "octocat"}]}"#, vec!["octocat"])]
    #[case::root_array(r#"[{"id": 12345}]"#, vec!["12345"])]
    #[case::depth_3_with_array(r#"{"a": {"items": [{"node_id": "abc123"}]}}"#, vec!["abc123"])]
    fn test_collect_with_arrays(#[case] json: &str, #[case] expected: Vec<&str>) {
        let expected: Vec<String> = expected.into_iter().map(String::from).collect();
        assert_eq!(
            collect_sensitive_values(serde_json::from_str(json).unwrap(), &HashSet::new(), true),
            expected
        );
    }

    // BUG: values inside array elements are not collected → not replaced in url fields
    #[rstest]
    fn test_obfuscation_in_array() {
        let input = indoc! {r#"
            {
                "users": [
                    {
                        "id": 12345,
                        "node_id": "MDQ6VXNlcjEyMzQ1",
                        "login": "octocat",
                        "url": "https://api.github.com/users/octocat",
                        "repos_url": "https://api.github.com/users/octocat/repos"
                    }
                ]
            }
        "#};
        let expected = indoc! {r#"
            {
                "users": [
                    {
                        "id": 11111,
                        "node_id": "AAA1AAAaaaAaAaA1",
                        "login": "aaaaaaa",
                        "url": "https://api.github.com/users/aaaaaaa",
                        "repos_url": "https://api.github.com/users/aaaaaaa/repos"
                    }
                ]
            }
        "#};
        assert_eq!(obfuscate_single(input).unwrap(), expected);
    }

    #[rstest]
    fn test_sample() {
        let input = indoc! {r#"
            {
                "a": "Hello",
                "id": 123456,
                "details": {
                    "user": "johnD",
                    "firstName": "John",
                    "fullName": "John Doe",
                    "url": "http://example.com/item/123456",
                    "welcome": "Welcome John Doe"
                }
            }
        "#};
        let expected = indoc! {r#"
            {
                "a": "Hello",
                "id": 111111,
                "details": {
                    "user": "aaaaA",
                    "firstName": "Aaaa",
                    "fullName": "Aaaa Aaa",
                    "url": "http://example.com/item/111111",
                    "welcome": "Welcome Aaaa Aaa"
                }
            }
        "#};
        assert_eq!(obfuscate_single(input).unwrap(), expected);
        assert_eq!(obfuscate_single(expected).unwrap(), expected);
    }

    #[rstest]
    fn test_no_collision() {
        let input = indoc! {r#"
            {
                "user": "johnD",
                "login": "janeD"
            }
        "#};
        let result = obfuscate_single(input).unwrap();
        // Both have shape "aaaaA" — must get distinct outputs
        let v1_start = result.find("\"user\": \"").unwrap() + 9;
        let v1_end = result[v1_start..].find('"').unwrap() + v1_start;
        let v2_start = result.find("\"login\": \"").unwrap() + 10;
        let v2_end = result[v2_start..].find('"').unwrap() + v2_start;
        assert_ne!(&result[v1_start..v1_end], &result[v2_start..v2_end]);
    }

    #[rstest]
    #[case::non_sensitive_key(r#"{"foo": "my-secret"}"#, &["my-secret"], r#"{"foo": "aa-aaaaaa"}"#)]
    #[case::mixed_auto_and_extra(
        r#"{"user": "alice", "note": "call alice later"}"#,
        &["call alice later"],
        r#"{"user": "aaaaa", "note": "aaaa aaaaa aaaaa"}"#
    )]
    #[case::extra_already_collected(
        r#"{"user": "alice"}"#,
        &["alice"],
        r#"{"user": "aaaaa"}"#
    )]
    fn test_replace_cli_values(#[case] json: &str, #[case] extra: &[&str], #[case] expected: &str) {
        assert_eq!(obfuscate_with_extra(json, extra).unwrap(), expected);
    }

    #[rstest]
    #[case("111", "112")]
    #[case("119", "121")]
    #[case("999", "1111")]
    #[case("zzz", "aaaa")]
    #[case("ZZZ", "AAAA")]
    #[case("11.9", "12.1")]
    fn test_increment_obfuscated(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(increment_obfuscated(input), expected);
    }

    #[rstest]
    #[case("123456", "111111")]
    #[case("12.3456", "11.1111")]
    #[case("123456.789", "111111.111")]
    #[case("123,456,789", "111,111,111")]
    #[case("123 456 789", "111 111 111")]
    #[case("johnD", "aaaaA")]
    #[case("John Doe", "Aaaa Aaa")]
    #[case("John99", "Aaaa11")]
    fn test_obfuscate_str(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(obfuscate_str(input), expected);
    }

    #[rstest]
    fn test_extra_field_collected() {
        let json = r#"{"myCustomField": "secret-val", "other": "plain"}"#;
        let extra: HashSet<String> = ["mycustomfield".to_string()].into();
        let values = collect_sensitive_values(serde_json::from_str(json).unwrap(), &extra, true);
        assert_eq!(values, vec!["secret-val".to_string()]);
    }

    #[rstest]
    fn test_no_default_fields_only_extra() {
        let json = r#"{"password": "should-stay", "myfield": "obfuscated"}"#;
        let extra: HashSet<String> = ["myfield".to_string()].into();
        let values = collect_sensitive_values(serde_json::from_str(json).unwrap(), &extra, false);
        assert_eq!(values, vec!["obfuscated".to_string()]);
    }

    #[rstest]
    fn test_no_default_fields_no_extra() {
        let json = r#"{"password": "s3cr3t", "user": "alice"}"#;
        let values =
            collect_sensitive_values(serde_json::from_str(json).unwrap(), &HashSet::new(), false);
        assert!(values.is_empty());
    }

    // Numeric boundary: "32" from id must not corrupt a longer number like "70733220"
    #[rstest]
    fn test_numeric_no_corrupt_longer_number() {
        let input = r#"{"id": 32, "resourceVersion": "70733220"}"#;
        let result = obfuscate_single(input).unwrap();
        assert!(
            result.contains("\"resourceVersion\": \"70733220\""),
            "resourceVersion corrupted: {result}"
        );
        assert!(result.contains("\"id\": 11"), "id not obfuscated: {result}");
    }

    // Numeric boundary: "30" from id must not corrupt ISO datetime "10:30:00" (colon is not allowed suffix)
    #[rstest]
    fn test_numeric_no_corrupt_datetime() {
        let input = r#"{"id": 30, "timestamp": "2024-01-15T10:30:00Z"}"#;
        let result = obfuscate_single(input).unwrap();
        assert!(
            result.contains("\"timestamp\": \"2024-01-15T10:30:00Z\""),
            "timestamp corrupted: {result}"
        );
    }

    // Whole-word: collected "app" must not corrupt "application"
    #[rstest]
    fn test_string_no_partial_word_match() {
        let input = r#"{"name": "app", "description": "application info"}"#;
        let result = obfuscate_single(input).unwrap();
        assert!(
            result.contains("\"description\": \"application info\""),
            "description corrupted: {result}"
        );
    }

    // Whole-word: collected login name must still replace inside URL path
    #[rstest]
    fn test_string_replaces_in_url_context() {
        let input = r#"{"login": "alice", "url": "https://api.example.com/users/alice"}"#;
        let result = obfuscate_single(input).unwrap();
        assert!(!result.contains("/users/alice"), "alice not replaced in URL: {result}");
        assert!(result.contains("https://api.example.com/users/"), "URL base corrupted: {result}");
    }

    /// Run the production `Replacer` with a single `from`→`to` entry.
    fn replace_one(text: &str, from: &str, to: &str) -> String {
        let mapping = HashMap::from([(from.to_string(), to.to_string())]);
        Replacer::new(&mapping).unwrap().replace(text)
    }

    // Numeric boundary contract, exercised through the single-pass `Replacer`.
    #[rstest]
    #[case("12345", "12345", "11111", true)] // exact standalone number — must replace
    #[case("70733220", "32", "11", false)] // "32" inside longer number — must NOT replace
    #[case("10:30:00", "30", "11", false)] // "30" inside datetime component — suffix ":" not allowed
    #[case(" 32 ", "32", "11", true)] // standalone number in text — must replace
    #[case("\"32\"", "32", "11", true)] // JSON-quoted number — must replace
    #[case(":32,", "32", "11", true)] // compact JSON number — must replace
    fn test_replacer_bounded_number(
        #[case] text: &str,
        #[case] from: &str,
        #[case] to: &str,
        #[case] replaced: bool,
    ) {
        let result = replace_one(text, from, to);
        if replaced {
            assert!(result.contains(to), "expected replacement in {text:?}: got {result:?}");
            assert!(!result.contains(from), "original not removed in {text:?}: got {result:?}");
        } else {
            assert!(result.contains(from), "unexpected replacement in {text:?}: got {result:?}");
        }
    }

    // Word boundary contract, exercised through the single-pass `Replacer`.
    #[rstest]
    #[case("application", "app", "xyz", false)] // "app" inside word — must NOT replace
    #[case("my app here", "app", "xyz", true)] // standalone word — must replace
    #[case("/users/alice", "alice", "bob", true)] // in URL path — must replace
    #[case("\"alice\"", "alice", "bob", true)] // JSON-quoted string — must replace
    fn test_replacer_whole_word(
        #[case] text: &str,
        #[case] from: &str,
        #[case] to: &str,
        #[case] replaced: bool,
    ) {
        let result = replace_one(text, from, to);
        if replaced {
            assert!(result.contains(to), "expected replacement in {text:?}: got {result:?}");
        } else {
            assert!(result.contains(from), "unexpected replacement in {text:?}: got {result:?}");
        }
    }

    // Multi-key single pass: longest-first wins, and a shorter key still applies
    // where the longer one is boundary-rejected.
    #[rstest]
    fn test_replacer_leftmost_longest() {
        let mapping = HashMap::from([
            ("john".to_string(), "XXXX".to_string()),
            ("john.doe@x.com".to_string(), "YYYY".to_string()),
        ]);
        let replacer = Replacer::new(&mapping).unwrap();
        // Standalone long value: longest key wins.
        assert_eq!(replacer.replace("\"john.doe@x.com\""), "\"YYYY\"");
        // Long key boundary-rejected (suffix word char) → shorter key applies.
        assert_eq!(replacer.replace("john.doe@x.comextra"), "XXXX.doe@x.comextra");
    }
}
