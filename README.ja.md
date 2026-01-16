# opencode-rs

AI駆動型開発ツールのRust実装

**⚡ クイックスタート**: 3ステップで始めるには[QUICKSTART.md](QUICKSTART.md)をご覧ください！

## 概要

opencode-rsは[opencode-ts](https://github.com/anomalyco/opencode)のRust移植版で、複数のLLMプロバイダーに対応した高速で効率的なCLIツールを提供します。

## 特徴

- 🚀 **高速・効率的**: Rustで書かれたパフォーマンスの高い実装
- 🤖 **複数のLLMプロバイダー**: Anthropic、OpenAIなどに対応
- 🛠️ **ツール統合**: ファイル操作、コード検索、シェルコマンドなどの組み込みツール
- 💬 **インタラクティブTUI**: チャットベースの開発を行うターミナルUI
- 📝 **CLIモード**: スクリプト用の非インタラクティブプロンプトモード
- ⚙️ **設定可能**: JSONベースの設定と環境変数のサポート

## インストール

### ソースからビルド

```bash
git clone https://github.com/your-repo/opencode-rs
cd opencode-rs
cargo build --release
```

バイナリは`target/release/opencode`に生成されます。

### PATHに追加

```bash
# シェルプロファイル（~/.bashrc、~/.zshrcなど）に追加
export PATH="$PATH:/path/to/opencode-rs/target/release"
```

## クイックスタート

### 1. 設定の初期化

```bash
opencode config init
```

これにより、`~/.config/opencode/opencode.json`にデフォルトの設定ファイルが作成されます。

### 2. APIキーの設定

設定ファイルを編集してAPIキーを追加します：

```json
{
  "provider": {
    "anthropic": {
      "key": "$ANTHROPIC_API_KEY"
    },
    "openai": {
      "key": "$OPENAI_API_KEY"
    }
  },
  "model": "anthropic/claude-3-5-sonnet-20241022"
}
```

`$`プレフィックスを使用することで、設定内で環境変数を直接使用できます。

### 3. 環境変数の設定

```bash
export ANTHROPIC_API_KEY="your-api-key-here"
# または
export OPENAI_API_KEY="your-api-key-here"
```

## 使い方

### インタラクティブTUIモード

インタラクティブなターミナルセッションを開始：

```bash
opencode
# または
opencode run
```

これにより、AIアシスタントとチャットできるフルスクリーンのTUIが起動します。

**重要**: TUIモードはインタラクティブターミナル（TTY）が必要です。つまり：
- ✅ 動作する環境: ターミナルエミュレータ（iTerm2、Terminal.app、GNOME Terminalなど）
- ❌ 動作しない環境: CI/CDパイプライン、パイプ、バックグラウンドジョブ、一部の環境での`cargo run`
- 🔧 代替手段: 非インタラクティブな使用には`prompt`コマンドを使用

`cargo run`でTTYエラーが出る場合は、コンパイル済みバイナリを直接実行してください：
```bash
cargo build --release
./target/release/opencode
```

### プロンプトモード（非インタラクティブ）

TUIなしで単一のプロンプトを送信：

```bash
opencode prompt "このコードを説明して" --model anthropic/claude-3-5-sonnet-20241022
```

オプション:
- `--model, -m`: 使用するモデルを指定（形式: `provider/model`）
- `--format`: 出力形式（`text`、`json`、`markdown`）

例:

```bash
# シンプルなプロンプト
opencode prompt "このディレクトリにあるファイルは何ですか？"

# 特定のモデルを指定
opencode prompt "このコードをレビューして" -m openai/gpt-4

# JSON出力
opencode prompt "すべてのTypeScriptファイルをリスト" --format json

# Markdown出力
opencode prompt "アーキテクチャを説明して" --format markdown
```

### 設定管理

```bash
# 現在の設定を表示
opencode config show

# 設定ファイルのパスを表示
opencode config path

# デフォルト設定で初期化
opencode config init
```

### セッション管理

```bash
# すべてのセッションをリスト
opencode session list

# セッション詳細を表示
opencode session show <session-id>

# セッションを削除
opencode session delete <session-id>
```

## 設定

設定は以下から読み込まれます：
1. グローバル設定: `~/.config/opencode/opencode.json`
2. プロジェクト設定: `./opencode.json`または`./.opencode/opencode.jsonc`
3. 環境変数

### 設定ファイル形式

```json
{
  "$schema": "https://opencode.ai/schema/config.json",
  "theme": "dark",
  "model": "anthropic/claude-3-5-sonnet-20241022",
  "small_model": "anthropic/claude-3-haiku-20240307",
  "provider": {
    "anthropic": {
      "key": "$ANTHROPIC_API_KEY"
    },
    "openai": {
      "key": "$OPENAI_API_KEY",
      "base_url": "https://api.openai.com/v1"
    }
  },
  "server": {
    "port": 19876,
    "hostname": "127.0.0.1"
  },
  "tui": {
    "scroll_speed": 3.0
  }
}
```

### 環境変数

- `ANTHROPIC_API_KEY`: Anthropic APIキー
- `OPENAI_API_KEY`: OpenAI APIキー
- `OPENCODE_MODEL`: デフォルトモデルを上書き
- `OPENCODE_THEME`: テーマを上書き（dark/light）
- `OPENCODE_LOG_LEVEL`: ログレベルを設定（debug/info/warn/error）

## 利用可能なツール

opencode-rsには、AIが使用できる組み込みツールが含まれています：

- **read**: ファイル内容の読み取り
- **write**: ファイルの作成または上書き
- **edit**: 正確な文字列置換によるファイル編集
- **bash**: シェルコマンドの実行
- **glob**: パターンによるファイル検索
- **grep**: ファイル内容の検索
- **todo**: タスクリスト管理

## テーマ

テーマは`themes/`ディレクトリにあります：
- `deltarune.json`: Dark World風テーマ
- `undertale.json`: Underground風テーマ

themesディレクトリにJSONファイルを作成することで、カスタムテーマを追加できます。

## プロジェクト構造

```
opencode-rs/
├── src/
│   ├── main.rs          # CLIエントリーポイント
│   ├── config.rs        # 設定管理
│   ├── provider/        # LLMプロバイダー統合
│   ├── session/         # セッション管理
│   ├── storage/         # データ永続化
│   ├── tool/            # 組み込みツール
│   ├── tui/             # ターミナルUI
│   └── cli/             # CLIコマンド
├── themes/              # カラーテーマ
├── .opencode/           # 設定例
│   ├── agent/           # エージェント例
│   ├── command/         # コマンド例
│   └── opencode.jsonc   # 設定例
├── Cargo.toml           # Rust依存関係
└── README.md            # このファイル
```

## 開発

### ビルド

```bash
cargo build
```

### テストの実行

```bash
cargo test
```

### デバッグログ付き実行

```bash
RUST_LOG=debug cargo run
```

## トラブルシューティング

### "No such device or address"エラー

このエラーは非インタラクティブモード（例：パイプ入力）で実行した場合に発生します。`prompt`コマンドを使用してください：

```bash
# これの代わりに:
echo "hello" | opencode

# これを使用:
opencode prompt "hello"
```

### モデルが設定されていない

"No default model configured"と表示される場合：
1. 設定を初期化: `opencode config init`
2. 設定ファイルにAPIキーを設定
3. 設定ファイルの`model`フィールドを設定

### APIキーが見つからない

APIキーは以下のいずれかで設定してください：
- 設定ファイル内（`$ENV_VAR`構文が使用可能）
- 環境変数として
- 両方の方法を組み合わせて使用可能

## opencode-tsとの比較

opencode-rsはopencode-tsとの互換性を目指しつつ、以下を提供します：
- より高速な起動と実行
- より低いメモリ使用量
- シングルバイナリ配布
- ファイル操作のネイティブパフォーマンス

注意: opencode-tsの一部の機能はまだ完全に実装されていない場合があります。これは進行中のプロジェクトです。

## ライセンス

MIT

## クレジット

Opencodeチームによる[opencode](https://github.com/anomalyco/opencode)をベースにしています。
