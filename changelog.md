# Changelog

## Unreleased

- Accept optional `SPLICECLAW_TOKEN` as a durable session fallback when Firefox cookies are unavailable (bird-style env transport). Token values are still never printed. `--profile` keeps the Firefox-only path.

## 0.1.0

- Search your own Splice Sounds samples from the Firefox session on Windows and macOS.
- `spliceclaw search` returns one-shot samples and preview links without spending credits.
- `spliceclaw link` prints one URL and does not save a file.
- `spliceclaw download` saves one audio file.
- An unlicensed source file spends one credit only when `--confirm` is set.
- Cookie values and signed URLs are not written to the repository.
- crates.io publication is off.
