# スラッシュコマンド実装ガイド

このドキュメントでは、opencode-rsにTypeScript版互換のスラッシュコマンド機能を実装した詳細を説明します。

## 概要

TypeScript版のopencode（opencode-ts）では、`.opencode/command/`ディレクトリに配置されたマークダウンファイルからスラッシュコマンドを動的に読み込む機能があります。この機能をRust版に移植しました。

## 実装されたモジュール

### 1. `slash_command/markdown.rs`

マークダウンファイルのfrontmatterをパースするモジュール。

**主な機能:**
- YAML frontmatterの抽出とパース
- `description`、`model`、`agent`、`subtask`フィールドのサポート
- マークダウン本文の抽出

**依存関係:**
- `yaml-rust2`: YAML frontmatterのパース

**使用例:**
```rust
use opencode::slash_command::markdown::parse_markdown_file;

let markdown = parse_markdown_file(Path::new("command.md")).await?;
println!("Description: {:?}", markdown.frontmatter.description);
println!("Content: {}", markdown.content);
```

### 2. `slash_command/loader.rs`

`.opencode/command/`ディレクトリからマークダウンファイルを読み込むモジュール。

**主な機能:**
- ディレクトリの再帰的スキャン（`walkdir`を使用）
- `.opencode/command/`と`.opencode/commands/`の両方をサポート
- 相対パスからコマンド名を計算（例: `nested/child.md` → `nested/child`）
- プロジェクト設定がグローバル設定を上書き

**主要関数:**
- `load_commands_from_directory()`: 単一ディレクトリからコマンドを読み込む
- `load_all_commands()`: 全ての`.opencode`ディレクトリからコマンドを読み込む
- `find_opencode_directories()`: カレントディレクトリから上位に向かって`.opencode`ディレクトリを検索

### 3. `slash_command/parser.rs`（拡張）

テンプレート変数の置換機能を拡張。

**追加機能:**
- `expand_template_async()`: 非同期テンプレート展開
  - シェルコマンド実行: `!`command``
  - ファイル参照抽出: `@filepath`
- `extract_file_references()`: テンプレートからファイル参照を抽出

**既存機能:**
- 位置引数の置換: `$1`, `$2`, `$3`, ...
- 全引数の置換: `$ARGUMENTS`
- 最後のプレースホルダーが残りの全引数を受け取る

**シェルコマンド実行の例:**
```rust
let template = "Current directory: !`pwd`";
let expanded = expand_template_async(template, &[]).await?;
// → "Current directory: /home/user/project"
```

### 4. `slash_command/template.rs`（更新）

`TemplateCommand`を更新して非同期テンプレート展開を使用。

**変更点:**
- `expand_template()`から`expand_template_async()`への切り替え
- ファイル参照の抽出とログ出力（将来の実装のため）

### 5. `tui/state.rs`（更新）

アプリケーション初期化時にマークダウンコマンドを読み込むように更新。

**変更点:**
- `init_commands()`メソッドに`load_all_commands()`の呼び出しを追加
- 読み込んだコマンドをコマンドレジストリに登録

## テンプレート変数リファレンス

### 位置引数

```markdown
Explain $1 in the context of $2
```

使用例:
```
/explain Rust "web development"
→ "Explain Rust in the context of web development"
```

### 全引数

```markdown
Process these arguments: $ARGUMENTS
```

使用例:
```
/process file1.rs file2.rs file3.rs
→ "Process these arguments: file1.rs file2.rs file3.rs"
```

### 残り引数の受け取り

最後のプレースホルダーは残りの全引数を受け取ります：

```markdown
First: $1, Rest: $2
```

使用例:
```
/cmd one two three four
→ "First: one, Rest: two three four"
```

### シェルコマンド実行

```markdown
Current branch: !`git branch --show-current`
```

使用例:
```
/status
→ "Current branch: main"
```

### ファイル参照

```markdown
Check @README.md and @src/main.rs for details
```

現在はファイルパスが抽出されるのみ（将来の実装でファイル内容が含まれる予定）。

## Frontmatterオプション

### description

コマンドの説明。ヘルプメッセージに表示されます。

```yaml
---
description: git commit and push
---
```

### model

このコマンド専用のモデルを指定します。

```yaml
---
model: opencode/glm-4.6
---
```

### agent

このコマンド専用のエージェントを指定します。

```yaml
---
agent: explorer
---
```

### subtask

サブタスクとして実行するかどうか。

```yaml
---
subtask: true
---
```

## コマンドファイルの配置

コマンドファイルは以下のディレクトリに配置できます：

1. プロジェクトローカル:
   - `./.opencode/command/*.md`
   - `./.opencode/commands/*.md`

2. グローバル:
   - `~/.config/opencode-rs/.opencode/command/*.md`
   - `~/.config/opencode-rs/.opencode/commands/*.md`

ネストされたディレクトリもサポートされます：
- `.opencode/command/git/commit.md` → コマンド名: `git/commit`

## 実装例

### シンプルなコマンド

`.opencode/command/hello.md`:
```markdown
---
description: Say hello
---

Hello! You said: $ARGUMENTS
```

使用:
```
/hello world
→ "Hello! You said: world"
```

### モデル指定コマンド

`.opencode/command/review.md`:
```markdown
---
description: Code review
model: anthropic/claude-3-5-sonnet-20241022
---

Review this code carefully:

$ARGUMENTS
```

使用:
```
/review async fn main() { ... }
```

### シェルコマンド統合

`.opencode/command/git-status.md`:
```markdown
---
description: Show git status
---

Current branch: !`git branch --show-current`
Uncommitted changes: !`git status --short`

Please help me with: $ARGUMENTS
```

## テスト

実装には包括的なテストが含まれています：

### ユニットテスト

- `slash_command::markdown`: frontmatterパースのテスト
- `slash_command::parser`: テンプレート展開のテスト
- `slash_command::loader`: コマンド読み込みのテスト

### 統合テスト

- `tests/command_loading.rs`: ディレクトリからのコマンド読み込み
- `tests/command_execution.rs`: コマンド実行とテンプレート展開
- `tests/template_expansion.rs`: 各種テンプレート変数の展開

テストの実行:
```bash
cargo test
```

特定のテストの実行:
```bash
cargo test slash_command::markdown
cargo test --test command_execution
```

## パフォーマンス考慮事項

- コマンドの読み込みはアプリケーション起動時に一度だけ実行されます
- `walkdir`による効率的なディレクトリトラバーサル
- 非同期I/Oによるファイル読み込み
- エラーが発生したファイルはスキップされ、警告がログに記録されます

## TypeScript版との互換性

この実装はTypeScript版との完全な互換性を目指しています：

### サポートされている機能

- ✅ マークダウンファイルからのコマンド読み込み
- ✅ YAML frontmatterのパース
- ✅ `description`、`model`、`agent`、`subtask`フィールド
- ✅ テンプレート変数: `$1`, `$2`, ..., `$ARGUMENTS`
- ✅ シェルコマンド実行: `!`command``
- ✅ ファイル参照: `@filepath`（抽出のみ）
- ✅ ネストされたコマンドディレクトリ
- ✅ プロジェクト設定によるグローバル設定の上書き

### 将来の改善

- ファイル参照の完全な実装（ファイル内容をメッセージに含める）
- エージェント参照のサポート
- MCP promptsとの統合

## まとめ

この実装により、opencode-rsはTypeScript版と同等のスラッシュコマンド機能を提供します。マークダウンベースの宣言的なコマンド定義により、ユーザーは簡単にカスタムコマンドを作成・共有できます。
