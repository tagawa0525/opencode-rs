# opencode-ts vs opencode-rs 機能比較

このドキュメントは、TypeScript版（opencode-ts）とRust版（opencode-rs）の機能差を記録したものです。

## 概要

| カテゴリ | opencode-ts | opencode-rs |
|---------|-------------|-------------|
| TUI | 完全実装 | 基本実装 |
| モデル選択 | ダイアログUI | CLI/設定のみ |
| 認証 | 複数方式対応 | 環境変数のみ |
| スラッシュコマンド | 完全実装 | 未実装 |
| プロバイダー | 動的同期 | 静的定義 |

---

## 1. モデル選択機能

### opencode-ts
- **ファイル**: `packages/opencode/src/cli/cmd/tui/component/dialog-model.tsx`
- ファジー検索フィルター
- お気に入りシステム（`model.json`に永続化）
- 最近使用したモデル追跡（直近10件）
- プロバイダー別グループ表示
- カテゴリセクション（お気に入り、最近、プロバイダー別）
- キーボードナビゲーション（上下、Page Up/Down）
- モデル切り替えキーバインド（F2で最近のモデルを循環）

### opencode-rs
- **ステータス**: 未実装
- モデルはCLIオプション（`--model`）または設定ファイルでのみ指定可能
- TUIにモデル選択UIなし

### 実装に必要な作業
- [ ] モデル選択ダイアログコンポーネント作成
- [ ] モデル一覧取得ロジック
- [ ] お気に入り/最近の永続化
- [ ] キーバインド接続（`Action::ModelSelector`は定義済み）

---

## 2. プロバイダー接続機能

### opencode-ts
- **ファイル**: `packages/opencode/src/cli/cmd/tui/component/dialog-provider.tsx`
- プロバイダー一覧表示（優先度順ソート）
- 「接続済み」ステータス表示
- 複数認証方式サポート:
  - APIキー入力
  - OAuthフロー（自動リダイレクト + コード入力）
  - デバイスコードフロー
- 接続成功後、自動的にモデルセレクターを表示

### opencode-rs
- **ステータス**: 未実装
- 認証は環境変数のみ対応（`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`等）
- プロバイダー接続UIなし

### 実装に必要な作業
- [ ] プロバイダー選択ダイアログ
- [ ] APIキー入力フォーム
- [ ] OAuth認証フロー（オプション）

---

## 3. 認証ストレージシステム

### opencode-ts
- **ファイル**: `packages/opencode/src/auth.ts`, `packages/opencode/src/cli/cmd/auth.ts`
- 永続ストレージ: `~/.local/share/opencode/auth.json`
- 複数認証タイプ:
  - APIキー
  - OAuthトークン（リフレッシュ対応）
  - Well-known認証（カスタムプロバイダー用）
- CLIコマンド: `opencode auth login`, `opencode auth logout`, `opencode auth list`
- カスタム認証フロー用プラグインシステム

### opencode-rs
- **ステータス**: 未実装
- 環境変数のみ対応
- 永続認証ストレージなし
- `auth`サブコマンドなし

### 実装に必要な作業
- [ ] `src/auth.rs`モジュール作成
- [ ] 認証情報の読み書き
- [ ] `auth`サブコマンド追加

---

## 4. 設定未完了時の動作

### opencode-ts
- **ファイル**: `packages/opencode/src/cli/cmd/tui/app.tsx` (277-286行目)
- プロバイダー未設定を検出
- 自動的にプロバイダー接続ダイアログを表示
- TUIは正常に起動し、ユーザーを設定へ誘導

```typescript
createEffect(
  on(
    () => sync.status === "complete" && sync.data.provider.length === 0,
    (isEmpty, wasEmpty) => {
      if (!isEmpty || wasEmpty) return
      dialog.replace(() => <DialogProviderList />)
    },
  ),
)
```

### opencode-rs
- **ファイル**: `src/tui/app.rs` (103行目付近)
- モデル未設定時に即座にエラー終了
- `Error: No model configured`を表示してクラッシュ
- TUIが起動しない

### 実装に必要な作業
- [ ] `App::new()`でのハードエラーを削除
- [ ] モデル未設定状態でTUI起動を許可
- [ ] 初期表示でプロバイダー/モデル設定ダイアログを表示

---

## 5. ローカル状態管理

### opencode-ts
- **ファイル**: `packages/opencode/src/cli/cmd/tui/context/local.tsx`
- エージェント別の現在モデル管理
- 利用可能プロバイダーに対するモデル検証
- フォールバックチェーン: CLIオプション → 設定 → 最近 → 最初の利用可能モデル
- モデル循環（最近/お気に入り内で切り替え）
- バリアント選択（バリアントを持つモデル用）
- 設定の永続化

### opencode-rs
- **ファイル**: `src/tui/app.rs`
- `App`構造体に単純な文字列フィールド:
  - `model_display`
  - `provider_id`
  - `model_id`
- 状態管理システムなし
- フォールバックロジックなし

### 実装に必要な作業
- [ ] モデル状態管理モジュール
- [ ] フォールバックチェーンロジック
- [ ] 状態の永続化

---

## 6. プロバイダー同期システム

### opencode-ts
- **ファイル**: `packages/opencode/src/cli/cmd/tui/context/sync.tsx`
- バックエンドサーバーとの同期:
  - モデル付きプロバイダー一覧
  - プロバイダー認証方式
  - プロバイダー別デフォルトモデル
  - 接続済みプロバイダー一覧
- イベント経由でリアルタイム更新

### opencode-rs
- **ファイル**: `src/provider/mod.rs`
- 静的なプロバイダー定義
- 同期メカニズムなし
- ハードコードされたモデル一覧

### 実装に必要な作業
- [ ] プロバイダー情報の動的取得
- [ ] モデル一覧のAPI取得
- [ ] 状態同期システム

---

## 7. スラッシュコマンド

### opencode-ts
- **ファイル**: `packages/opencode/src/cli/cmd/tui/prompt/autocomplete.tsx`
- `/models` - モデルセレクターを開く
- `/connect` - プロバイダー接続ダイアログを開く
- `/agents` - エージェント一覧
- `/mcp` - MCPサーバー切り替え
- `/session` - セッション管理
- `/theme` - テーマ切り替え
- 他多数

### opencode-rs
- **ステータス**: 未実装
- スラッシュコマンドパーサーなし
- 入力はすべてプロンプトとして扱われる

### 実装に必要な作業
- [ ] スラッシュコマンドパーサー
- [ ] コマンドハンドラー登録システム
- [ ] 各コマンドの実装

---

## 8. キーバインドシステム

### opencode-ts
- **ファイル**: `packages/opencode/src/cli/cmd/tui/context/keybind.tsx`
- `opencode.json`で設定可能
- リーダーキーサポート
- 組み込みバインディング:
  - `Ctrl+X M` - モデル一覧
  - `F2` - モデル循環
  - カスタムキーバインド対応

### opencode-rs
- **ファイル**: `src/tui/input.rs`
- 基本的なハードコードされたキーバインド
- `Action::ModelSelector`は定義済みだが未接続
- 設定可能なキーバインドなし

### 実装に必要な作業
- [ ] `Action::ModelSelector`をダイアログに接続
- [ ] 設定からのキーバインド読み込み
- [ ] リーダーキーサポート

---

## 9. ダイアログシステム

### opencode-ts
- **ファイル**: `packages/opencode/src/cli/cmd/tui/ui/dialog-select.tsx`
- 汎用選択ダイアログコンポーネント
- 検索/フィルター機能
- カテゴリグループ化
- キーボードナビゲーション
- スタック可能なダイアログ管理

### opencode-rs
- **ステータス**: 部分実装
- `components.rs`に基本的なダイアログ構造あり
- 検索/フィルター機能なし
- スタック管理なし

### 実装に必要な作業
- [ ] 汎用選択ダイアログコンポーネント
- [ ] 検索/フィルター機能
- [ ] ダイアログスタック管理

---

## 10. その他の機能差

| 機能 | opencode-ts | opencode-rs |
|------|-------------|-------------|
| セッション共有 | 完全実装 | 未実装 |
| MCPサーバー管理 | TUI対応 | 設定のみ |
| テーマ切り替え | ダイアログUI | 設定のみ |
| エージェント選択 | ダイアログUI | 設定のみ |
| ファイル添付 | ドラッグ&ドロップ | 未実装 |
| 画像表示 | Sixel/iTerm2対応 | 未実装 |
| プラグインシステム | 完全実装 | 未実装 |
| 自動更新 | 対応 | 未実装 |

---

## 実装優先度

### 高（起動に必要）
1. モデル未設定でもTUI起動を許可
2. 基本的なモデル選択ダイアログ
3. APIキー入力・保存機能

### 中（基本的なUX改善）
4. スラッシュコマンドシステム
5. プロバイダー接続ダイアログ
6. 認証ストレージモジュール

### 低（機能パリティ）
7. お気に入り/最近のモデル追跡
8. モデル循環キーバインド
9. 完全なOAuth対応

---

## 参照ファイル

### TypeScript（移植元）
- `packages/opencode/src/cli/cmd/tui/component/dialog-model.tsx`
- `packages/opencode/src/cli/cmd/tui/component/dialog-provider.tsx`
- `packages/opencode/src/cli/cmd/tui/context/local.tsx`
- `packages/opencode/src/cli/cmd/tui/context/sync.tsx`
- `packages/opencode/src/cli/cmd/tui/ui/dialog-select.tsx`
- `packages/opencode/src/cli/cmd/auth.ts`
- `packages/opencode/src/auth.ts`

### Rust（修正対象）
- `src/tui/app.rs` - ハードエラー削除、ダイアログ状態追加
- `src/tui/components.rs` - ダイアログコンポーネント追加
- `src/tui/input.rs` - ModelSelector アクション接続
- `src/tui/ui.rs` - ダイアログレンダリング
- `src/provider/mod.rs` - 認証ロード追加
- 新規: `src/auth.rs` - 認証ストレージモジュール
