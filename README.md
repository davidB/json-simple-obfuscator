# json-simple-obfuscator

[![CI](https://github.com/davidB/json-simple-obfuscator/actions/workflows/ci.yml/badge.svg)](https://github.com/davidB/json-simple-obfuscator/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/json-simple-obfuscator.svg)](https://crates.io/crates/json-simple-obfuscator)
[![Crates.io Downloads](https://img.shields.io/crates/d/json-simple-obfuscator.svg)](https://crates.io/crates/json-simple-obfuscator)
[![GitHub Downloads](https://img.shields.io/github/downloads/davidB/json-simple-obfuscator/total.svg)](https://github.com/davidB/json-simple-obfuscator/releases)
[![License: CC0-1.0](https://img.shields.io/badge/License-CC0_1.0-lightgrey.svg)](https://creativecommons.org/publicdomain/zero/1.0/)

A tool to partially hide json value (using unsecure pseudonimize / obfuscate algo).

## Goals / Use-cases

- Hide sensitive values into samples json used for test, demo
- Hide values also when present as part of an other string
- Idempotent and constant: `apply(a.json) == apply(apply(apply(.... (apply(a.json)))))`, so it could be used as part of pre-commit hook, build stage,...
- Injective: distinct input values produce distinct output values, preserving references and foreign-key relationships across files
- **DO NOT** use it to hide or to encrypt secrets,...

## Usage

```bash
❯ json-simple-obfuscator file1.json file2.json

◇  Collected 2464 values from 102 files in 62.7ms
│
◇  Computed 267 replacements in 2.6ms
│
◇  Obfuscated 102 files in 103.3ms
│
└  Done!
```

```bash
❯ json-simple-obfuscator -h

A tool to partially hide json value (using unsecure pseudonimize / obfuscate algo).

Usage: json-simple-obfuscator [OPTIONS] [FILE]...

Arguments:
  [FILE]...  path of files to obfuscate

Options:
  -r, --replace <REPLACE>  additional values to obfuscate (can be repeated)
  -f, --field <FIELD>      additional field names whose values are obfuscated (can be repeated, case-insensitive). Built-in sensitive fields: contains password/secret/token/phone/email; ends with name/_id/-id/Id; exact match user/login/address/id
      --no-default-fields  disable built-in sensitive field detection (combine with --field to define an exact list)
  -s, --stem <STEM>        write output to a sibling file with this stem inserted before the extension (e.g. `-s pseudo` => foo.json -> foo.pseudo.json) instead of rewriting in place. Existing destination is overwritten
  -h, --help               Print help
  -V, --version            Print version
```

By default each input file is rewritten **in place**. Pass `--stem <STEM>` to
keep the original untouched and write the obfuscated output to a sibling file
with the stem inserted before the extension:

```bash
❯ json-simple-obfuscator --stem pseudo foo.json   # writes foo.pseudo.json
```

See [`examples/data-01/`](examples/data-01/) for a runnable sample.

### Install

Download the binary from the [release page](https://github.com/davidB/json-simple-obfuscator/releases).

<details>
<summary>Via shell script</summary>

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/davidB/json-simple-obfuscator/releases/latest/download/json-simple-obfuscator-installer.sh | sh
```
</details>

<details>
<summary>Via power shell script</summary>

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/davidB/json-simple-obfuscator/releases/latest/download/json-simple-obfuscator-installer.ps1 | iex"
```
</details>

<details>
<summary>Via homebrew</summary>

```bash
brew install davidB/tap/json-simple-obfuscator
```
</details>

<details>
<summary>Via cargo</summary>

```bash
cargo install json-simple-obfuscator
```
</details>

<details>
<summary>Via mise</summary>

```toml
[tools]
"github:davidB/json-simple-obfuscator" = "latest"
```
</details>

## A simple algorithm

```json
{
    "a": "Hello",
    "id": 123456,
    "details": {
        "user": "johnD",
        "name": "John Doe",
        "url": "http://example.com/item/123456"
    }
}
```

becomes

```json
{
    "a": "Hello",
    "id": 111111,
    "details": {
        "user": "aaaaA",
        "name": "Aaaa Aaa",
        "url": "http://example.com/item/111111"
    }
}
```

1. Collect values (string or number) of "sensitive" fields across **all input files**.
   Sensitive fields are those named (in lowercase): `id`, `_id`, `*token`, `*password`, `*secret`, `user`, `*name`, `*email`, `*phone`, `login`, `address`, `*Id`
2. Sort all collected values alphabetically, then assign each a unique obfuscated replacement:
    - Base replacement: digits → `1`, lowercase → `a`, uppercase → `A` (non-alphanumeric preserved)
    - If the base is already taken, increment right-to-left within each character class (`1→2→…→9`, `a→b→…→z`, `A→B→…→Z`), carrying left on overflow; prepend a char if all positions are exhausted
    - Sorting ensures the mapping is deterministic and reproducible for the same set of inputs
3. Replace collected values in each file as text (preserves structure, order, comments for json5/jsonc, ...)

## Possible feature (on-demand)

Feedback, PR and feature request are welcomes. By example:

- ~~Option to provide the list of sensitive fields~~
- ~~Option to exclude some field name form the sensitive pattern~~
- Option to provide fixed replacement (using a lookup table)
- Option to compute replacement for different alphabet, emoji, ...
- Option to use random replacement (and break the idempotency)
