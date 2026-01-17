# TUI Agent Implementation - OpenCode Rust

## 概要

TUIモードでLLMエージェントによるツール実行を可能にする実装を追加しました。TypeScript版を参考に、以下の機能を実装しています：

1. **Agenticループ** - Tool実行後にLLMにループバックし、複数ステップのタスクを実行
2. **Permission system** - Tool実行前にユーザーに許可を求めるダイアログ
3. **リアルタイムUI更新** - Tool callやResultをストリーミングで表示
4. **会話履歴の拡張** - TextだけでなくToolUseやToolResultもサポート

## 実装の詳細

### 1. データ構造の拡張

#### `DialogType` に `PermissionRequest` を追加
```rust
pub enum DialogType {
    // ... 既存の型
    PermissionRequest,  // ← 新規追加
}
```

#### `PermissionRequest` 構造体
```rust
pub struct PermissionRequest {
    pub id: String,
    pub tool_name: String,
    pub arguments: String,
    pub description: String,
}
```

#### `AppEvent` に新しいイベントを追加
```rust
enum AppEvent {
    // ... 既存のイベント
    ToolResult { id: String, output: String, is_error: bool },
    PermissionRequested(PermissionRequest),
    PermissionResponse { id: String, allow: bool, always: bool },
}
```

#### `DisplayMessage` の拡張
```rust
pub struct DisplayMessage {
    pub role: String,
    pub content: String,
    pub time_created: i64,
    pub parts: Vec<MessagePart>,  // ← 新規追加
}

pub enum MessagePart {
    Text { text: String },
    ToolCall { id: String, name: String, args: String },
    ToolResult { id: String, output: String, is_error: bool },
}
```

### 2. Agenticループの実装

#### `stream_response_agentic()` 関数
src/tui/app.rs:1425-1712

主な処理フロー：
1. 初期ユーザーメッセージから開始
2. LLMをストリーミング呼び出し
3. Tool callsを収集
4. Permission checkを実行（auto-allow/ask/deny）
5. 承認されたToolsを並列実行
6. Tool resultsを会話履歴に追加
7. finish_reasonが"stop"以外ならループ継続
8. 最大10ステップまで実行

```rust
async fn stream_response_agentic(
    provider_id: String,
    model_id: String,
    initial_prompt: String,
    event_tx: mpsc::Sender<AppEvent>,
) -> Result<()>
```

### 3. Permission Dialog UI

#### `render_permission_dialog()` 関数
src/tui/ui.rs:541-607

UI レイアウト：
```
┌─ Permission Request ─┐
│                       │
│ Tool: read            │
│                       │
│ Read file: Cargo.toml │
│ {"filePath":"..."}    │
│                       │
│ [Y] Allow once        │
│ [A] Allow always      │
│ [N] Reject            │
│                       │
│ Press Y/A/N | Esc     │
└───────────────────────┘
```

#### Dialog入力処理
src/tui/app.rs:1371-1411

- `Y` - Allow once（一度だけ許可）
- `A` - Allow always（常に許可）
- `N`/`Esc` - Reject（拒否）

### 4. Permission システム

既存の `PermissionChecker` を使用：
- `read`, `glob`, `grep` → 自動許可（Allow）
- `write`, `edit`, `bash` → 確認が必要（Ask）
- `doom_loop` → 確認が必要（Ask）

設定ファイル `opencode.json` で上書き可能：
```json
{
  "permission": {
    "read": "allow",
    "write": "ask",
    "bash": "deny"
  }
}
```

### 5. イベントフロー

```
User入力
  ↓
stream_response_agentic() スポーン
  ↓
LLM streaming開始
  ↓
StreamEvent::ToolCallStart
  → AppEvent::ToolCall
  → UI更新（"[Calling tool: read]"）
  ↓
Permission check
  → AppEvent::PermissionRequested (if Ask)
  → Dialog表示
  → ユーザー入力待ち
  → AppEvent::PermissionResponse
  ↓
Tool実行（並列）
  ↓
AppEvent::ToolResult
  → UI更新（"[Tool xxx result: OK]"）
  ↓
Tool results を会話履歴に追加
  ↓
finish_reason == "tool_calls"?
  YES → LLMに再度ストリーミング（ループ）
  NO  → AppEvent::StreamDone
```

## 現在の制限事項

### 1. Permission待機の実装が未完成
現在、Permission requestは送信されますが、応答を待たずにツールをスキップします。

**TODO:**
- Permission responseのチャネルを実装
- agentic loopで応答を待機
- タイムアウト処理

**コード位置:** src/tui/app.rs:1617-1643

```rust
// 現在の実装（簡略版）
crate::config::PermissionAction::Ask => {
    let request = PermissionRequest { ... };
    let _ = event_tx.send(AppEvent::PermissionRequested(request)).await;
    // TODO: Wait for permission response
    // For now, just skip
}
```

### 2. Doom Loop検出の改善が必要
Doom loopが検出された場合、現在はloopを停止します。

**TODO:**
- Permission dialogを表示して継続可否を確認
- 応答を待ってから継続/停止を決定

**コード位置:** src/tui/app.rs:1584-1603

### 3. 会話履歴の永続化がない
現在、TUIを閉じると会話履歴が失われます。

**TODO:**
- Session systemと統合
- データベースに会話を保存
- 再起動時に復元

### 4. MessagePartの表示が未実装
`MessagePart` enum は定義しましたが、UI rendering では使用していません。

**TODO:**
- `render_messages()` 関数を更新してpartsを表示
- Tool callsとResultsを視覚的に区別

## テスト方法

### 基本的なTool実行テスト

1. TUIを起動
```bash
cd opencode-rs
./target/release/opencode tui
```

2. モデルを選択（例：anthropic/claude-3-5-sonnet-20241022）

3. read tool（auto-allow）をテスト
```
Read the file Cargo.toml and tell me the package name
```

期待される動作：
- LLMがread toolを呼び出す
- Permission確認なし（auto-allow）
- ファイル内容を読み取る
- LLMが応答を生成

4. write tool（ask permission）をテスト
```
Create a file test.txt with content "Hello World"
```

期待される動作：
- LLMがwrite toolを呼び出す
- Permission dialogが表示される
- Y/A/Nで応答
- Allowした場合、ファイルが作成される

### Agenticループのテスト

複数ステップのタスク：
```
Read Cargo.toml, find all dependencies, and create a file deps.txt listing them
```

期待される動作：
1. read toolでCargo.tomlを読む
2. 依存関係を解析
3. write toolでdeps.txtを作成（Permission確認）
4. 完了メッセージを表示

## TS版との違い

### 実装済み
- ✅ Agenticループ（最大10ステップ）
- ✅ Tool execution（並列実行）
- ✅ Permission dialog UI
- ✅ Doom loop検出
- ✅ イベントベースUI更新

### 未実装/簡略化
- ❌ Permission応答の非同期待機（現在はスキップ）
- ❌ Session/Message永続化
- ❌ Message Partの詳細表示
- ❌ Context overflow / Compaction
- ❌ Subtask (Task tool) サポート
- ❌ リアルタイムPart更新（TS版のようなbinary search）

## 次のステップ

### 優先度: 高
1. **Permission応答待機の実装**
   - tokio::sync::oneshot または mpscチャネル
   - agenticループでの待機処理
   
2. **MessagePartの表示**
   - UI rendering更新
   - Tool callsとResultsの視覚的表示

### 優先度: 中
3. **Session統合**
   - 会話履歴の永続化
   - Session list表示

4. **Error handling改善**
   - Tool実行エラーの詳細表示
   - Retry機能

### 優先度: 低
5. **Context compaction**
   - 長い会話の自動要約
   - トークン数管理

6. **Advanced features**
   - Subtask support
   - Custom agents
   - Tool result filtering

## 参考ファイル

### 主な変更ファイル
- `src/tui/app.rs` - Agentic loop, Permission dialog, Events
- `src/tui/ui.rs` - Permission dialog UI rendering
- `src/permission.rs` - Permission checker（既存、TUIで再利用）
- `src/tool/executor.rs` - Tool execution logic（既存、TUIで再利用）

### TypeScript版参考実装
- `opencode-ts/packages/opencode/src/session/prompt.ts` - Agentic loop
- `opencode-ts/packages/opencode/src/cli/cmd/tui/routes/session/permission.tsx` - Permission UI
- `opencode-ts/packages/opencode/src/cli/cmd/tui/context/sync.tsx` - Event synchronization

## まとめ

TUIモードでの基本的なAgent/Tool実行機能を実装しました。Permission dialogやAgentic loopなど、OpenCodeの核心機能が動作します。

ただし、Permission応答の非同期待機など、いくつかの機能は簡略化されているため、実用には追加の実装が必要です。

現在の実装でも、read/glob/grepなどの安全なツールは自動実行されるため、基本的なコード探索やファイル操作は可能です。
