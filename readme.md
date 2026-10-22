# spliceclaw

[![CI](https://github.com/eiei114/spliceclaw/actions/workflows/ci.yml/badge.svg)](https://github.com/eiei114/spliceclaw/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

自分の Firefox に入っている Splice セッションで、Sounds のサンプルを探す CLI。Splice 非公式。Splice とは無関係。

cookie の `_splice_token_prod` だけを読み取り専用で使う。値は表示しない。Keychain と `key4.db` は解かない。

Firefox が無い／読めない環境向けに、環境変数 `SPLICECLAW_TOKEN`（同じ JWT）も使える。CLI 引数での cookie 渡しはしない。`--profile` を付けると Firefox を強制し、env は無視する。

## コマンド

```powershell
spliceclaw doctor
spliceclaw auth status
spliceclaw auth status --live
spliceclaw search footstep --live
spliceclaw link <catalog-uuid> --live --preview
spliceclaw link <catalog-uuid> --live --confirm
spliceclaw download <catalog-uuid> --live --confirm
```

`search` は購入しない。結果の `previewUrl` は試聴用で、クレジットは減らない。

`link` はファイルを保存せず URL だけ出す。`download` はその URL を `splice-downloads` に保存する。未購入の本ファイルは `--confirm` があるときだけ 1 クレジットで購入する。`--preview` は試聴 MP3 だけで、購入しない。

既定の種類は one-shot。ループも含めるときは `--category any`。

## まだやらないこと

パック一括、いいね、スタック、プレセット、他人のアカウント。

## リポジトリ

https://github.com/eiei114/spliceclaw

crates.io へは出していない。

## License

MIT OR Apache-2.0. `license-mit` と `license-apache`。
