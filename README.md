# rinkaku-laravel

**English** · [日本語](#rinkaku-laravel-日本語)

A fork of [hiro-o918/rinkaku](https://github.com/hiro-o918/rinkaku), customized
for reviewing PHP / Laravel / Vue codebases.

rinkaku condenses a pull request's diff into **the signatures of the changed
symbols and their dependencies**, as an interactive TUI or as Markdown/JSON
meant to be fed to an LLM — so you can see the shape of a change before reading
every line of it. For the mechanism and the upstream documentation, see
upstream's [README](https://github.com/hiro-o918/rinkaku#readme) and
[docs/](https://github.com/hiro-o918/rinkaku/tree/main/docs).

## What this fork adds

- **PHP support** — functions, methods, `class` / `interface` / `trait` /
  `enum`, with PHPUnit conventions (`tests/`, `Tests/`, `*Test.php`) deciding
  what counts as a test (ADR 0075).
- **Vue SFC and Svelte support** — everything outside `<script>` is masked to
  spaces and the rest is parsed as TypeScript, preserving every line and byte
  offset, so an Inertia (Vue) or Svelte frontend is covered by the same
  machinery (ADR 0075). A component's declared interface is part of that
  surface: `defineProps` / `defineEmits` / `defineModel` / `defineSlots` /
  `defineExpose`, the Options API's `props:` / `emits:`, Svelte's `export let`
  and `$props()` all extract as symbols, so adding a required prop reads as
  `api props — signature changed` rather than as a line count (ADR 0097).
  The component itself is a symbol too, named after its file, whose
  dependencies are the children its markup renders — read by scanning the
  markup, so a Nuxt auto-imported `<BaseButton>` that never appears in the
  script still resolves (ADR 0098).
- **Markdown support** — a document's headings become its outline: each
  section is a symbol whose signature is its heading and whose container is
  the heading above it, so a docs change shows *which sections* moved rather
  than a line count (ADR 0096).
- **Parallel dependency index** — the startup scan runs across all CPU cores
  (ADR 0076).
- **Scoped dependency scan** — in a monorepo, only the project(s) actually
  touched by the diff are scanned, a project being the nearest directory with a
  manifest (`composer.json`, `package.json`, `Cargo.toml`). Files in unsupported
  languages are never read at all (`--deps-scope`, on by default — ADR 0078).
- **Hardening for untrusted diffs** — a diff written by whoever opened the PR is
  treated as hostile input: paths that leave the repository are refused (ADR
  0090) and terminal control sequences in rendered output are escaped (ADR
  0091). See [SECURITY.md](SECURITY.md).

## Install

```sh
cargo install --git https://github.com/Takahito-Kinouchi/rinkaku-laravel rinkaku-laravel
```

The command is `rinkaku-laravel`, deliberately not upstream's `rinkaku`, so the
two can coexist on one machine (ADR 0083). There is no self-update: re-run
`cargo install` to update, adding `--force` when you want to be certain the
binary was rebuilt. If you previously installed upstream's binary, remove it
with `cargo uninstall rinkaku`.

## Usage

Four ways to give rinkaku something to look at:

```sh
rinkaku-laravel --base main                      # diff local refs
rinkaku-laravel --pr 76                           # a PR, from inside its clone
rinkaku-laravel --pr https://github.com/o/r/pull/76   # a PR, from anywhere
gh pr diff 123 | rinkaku-laravel                  # a diff on stdin
rinkaku-laravel                                   # no input: whole-repo outline
```

`--pr` needs [`gh`](https://cli.github.com/) installed and authenticated; a bare
number resolves against the current clone, while a URL auto-clones into a cache
so it works from any directory.

The output stage is separate from the input:

```sh
rinkaku-laravel --base main                # TUI when stdout is a terminal
rinkaku-laravel --base main --format md    # Markdown (LLM-facing default)
rinkaku-laravel --base main --format json  # JSON, for tooling
rinkaku-laravel --base main --format digest   # one line per API change
rinkaku-laravel --base main --format mermaid  # graph for a PR comment
```

Useful flags: `--deps 0` skips dependency resolution entirely (faster),
`--exclude-tests` moves test symbols into a summary, `--entry <path>` re-roots
the change graph at a path, and `--deps-scope repo` widens the scan back to
every tracked file. `rinkaku-laravel --help` documents the rest.

### Comparing local work against what you have already pushed

"Already pushed" is the remote-tracking branch `origin/<your branch>`, so that
is the ref to compare against. Fetch first — a tracking branch is a local
cache, no fresher than your last fetch.

```sh
git fetch origin my-branch
rinkaku-laravel --base origin/my-branch    # outline of the unpushed commits
git log --oneline origin/my-branch..HEAD   # which commits are unpushed
git diff origin/my-branch                  # unpushed commits + uncommitted edits
```

`--base` runs `git diff <base>...<head>` — three dots, comparing from the merge
base of the two refs, the same view GitHub's compare page and a PR's "Files
changed" tab show. Two dots (`git diff a..b`) compare the two refs directly
instead. Both sides of `--base` are commits, so work you have not committed yet
is outside the comparison: commit or stash it first, or read it with plain
`git diff`.

The same comparison in a GUI:

| Tool | Where to find it |
| --- | --- |
| GitHub (web) | A PR's **Files changed → Changes from** menu shows only what arrived after a chosen commit. Unpushed work never appears here — it is not on the server yet |
| VS Code | Source Control: **Changes** is uncommitted work, and the Sync / Incoming-Outgoing section lists unpushed commits. With GitLens: Search & Compare → **Compare References** → `origin/<branch>` against `HEAD` |
| GitHub Desktop | **Current Branch → Choose a branch to compare with → `origin/<branch>`**. In History, unpushed commits carry an upload arrow |
| JetBrains IDEs | Git tool window → Log → right-click the `origin/<branch>` label → **Compare with Local** |

## Versioning

`<upstream version>+laravel.<n>` (ADR 0089). The part before `+` is the
upstream [hiro-o918/rinkaku](https://github.com/hiro-o918/rinkaku) release this
fork is built on; `<n>` counts the fork's own releases on top of it.

```sh
rinkaku-laravel --version
# rinkaku-laravel 0.6.22+laravel.2 (fork of hiro-o918/rinkaku)
#                 ^^^^^^ upstream base  ^ fork release
```

[`docs/UPSTREAM.md`](docs/UPSTREAM.md) records the exact upstream commit this
fork sits on, and how to take upstream changes in. Upstream's `CHANGELOG.md`
files are frozen at the fork point; this fork's history lives in
[`docs/adr/`](docs/adr) and in its pull requests.

## TUI reference

Press `?` in the TUI for the same keymap, marker legend, and glossary, filtered
to what is actually pressable on the current screen.

<details>
<summary><b>On-screen markers</b></summary>

### Tree pane (entry view)

| Marker | Color | Meaning |
| --- | --- | --- |
| `v` / `>` | — | Expanded / collapsed; blank means a leaf with nothing to expand |
| `fn` `struct` `enum` `trait` `class` `iface` `type` `block` `section` | — | Symbol kind prefix, abbreviating each language's keyword |
| `+` | green | Added symbol |
| `~` | yellow | Symbol whose signature changed |
| `x` | red | Removed symbol |
| (no marker, one blank column) | grey | Body-only change, unclassified, or a test symbol — the name is grey too: present, but low review priority |
| Grey struck-through name | grey | The name of a removed symbol |
| `!` | red, bold | Risk marker: a contract change and a high fan-in symbol coexist in this subtree |
| `(cycle)` | yellow | The directory contains a dependency cycle |
| `[test] (N symbols)` | magenta | The whole file is tests |
| `N tests` | grey | Collapsed group of the file's test symbols |
| `(skipped: ...)` | grey | Why the file was not analyzed (unsupported language, binary, generated, deleted, outside the repository) |

### Count badges

| Badge | Color | Meaning |
| --- | --- | --- |
| `chg:N` | cyan | Changed (non-removed) symbols in this subtree |
| `api:N` | yellow | Contract changes in this subtree: signature-changed plus removed symbols |
| `fan-in:N` | cyan | Total `used_by` count of the high fan-in symbols in this subtree |
| `lines:N` | band color | This file's own line count, colored by file-size band (normal: default / watch: yellow / warn: red / split: red bold) |
| `warn:N` | yellow | Directory row: Warn-band files in this subtree |
| `split:N` | red | Directory row: Split-band files in this subtree |
| `tests:0` | yellow | Symbol row: no test symbol references this symbol |
| `ann:N` | cyan | Review annotations attached to this row |

`api:`, `warn:`, `split:` and `tests:0` are yellow or red because they mean
"look at this"; cyan badges are information, not a warning.

### Diff and Detail panes

| Marker | Color | Meaning |
| --- | --- | --- |
| `+` / `-` | green / red | Added / removed line (also in the Detail pane's signature diff) |
| `@@ ...` | grey | Hunk header |
| `*` at line start | cyan | This line carries a review annotation (new side only, in both unified and split views) |
| `▲N` / `▼N` in the title | yellow, bold | Lines of the selected symbol's change still above / below the viewport; `ctrl-f` / `ctrl-b` (or `↓` / `↑` with tree focus) reads through them. The title's `(12-41/210)` is file-scoped, this is symbol-scoped |

### Blast radius pane

| Marker | Meaning |
| --- | --- |
| `! ... — already shown above (cycle)` | The tree stopped here because of a dependency cycle; the node was already shown |

</details>

<details>
<summary><b>Key bindings</b></summary>

### Tree focus (left pane)

| Key | Action |
| --- | --- |
| `j` / `k` | Move the cursor |
| `ctrl-d` / `ctrl-u` | Move the cursor half a page |
| `gg` / `G` | Jump to the top / bottom of the tree |
| `ctrl-f` / `ctrl-b` / `↓` / `↑` | Read through the change one screen at a time (a symbol row reads that symbol's change, a file row the file's whole diff); once nothing is left off-screen, the cursor moves to the next / previous row |
| `/` | Start a search |
| `n` / `N` | Jump to the next / previous match |
| `enter` | Toggle a directory row, or open a file / symbol row (focus moves right) |
| `space` | Toggle a directory / file row without moving focus |
| `e` / `c` | Expand / collapse everything |

### Right pane focus (Detail / Diff / blast radius)

| Key | Action |
| --- | --- |
| `j` / `k` / `↓` / `↑` | Scroll one line |
| `ctrl-d` / `ctrl-u` | Scroll half a page |
| `gg` / `G` | Jump to the top / bottom of the pane |
| `ctrl-f` / `ctrl-b` | Read through the change one screen at a time (same as with tree focus) |
| `h` / `esc` | Return focus to the tree |
| `]` / `[` | Jump to the next / previous hunk (Diff pane only) |

### Entry screen only

| Key | Action |
| --- | --- |
| `d` | Switch the right pane between Detail and Diff |
| `r` | Switch the right pane to the selected row's blast radius |
| `o` | Toggle topological / alphabetical ordering |
| `s` | Open the source view for the symbol under the cursor |
| `ctrl-o` / `ctrl-i` | Move back / forward through the jumplist |

### Source view

| Key | Action |
| --- | --- |
| `j` / `k`, `ctrl-d` / `ctrl-u`, `gg` / `G` | Scroll, or jump to the start / end of the file |
| `/`, `n` / `N` | Search, jump to the next / previous match |
| `esc` / `q` | Return to the entry view |

### Review annotations

| Key | Action |
| --- | --- |
| `a` | Annotate the line under the cursor |
| `A` | Open the annotation list |
| In the list: `j` / `k`, `Enter`, `d`, `Esc` | Move, export menu, delete the selected one, close |

### Global

| Key | Action |
| --- | --- |
| `v` | Toggle unified / split view (both the Diff pane and the source view's diff) |
| `gd` / `gr` | Jump to the callee / caller of the symbol under the cursor |
| `w` | Open the current PR's page in a browser (`--pr` mode only) |
| `?` | Toggle the help overlay |
| `q` / `ctrl-c` | Quit (`q` asks for confirmation, `ctrl-c` does not; in the source view `esc` / `q` returns to the entry view instead) |

</details>

<details>
<summary><b>Glossary</b></summary>

| Term | Meaning |
| --- | --- |
| **fan-in** | How many *changed* symbols reference this one (`used_by`). References from tests are not counted. Two or more makes it "high fan-in", and those counts are summed into the `fan-in:N` badge on directory and file rows. The larger it is, the further a change here reaches |
| **contract change (`api:`)** | Signature-changed plus removed symbols — the changes that oblige you to check the callers |
| **blast radius** | The dependency tree rooted at the selected directory or file: what a change there can reach |
| **cycle** | Symbols that depend on each other. The tree stops at the point the node first appeared |
| **topological order** | Directories ordered by dependency, the foundational ones last |
| **alphabetical order** | A-Z, ignoring dependencies |
| **jumplist** | The history of `gd` / `gr` jumps, walked with `ctrl-o` / `ctrl-i` |
| **file-size band** | Line-count bands: ≤ 600 normal, > 600 watch, > 1000 warn, > 1500 split. Drives the color of `lines:N` and the `warn:N` / `split:N` totals on directory rows |

</details>

## Development

```sh
make test    # cargo test --all-features
make lint    # cargo fmt --check + clippy -D warnings
make audit   # cargo audit against the RustSec advisory database
make help    # list every target
```

CI runs the same commands. Project conventions — architecture, test style, ADR
requirements — are in [CLAUDE.md](CLAUDE.md); design decisions are recorded in
[`docs/adr/`](docs/adr).

## Security

rinkaku reads diffs written by other people, so [SECURITY.md](SECURITY.md)
states what it treats as untrusted, what it trusts, the known limits, and how to
report a vulnerability.

## License

MIT. This repository is derived from hiro-o918/rinkaku; see
[LICENSE](./LICENSE) for the original copyright notice.

---

# rinkaku-laravel (日本語)

[English](#rinkaku-laravel) · **日本語**

[hiro-o918/rinkaku](https://github.com/hiro-o918/rinkaku) のフォーク。
PHP / Laravel / Vue のコードベースをレビューするためのカスタマイズを加えています。

rinkaku は PR の diff を **変更されたシンボルのシグネチャと依存関係** に凝縮して
見せるツールです。対話的な TUI としても、LLM に渡す Markdown / JSON としても使え、
差分を 1 行ずつ読む前に変更の輪郭をつかめます。仕組みと詳細なドキュメントは上流の
[README](https://github.com/hiro-o918/rinkaku#readme) と
[docs/](https://github.com/hiro-o918/rinkaku/tree/main/docs) を参照してください。

## このフォークの変更点

- **PHP 対応** — 関数・メソッド・`class` / `interface` / `trait` / `enum` の抽出。
  テスト判定は PHPUnit の規約（`tests/`, `Tests/`, `*Test.php`）に従います（ADR 0075）。
- **Vue SFC / Svelte 対応** — `<script>` ブロック以外を空白でマスクし、残りを
  TypeScript 文法で解析します。行番号とバイトオフセットが保存されるため、Inertia
  (Vue) や Svelte のフロントエンドも同じ機構でカバーできます（ADR 0075）。
  コンポーネントが宣言するインターフェースも surface の一部として扱います。
  `defineProps` / `defineEmits` / `defineModel` / `defineSlots` /
  `defineExpose`、Options API の `props:` / `emits:`、Svelte の `export let` と
  `$props()` がシンボルとして抽出されるので、必須 prop の追加が行数ではなく
  `api props — signature changed` として読めます（ADR 0097）。
  コンポーネント自身もファイル名を名前に持つシンボルになり、マークアップが
  描画する子コンポーネントがその依存になります。マークアップを走査して読むので、
  script に一切現れない Nuxt の auto-import された `<BaseButton>` も解決できます
  （ADR 0098）。
- **Markdown 対応** — 見出しをドキュメントの輪郭として扱います。各セクションが
  シンボルになり、シグネチャはその見出し、コンテナは 1 つ上の見出しです。
  ドキュメントの変更が行数ではなく「どのセクションが変わったか」で見えます
  （ADR 0096）。
- **依存関係インデックスの並列化** — 起動時のスキャンを全 CPU コアで実行します
  （ADR 0076）。
- **依存関係スキャンの範囲限定** — モノレポでは、diff が実際に触れたプロジェクト
  （`composer.json` / `package.json` / `Cargo.toml` などのマニフェストを持つ最も近い
  ディレクトリ）配下だけをスキャンします。未対応言語のファイルは読み込み自体を
  省略します（`--deps-scope`、既定で有効 — ADR 0078）。
- **信頼できない diff への防御** — PR を出した人が書いた diff は敵性入力として扱います。
  リポジトリ外に出るパスは拒否し（ADR 0090）、出力に含まれる端末制御シーケンスは
  エスケープします（ADR 0091）。詳細は [SECURITY.md](SECURITY.md)。

## インストール

```sh
cargo install --git https://github.com/Takahito-Kinouchi/rinkaku-laravel rinkaku-laravel
```

コマンド名は `rinkaku-laravel` です。上流の `rinkaku` と同居できるよう、意図的に
別名にしています（ADR 0083）。自動アップデート機能は持たないので、更新は
`cargo install` の再実行で行ってください。確実に入れ直したいときは `--force` を
付けます。以前に上流版を入れていた場合は `cargo uninstall rinkaku` で削除して
おいてください。

## 使い方

入力の与え方は 4 通りあります:

```sh
rinkaku-laravel --base main                      # ローカルの ref 同士を diff
rinkaku-laravel --pr 76                           # クローン内から PR を指定
rinkaku-laravel --pr https://github.com/o/r/pull/76   # どこからでも PR を指定
gh pr diff 123 | rinkaku-laravel                  # diff を標準入力から
rinkaku-laravel                                   # 入力なし: リポジトリ全体の概要
```

`--pr` には [`gh`](https://cli.github.com/) のインストールと認証が必要です。番号
だけを渡した場合は現在のクローンに対して解決され、URL を渡した場合はキャッシュへ
自動クローンするのでどのディレクトリからでも動きます。

出力段は入力段と独立しています:

```sh
rinkaku-laravel --base main                # stdout が端末なら TUI
rinkaku-laravel --base main --format md    # Markdown（LLM 向けの既定）
rinkaku-laravel --base main --format json  # JSON（ツール連携用）
rinkaku-laravel --base main --format digest   # API 変更を 1 行ずつ
rinkaku-laravel --base main --format mermaid  # PR コメント用のグラフ
```

よく使うフラグ: `--deps 0` は依存解決を丸ごと省略して高速化、`--exclude-tests` は
テストシンボルを集計欄へ移動、`--entry <path>` は変更グラフの起点を付け替え、
`--deps-scope repo` はスキャン範囲を全追跡ファイルに戻します。残りは
`rinkaku-laravel --help` を参照してください。

### プッシュ済みの内容とローカルの作業を比べる

「プッシュ済み」の実体はリモート追跡ブランチ `origin/<ブランチ名>` なので、これを
比較の基準に指定します。追跡ブランチはローカルのキャッシュで、最後に fetch した
時点までしか新しくないため、先に fetch してください。

```sh
git fetch origin my-branch
rinkaku-laravel --base origin/my-branch    # 未プッシュのコミットの輪郭
git log --oneline origin/my-branch..HEAD   # 未プッシュのコミット一覧
git diff origin/my-branch                  # 未プッシュのコミット + 未コミットの編集
```

`--base` は `git diff <base>...<head>` を実行します。ドット 3 個なので 2 つの ref の
マージベースからの差分、つまり GitHub の compare ページや PR の "Files changed" と
同じ見え方になります。ドット 2 個（`git diff a..b`）は 2 つの ref を直接比較します。
`--base` は両側ともコミットを指すため、まだコミットしていない変更は比較の対象外です。
先にコミットまたは stash するか、素の `git diff` で確認してください。

GUI で同じ比較をする場合:

| ツール | 操作 |
| --- | --- |
| GitHub（Web） | PR の **Files changed → Changes from** メニューで、選んだコミット以降に届いた分だけを表示できます。未プッシュの作業はまだサーバー上に無いため、ここには現れません |
| VS Code | Source Control の **Changes** が未コミットの変更、Sync（Incoming/Outgoing）セクションが未プッシュのコミットです。GitLens 併用時は Search & Compare → **Compare References** → `origin/<ブランチ名>` と `HEAD` を比較 |
| GitHub Desktop | **Current Branch → Choose a branch to compare with → `origin/<ブランチ名>`**。History では未プッシュのコミットにアップロード矢印が付きます |
| JetBrains 系 IDE | Git ツールウィンドウ → Log → `origin/<ブランチ名>` のラベルを右クリック → **Compare with Local** |

## バージョン

`<上流のバージョン>+laravel.<n>` 形式です（ADR 0089）。`+` の前がこのフォークが
ベースにしている上流 [hiro-o918/rinkaku](https://github.com/hiro-o918/rinkaku) の
バージョン、`<n>` がその上に積んだフォーク側の版数です。

```sh
rinkaku-laravel --version
# rinkaku-laravel 0.6.22+laravel.2 (fork of hiro-o918/rinkaku)
#                 ^^^^^^ 上流ベース  ^ フォーク版数
```

上流のどのコミットを取り込んだかと、取り込みの手順は
[`docs/UPSTREAM.md`](docs/UPSTREAM.md) に記録しています。上流の `CHANGELOG.md` は
フォーク時点で凍結されており、フォーク側の変更履歴は [`docs/adr/`](docs/adr) と
プルリクエストにあります。

## TUI リファレンス

TUI 実行中に `?` を押すと、同じキーマップ・マーカー凡例・用語集を、その画面で
実際に押せるものだけに絞って表示できます。

<details>
<summary><b>画面上の記号</b></summary>

### ツリーペイン（エントリービュー）

| 記号 | 色 | 意味 |
| --- | --- | --- |
| `v` / `>` | — | 展開 / 折りたたみ。空欄は展開対象のないリーフ |
| `fn` `struct` `enum` `trait` `class` `iface` `type` `block` | — | シンボル行の種別プレフィックス（各言語のキーワードの略記） |
| `+` | 緑 | 追加されたシンボル |
| `~` | 黄 | シグネチャが変更されたシンボル |
| `x` | 赤 | 削除されたシンボル |
| （マーカー無し・空白 1 桁） | 灰 | body のみの変更、未分類、またはテストのシンボル。名前も灰色で、存在はするがレビュー優先度は低いことを示す |
| 灰色＋取り消し線の名前 | 灰 | 削除されたシンボルの名前 |
| `!` | 赤（太字） | リスクマーカー: 同じサブツリー内に契約変更と高 fan-in シンボルが共存 |
| `(cycle)` | 黄 | ディレクトリ内に依存の循環がある |
| `[test] (N symbols)` | マゼンタ | ファイル全体がテスト |
| `N tests` | 灰 | ファイル内のテストシンボルの折りたたみグループ |
| `(skipped: ...)` | 灰 | 解析されなかった理由（未対応言語、バイナリ、生成物、削除済み、リポジトリ外） |

### バッジ（件数）

| バッジ | 色 | 意味 |
| --- | --- | --- |
| `chg:N` | シアン | このサブツリー内の変更済み（削除以外）シンボル数 |
| `api:N` | 黄 | このサブツリー内の契約変更: シグネチャ変更シンボルと削除シンボルの合計 |
| `fan-in:N` | シアン | このサブツリー内の高 fan-in シンボルの `used_by` 合計 |
| `lines:N` | band 色 | このファイル自体の行数。file-size band で色分け（normal: 既定色 / watch: 黄 / warn: 赤 / split: 赤・太字） |
| `warn:N` | 黄 | ディレクトリ行: このサブツリー内の Warn band ファイル数 |
| `split:N` | 赤 | ディレクトリ行: このサブツリー内の Split band ファイル数 |
| `tests:0` | 黄 | シンボル行: このシンボルを参照するテストシンボルが 0 件 |
| `ann:N` | シアン | この行に紐づくレビューアノテーションの件数 |

`api:` と `warn:` / `split:` / `tests:0` が黄・赤なのは「確認が必要」を示すため、
シアンのバッジは単なる情報量です。

### Diff ペイン / Detail ペイン

| 記号 | 色 | 意味 |
| --- | --- | --- |
| `+` / `-` | 緑 / 赤 | 追加行 / 削除行（Detail ペインのシグネチャ差分も同じ） |
| `@@ ...` | 灰 | hunk ヘッダ |
| 行頭の `*` | シアン | その行にレビューアノテーションが付いている（unified / split とも新側のみ） |
| タイトルの `▲N` / `▼N` | 黄（太字） | 選択シンボルの変更のうち画面の上 / 下に残っている行数。`ctrl-f` / `ctrl-b`（ツリーフォーカス時は `↓` / `↑` でも）で読み進められる（タイトルの `(12-41/210)` はファイル全体、こちらはシンボル単位） |

### Blast radius ペイン

| 記号 | 意味 |
| --- | --- |
| `! ... — already shown above (cycle)` | 依存の循環でツリーを打ち切った位置（既出のノードを指している） |

</details>

<details>
<summary><b>キー操作</b></summary>

### ツリーフォーカス（左ペイン）

| キー | 動作 |
| --- | --- |
| `j` / `k` | カーソルを移動 |
| `ctrl-d` / `ctrl-u` | カーソルを半ページ分移動 |
| `gg` / `G` | ツリーの先頭 / 末尾へジャンプ |
| `ctrl-f` / `ctrl-b` / `↓` / `↑` | 変更を 1 画面ずつ読み進める（シンボル行ではそのシンボルの変更、ファイル行ではファイルの diff 全体）。画面外に残りが無くなると次 / 前の行へ進む |
| `/` | 検索を開始 |
| `n` / `N` | 次 / 前の検索結果へジャンプ |
| `enter` | ディレクトリ行を開閉、またはファイル / シンボル行を開く（フォーカスが右へ移動） |
| `space` | ディレクトリ / ファイル行を開閉（フォーカスは移動しない） |
| `e` / `c` | 全行を展開 / 折りたたむ |

### 右ペインフォーカス（Detail / Diff / blast radius）

| キー | 動作 |
| --- | --- |
| `j` / `k` / `↓` / `↑` | 1 行スクロール |
| `ctrl-d` / `ctrl-u` | 半ページ分スクロール |
| `gg` / `G` | 右ペインの先頭 / 末尾へジャンプ |
| `ctrl-f` / `ctrl-b` | 変更を 1 画面ずつ読み進める（ツリーフォーカス時と同じ） |
| `h` / `esc` | フォーカスをツリーに戻す |
| `]` / `[` | 次 / 前の hunk へジャンプ（Diff ペインのみ） |

### エントリー画面のみ

| キー | 動作 |
| --- | --- |
| `d` | 右ペインを Detail と Diff で切り替え |
| `r` | 右ペインを選択行の blast radius に切り替え |
| `o` | topological / alphabetical の並び順を切り替え |
| `s` | カーソル位置のシンボルのソースビューを開く |
| `ctrl-o` / `ctrl-i` | jumplist を前 / 次へ移動 |

### ソースビュー

| キー | 動作 |
| --- | --- |
| `j` / `k`、`ctrl-d` / `ctrl-u`、`gg` / `G` | スクロール / ファイル先頭・末尾へジャンプ |
| `/`、`n` / `N` | 検索、次 / 前の検索結果へジャンプ |
| `esc` / `q` | エントリービューに戻る |

### レビュー（アノテーション）

| キー | 動作 |
| --- | --- |
| `a` | カーソル位置の行にレビューアノテーションを作成 |
| `A` | アノテーション一覧を開く |
| 一覧内: `j` / `k`、`Enter`、`d`、`Esc` | カーソル移動、エクスポートメニュー、選択中を削除、閉じる |

### グローバル

| キー | 動作 |
| --- | --- |
| `v` | unified / split（左右分割）表示を切り替え（Diff ペインとソースビューの diff 両方） |
| `gd` / `gr` | カーソル位置のシンボルの callee / caller へジャンプ |
| `w` | 現在の PR のページをブラウザで開く（`--pr` モードのみ） |
| `?` | ヘルプオーバーレイを開閉 |
| `q` / `ctrl-c` | 終了（`q` は確認ポップアップ、`ctrl-c` は確認なし。ソースビューでは `esc` / `q` でエントリービューに戻る） |

</details>

<details>
<summary><b>用語</b></summary>

| 用語 | 意味 |
| --- | --- |
| **fan-in** | そのシンボルを参照している *変更されたシンボル* の数（`used_by` の件数）。テストからの参照は数えません。2 件以上で「高 fan-in」とみなされ、ディレクトリ / ファイル行の `fan-in:N` バッジに合算されます。大きいほど変更の影響が広がりやすい箇所です |
| **契約変更（`api:`）** | シグネチャが変更されたシンボルと削除されたシンボルの合計。呼び出し側の確認が必要な変更 |
| **blast radius** | 選択したディレクトリ / ファイルを起点とする依存ツリー — そこを変更したときに影響が届く範囲 |
| **cycle（循環）** | 複数のシンボルが互いに依存している状態。ツリーは最初に現れた地点を指して打ち切られます |
| **topological order** | 依存が少ないディレクトリから並べる順序（基盤となるものほど後ろ） |
| **alphabetical order** | 依存関係を無視して A-Z で並べる順序 |
| **jumplist** | `gd` / `gr` のジャンプ履歴。`ctrl-o` / `ctrl-i` で前後に移動できます |
| **file-size band** | ファイル行数の区分。600 行以下: normal / 600 超: watch / 1000 超: warn / 1500 超: split。`lines:N` の色と、ディレクトリ行の `warn:N` `split:N` の集計に使われます |

</details>

## 開発

```sh
make test    # cargo test --all-features
make lint    # cargo fmt --check + clippy -D warnings
make audit   # RustSec アドバイザリに対する cargo audit
make help    # 全ターゲットを一覧表示
```

CI も同じコマンドを実行します。アーキテクチャ・テストの書き方・ADR の要否などの
規約は [CLAUDE.md](CLAUDE.md) に、設計判断は [`docs/adr/`](docs/adr) にあります。

## セキュリティ

rinkaku は他人が書いた diff を読むツールなので、何を信頼せず何を信頼するか、
既知の制限、脆弱性の報告先を [SECURITY.md](SECURITY.md) にまとめています。

## ライセンス

MIT。本リポジトリは hiro-o918/rinkaku の派生物であり、原著作者の著作権表示は
[LICENSE](./LICENSE) を参照してください。
