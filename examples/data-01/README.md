# Example: data-01

Demonstrates the `--stem` option, which writes the obfuscated output to a
sibling file (`input.json` -> `input.pseudo.json`) instead of rewriting the
input in place.

- `input.json` — sample input with sensitive fields (`email`, `password`,
  `token`, `secret`, `userName`, `phone`, `address`) plus non-sensitive ones.
  Note how a sensitive value reused in a free-text field (`notes`) is also
  obfuscated consistently.
- `input.pseudo.json` — committed obfuscated output.

## Regenerate

```sh
mise run example:data-01
# or:
cargo run -- --stem pseudo examples/data-01/input.json
```

The original `input.json` is left untouched; the destination is overwritten.
