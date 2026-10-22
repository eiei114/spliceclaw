# Changelog

## 0.1.1

- Add `auth refresh --live` on macOS: open Splice in the selected Firefox profile, wait, quit normally, then verify the account without purchasing.
- Refuse to take over an already-running Firefox; never force-kill the browser or copy its cookies.
- Allow `--wait-seconds 1..300` for manual sign-in, with a default of 20 seconds.

## 0.1.0

- Search your own Splice Sounds samples from the Firefox session on Windows and macOS.
- `spliceclaw search` returns one-shot samples and preview links without spending credits.
- `spliceclaw link` prints one URL and does not save a file.
- `spliceclaw download` saves one audio file.
- An unlicensed source file spends one credit only when `--confirm` is set.
- Cookie values and signed URLs are not written to the repository.
- crates.io publication is off.
