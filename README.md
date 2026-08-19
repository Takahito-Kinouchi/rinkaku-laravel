# rinkaku-laravel

[hiro-o918/rinkaku](https://github.com/hiro-o918/rinkaku) のフォーク。
PHP / Laravel / Vue のコードベースをレビューするためのカスタマイズを加えています。

rinkaku は PR の diff を「変更されたシンボルのシグネチャと依存関係」に凝縮して
表示する CLI / TUI ツールです。仕組み・操作方法・詳細なドキュメントは上流の
[README](https://github.com/hiro-o918/rinkaku#readme) と
[docs/](https://github.com/hiro-o918/rinkaku/tree/main/docs) を参照してください。

## このフォークの変更点

- **PHP 対応**: 関数・メソッド・class / interface / trait / enum の抽出、
  PHPUnit 規約（`tests/`, `Tests/`, `*Test.php`）でのテスト判定（ADR 0075）
- **Vue SFC / Svelte 対応**: `<script>` ブロック以外を空白マスクして
  TypeScript 文法で解析する行・オフセット保存方式（ADR 0075）。Inertia
  (Vue) と Svelte のフロントエンドを同じ機構でカバー
- **依存関係インデックスの並列化**: 起動時のスキャンを全 CPU コアで実行
  （ADR 0076）
- **依存関係スキャンの範囲限定と高速化**: モノレポでは変更があった
  プロジェクト（`composer.json` 等のマニフェストを持つディレクトリ）
  配下だけをスキャン（`--deps-scope`、既定で有効）。対応言語のファイル
  以外は読み込み自体を省略し、読み込み中も件数を表示（ADR 0078）

## インストール

```sh
cargo install --git https://github.com/Takahito-Kinouchi/rinkaku-laravel rinkaku-laravel
```

コマンド名は `rinkaku-laravel`（上流の `rinkaku` とは別名 — ADR 0083）。
自動アップデート機能は持たない（本フォークに GitHub Releases が無いため）ので、
更新は `cargo install` の再実行で行う。以前アップストリーム版の `rinkaku` を
インストールしていた場合は `cargo uninstall rinkaku` で削除しておくこと。

## 使い方（最小）

```sh
cargo build --release
./target/release/rinkaku-laravel --base main          # TUI
./target/release/rinkaku-laravel --base main --format md
```

開発時の品質ゲートは上流と同じです:

```sh
make test    # cargo test --all-features
make lint    # cargo fmt --check + clippy -D warnings
```

## ライセンス

MIT。本リポジトリは hiro-o918/rinkaku の派生物であり、原著作者の
著作権表示は [LICENSE](./LICENSE) を参照してください。
