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
- **Vue SFC 対応**: `<script>` ブロック以外を空白マスクして TypeScript 文法で
  解析する行・オフセット保存方式（ADR 0075）
- **依存関係インデックスの並列化**: 起動時のスキャンを全 CPU コアで実行
  （ADR 0076）
- **TUI の Tests セクション修正**: テストディレクトリが通常ツリーと Tests の
  両方に重複表示されるバグ、および実在する `__tests__/` ディレクトリと
  セクション内部パスが衝突するバグの修正

## 使い方（最小）

```sh
cargo build --release
./target/release/rinkaku --base main          # TUI
./target/release/rinkaku --base main --format md
```

開発時の品質ゲートは上流と同じです:

```sh
make test    # cargo test --all-features
make lint    # cargo fmt --check + clippy -D warnings
```

## ライセンス

MIT。本リポジトリは hiro-o918/rinkaku の派生物であり、原著作者の
著作権表示は [LICENSE](./LICENSE) を参照してください。
