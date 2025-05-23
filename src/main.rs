#![allow(clippy::missing_errors_doc)]

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
    for json_file in cli.file {
        let json_txt = std::fs::read_to_string(&json_file)?;
        let new_json = obfuscate_jsontxt(&json_txt)?;
        std::fs::write(json_file, new_json)?;
        progress.inc(1);
    }
    progress.stop(format!("Obfuscated {count} files"));
    outro("Done!")?;
    Ok(())
}

fn obfuscate_jsontxt(json_txt: &str) -> Result<String> {
    let values = collect_sensitive_values(serde_json::from_str(json_txt)?);
    let replacements = values
        .into_iter()
        .map(|value| {
            let obfuscated = obfuscate_str(&value);
            (value, obfuscated)
        })
        .collect::<Vec<(String, String)>>();
    Ok(replace_all(&replacements, json_txt))
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

    if let Value::Object(obj) = json {
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
        assert_eq!(obfuscate_jsontxt(input).unwrap(), expected);
        assert_eq!(obfuscate_jsontxt(expected).unwrap(), expected);
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
