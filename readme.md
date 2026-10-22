# spliceclaw

[![CI](https://github.com/eiei114/spliceclaw/actions/workflows/ci.yml/badge.svg)](https://github.com/eiei114/spliceclaw/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

自分の Firefox に入っている Splice セッションで、Sounds のサンプルを探す CLI。Splice 非公式。Splice とは無関係。

cookie の `_splice_token_prod` だけを読み取り専用で使う。値は表示も保存もしない。Keychain と `key4.db` は解かない。

## コマンド

```powershell
spliceclaw doctor
spliceclaw auth status
spliceclaw auth status --live
spliceclaw auth refresh --live
spliceclaw search footstep --live
spliceclaw link <catalog-uuid> --live --preview
spliceclaw link <catalog-uuid> --live --confirm
spliceclaw download <catalog-uuid> --live --confirm
```

`search` は購入しない。結果の `previewUrl` は試聴用で、クレジットは減らない。

`link` はファイルを保存せず URL だけ出す。`download` はその URL を `splice-downloads` に保存する。未購入の本ファイルは `--confirm` があるときだけ 1 クレジットで購入する。`--preview` は試聴 MP3 だけで、購入しない。

既定の種類は one-shot。ループも含めるときは `--category any`。

## Firefox セッションの更新（macOS）

```sh
spliceclaw auth refresh --live
# 手動ログインに時間が必要なとき。1〜300秒、既定20秒。
spliceclaw --profile default-release auth refresh --live --wait-seconds 60
```

Firefox を起動して `https://splice.com/sounds` を開き、指定秒数のあと通常終了させ、Cookie DB のロックが解けてからアカウントを1回確認する。Splice にログイン済みならページを開くことでセッションを更新できる。未ログイン・再認証・2FA が必要な場合は、待ち時間中に自分で操作する。ログインの成功は保証せず、終了後に期限切れ・未ログインならエラーを返す。

- `--live` は必須。認証確認だけでサンプルの購入・クレジット消費はしない。
- 起動前から Firefox が動いている場合は、何も変更せず拒否する。先に `⌘Q` で終了する。
- 既存のプロファイルを使う。複数の既定プロファイルがある場合は `--profile` で明示する。
- コマンドが管理する Firefox だけを通常終了する。実行中は別の Firefox ウィンドウを開かない。別インスタンスを検出した場合や終了に失敗した場合は、強制終了せず手動終了を案内する。
- macOS が Firefox の操作許可を求めた場合は、内容を確認して許可する。許可しなければエラーになる。
- Ctrl+C でコマンドを中断した場合、Firefox が残ることがある。その場合は手動終了する。
- Cookie 値・DB をコピー／書き出ししない。Firefox が自身のプロファイルへ保存したセッションを読み取り専用で使う。
- この自動起動・終了コマンドは macOS のみ。Windows / Linux では従来どおり Firefox でログイン後に閉じて `auth status --live` を実行する。

## まだやらないこと

パック一括、いいね、スタック、プレセット、他人のアカウント。

## リポジトリ

https://github.com/eiei114/spliceclaw

crates.io へは出していない。

## License

MIT OR Apache-2.0. `license-mit` と `license-apache`。
