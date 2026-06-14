#![allow(clippy::missing_errors_doc)]

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use cliclack::{outro, progress_bar};
use serde_json::Value;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// path of files to obfuscate
    file: Vec<PathBuf>,
}

pub fn main() -> Result<()> {
    let cli = Cli::parse();
    let count = cli.file.len() as u64;
    let progress = progress_bar(count);
    progress.start("Obfuscating files...");

    // Pass 1: read all files, collect all sensitive values globally
    let mut all_texts: Vec<(PathBuf, String)> = Vec::with_capacity(cli.file.len());
    let mut all_values: Vec<String> = Vec::new();
    for json_file in &cli.file {
        let json_txt = std::fs::read_to_string(json_file)?;
        all_values.extend(collect_sensitive_values(serde_json::from_str(&json_txt)?));
        all_texts.push((json_file.clone(), json_txt));
    }

    // Build one deterministic mapping for all files
    let mapping = build_mapping(all_values);

    // Pass 2: apply mapping to each file
    for (json_file, json_txt) in all_texts {
        let new_json = obfuscate_jsontxt(&json_txt, &mapping)?;
        std::fs::write(json_file, new_json)?;
        progress.inc(1);
    }
    progress.stop(format!("Obfuscated {count} files"));
    outro("Done!")?;
    Ok(())
}

fn build_mapping(all_values: impl IntoIterator<Item = String>) -> HashMap<String, String> {
    let mut sorted: Vec<String> = all_values.into_iter().collect();
    sorted.sort();
    sorted.dedup();
    let mut used: HashSet<String> = HashSet::new();
    let mut mapping: HashMap<String, String> = HashMap::new();
    for value in sorted {
        let mut obfuscated = obfuscate_str(&value);
        while used.contains(&obfuscated) {
            obfuscated = increment_obfuscated(&obfuscated);
        }
        used.insert(obfuscated.clone());
        mapping.insert(value, obfuscated);
    }
    mapping
}

fn obfuscate_jsontxt(json_txt: &str, mapping: &HashMap<String, String>) -> Result<String> {
    // Use local sensitive-value order (longest first) to drive replacement order
    let local_values = collect_sensitive_values(serde_json::from_str(json_txt)?);
    let replacements: Vec<(String, String)> = local_values
        .into_iter()
        .filter_map(|v| mapping.get(&v).map(|obf| (v, obf.clone())))
        .collect();
    Ok(replace_all(&replacements, json_txt))
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
fn collect_sensitive_values(json: Value) -> Vec<String> {
    let mut values = Vec::new();

    match json {
        Value::Object(obj) => {
            for (key, value) in obj {
                if is_sensitive(&key) {
                    match value {
                        Value::String(s) => values.push(s),
                        Value::Number(n) => values.push(n.to_string()),
                        _ => {}
                    }
                } else {
                    values.extend(collect_sensitive_values(value));
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                values.extend(collect_sensitive_values(item));
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

fn replace_all(replacements: &[(String, String)], json_txt: &str) -> String {
    let mut new_json = json_txt.to_string();
    for (from, to) in replacements {
        new_json = new_json.replace(from, to);
    }
    new_json
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use rstest::*;
    use similar_asserts::assert_eq;

    fn obfuscate_single(json_txt: &str) -> Result<String> {
        let values = collect_sensitive_values(serde_json::from_str(json_txt)?);
        let mapping = build_mapping(values);
        obfuscate_jsontxt(json_txt, &mapping)
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
        assert_eq!(collect_sensitive_values(serde_json::from_str(input).unwrap()), expected);
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
        assert_eq!(collect_sensitive_values(serde_json::from_str(json).unwrap()), expected);
    }

    #[rstest]
    #[case::depth_1(r#"{"id": "v1"}"#, vec!["v1"])]
    #[case::depth_2(r#"{"wrap": {"id": "v1"}}"#, vec!["v1"])]
    #[case::depth_3(r#"{"a": {"b": {"id": "v1"}}}"#, vec!["v1"])]
    fn test_collect_depths(#[case] json: &str, #[case] expected: Vec<&str>) {
        let expected: Vec<String> = expected.into_iter().map(String::from).collect();
        assert_eq!(collect_sensitive_values(serde_json::from_str(json).unwrap()), expected);
    }

    #[rstest]
    #[case::string(r#"{"id": "str-val"}"#, vec!["str-val"])]
    #[case::number(r#"{"id": 42}"#, vec!["42"])]
    #[case::object(r#"{"id": {"nested": "x"}}"#, vec![])]
    #[case::null(r#"{"id": null}"#, vec![])]
    #[case::bool(r#"{"id": true}"#, vec![])]
    fn test_collect_value_types(#[case] json: &str, #[case] expected: Vec<&str>) {
        let expected: Vec<String> = expected.into_iter().map(String::from).collect();
        assert_eq!(collect_sensitive_values(serde_json::from_str(json).unwrap()), expected);
    }

    // BUG: collect_sensitive_values doesn't recurse into arrays — these fail
    #[rstest]
    #[case::array_under_non_sensitive_key(r#"{"users": [{"login": "octocat"}]}"#, vec!["octocat"])]
    #[case::root_array(r#"[{"id": 12345}]"#, vec!["12345"])]
    #[case::depth_3_with_array(r#"{"a": {"items": [{"node_id": "abc123"}]}}"#, vec!["abc123"])]
    fn test_collect_with_arrays(#[case] json: &str, #[case] expected: Vec<&str>) {
        let expected: Vec<String> = expected.into_iter().map(String::from).collect();
        assert_eq!(collect_sensitive_values(serde_json::from_str(json).unwrap()), expected);
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
}
