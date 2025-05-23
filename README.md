# json-simple-obfuscator

A tool to partially hide json value (using unsecure pseudonimize / obfuscate algo).

## Goals / Use-cases

- Hide sensitive values into samples json used for test, demo
- Hide values also when present as part of an other string
- Idempotent and constant: `apply(a.json) == apply(apply(apply(.... (apply(a.json)))))`, so it could be used as part of pre-commit hook, build stage,...
- **DO NOT** use it to encrypt secrets,...

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

1. Collect values (string or number) of "sensitive" fields.
  the "sensitive" fields are field named(in lowercase): `id`, `_id`, `*token`, `*password`, `*secret`, `user`, `*name`, `
2. For each value, compute the replacement value
    - for number, replace every digit by `1` (preserve the number of digit, dot & comma)
    - for string, replace lowercase by `a` and uppercase by `A`, digit by `1` (preserve other caracteres: )
3. Search collected values, and replace by the computed replacement into the json as text (to preserve structure, order, comment for json5/jsonc, ...)

## Possible feature (on-demand)

Feedback, PR and feature request are welcomes. By example:

- Option to provide the list of sensitive fields
- Option to exclude some field name form the sensitive pattern
- Option to provide fixed replacement (using a lookup table)
- Option to compute replacement for different alphabet, emoji, ...
- Option to use random replacement (and break the idempotency)
