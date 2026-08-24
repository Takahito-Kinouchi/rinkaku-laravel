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

## 画面上の記号

TUI 上のマーカー / バッジの一覧です。実行中に `?` を押すと、ヘルプオーバーレイの
「マーカー」欄でも同様の凡例を確認できます。

### ツリーペイン（エントリービュー）

| 記号 | 色 | 意味 |
| --- | --- | --- |
| `v` / `>` | — | 展開マーカー: 子要素の表示 / 非表示（空欄はリーフで展開対象なし） |
| `fn` `struct` `enum` `trait` `class` `iface` `type` `block` | — | シンボル行の種別プレフィックス（各言語のキーワードを略記） |
| `+` | 緑 | 追加されたシンボル |
| `~` | 黄 | シグネチャが変更されたシンボル |
| `x` | 赤 | 削除されたシンボル |
| （マーカー無し・空白 1 桁） | 灰 | body のみの変更、未分類、またはテストのシンボル。名前も灰色で表示され、存在はするがレビュー上の重要度は低いことを示す |
| 灰色＋取り消し線の名前 | 灰 | 削除されたシンボルの名前 |
| `!` | 赤（太字） | リスクマーカー: 同じサブツリー内に契約変更と高 fan-in シンボルが共存 |
| `(cycle)` | 黄 | ディレクトリ内に依存の循環がある |
| `[test] (N symbols)` | マゼンタ | ファイル全体がテストであることを示すバッジ |
| `N tests` | 灰 | ファイル内のテストシンボルをまとめた折りたたみグループ |
| `(skipped: ...)` | 灰 | ファイルが解析されなかった理由（未対応言語、バイナリ、生成物、削除済み） |

### バッジ（件数）

| バッジ | 色 | 意味 |
| --- | --- | --- |
| `chg:N` | シアン | このサブツリー内の変更済み（削除以外）シンボル数 |
| `api:N` | 黄 | このサブツリー内の契約変更: シグネチャ変更シンボルと削除シンボルの合計 |
| `fan-in:N` | シアン | このサブツリー内の高 fan-in シンボルの used_by 数の合計 |
| `lines:N` | band 色 | このファイル自体の行数。file-size band で色分け（normal: 既定色 / watch: 黄 / warn: 赤 / split: 赤・太字） |
| `warn:N` | 黄 | ディレクトリ行: このサブツリー内の Warn band ファイル数 |
| `split:N` | 赤 | ディレクトリ行: このサブツリー内の Split band ファイル数 |
| `tests:0` | 黄 | シンボル行: このシンボルを参照するテストシンボルが 0 件 |
| `ann:N` | シアン | この行に紐づくレビューアノテーションの件数 |

`api:` と `warn:` / `split:` / `tests:0` が黄・赤なのは「確認が必要」を示す
ためで、シアンのバッジは単なる情報量です。

### Diff ペイン / Detail ペイン

| 記号 | 色 | 意味 |
| --- | --- | --- |
| `+` / `-` | 緑 / 赤 | 追加行 / 削除行（Detail ペインのシグネチャ差分でも同じ） |
| `@@ ...` | 灰 | hunk ヘッダ |
| 行頭の `*` | シアン | その行にレビューアノテーションが付いている（unified / split の新側のみ） |

### Blast radius ペイン

| 記号 | 意味 |
| --- | --- |
| `! ... — already shown above (cycle)` | 依存の循環でツリーを打ち切った位置（既出のノードを指している） |

## ライセンス

MIT。本リポジトリは hiro-o918/rinkaku の派生物であり、原著作者の
著作権表示は [LICENSE](./LICENSE) を参照してください。
