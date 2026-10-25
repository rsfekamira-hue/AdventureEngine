# AdventureEngine

作品データを持たないRust製のイベント・描画・デバッグ部品。ゲームとは独立したGitリポジトリとして管理する。

正本：[rsfekamira-hue/AdventureEngine](https://github.com/rsfekamira-hue/AdventureEngine)。共通コアロジック・描画・デバッグの変更と履歴はこのリポジトリへ保存する。

## 構成と依存

| crate | 責務 |
| --- | --- |
| `engine-core` | Event・Flow・条件・共有パラメーター、文字送り、画像配置、パーティクル |
| `render-contract` | GPU・ブラウザに依存しないスプライト描画データ |
| `sprite-renderer` | `render-contract` を受け取るwgpu描画器 |
| `engine-renderer-2d` | eguiの描画命令・画像・文字・図形の描画。eframeの起動処理は含まない |
| `engine-debug-tools` | コマンド解析、JSON状態検査、上限付き再生、CLI実行ループ、quiet制御・診断、`DebugTarget` / `CliHost` 契約 |
| `engine-capture` | ウィンドウ不要のegui/wgpu描画、画像読み戻し、PNG保存 |
| `engine-debug-cli` | Core JSONを直接読み込む単独実行ツール |

`engine-core` はゲーム・描画器・デバッグツールを参照しない。`engine-debug-tools` もゲームやCoreへ依存しない。CLIやゲーム側アダプターが公開入力と読み取りAPIを接続する。

Coreの診断スナップショットは読み取り専用。内部状態の所有を保つため、強制ジャンプ・復元・パラメーター編集の実装はCore内部の `debug-jump` featureに置く。通常ビルドでは含めない。操作列再生は強制書き換えを使わず通常入力を使う。

ゲーム固有の開始地点、探索ルール、画面構成、表示ラベル、画像・音声・原稿、F3画面とWeb撮影の接続処理は利用側で管理する。

## 単独で検証する

```powershell
cargo test --locked --workspace --all-features
cargo run --locked -p engine-debug-cli -- --definition examples/minimal.json --event sample --commands examples/minimal.commands
```

対話入力は `--commands` を省略する。`--plan --commands <file>` は定義ファイルなしで操作列をJSONに変換する。

コマンド：`advance [count]`、`complete`、`choose <番号|ID>`、`choose-id <ID>`、`show`、`check <JSON pointer> <JSON>`、`until <JSON pointer> <JSON>`、`restart`、`capture <name>`、`quit`。Coreには本文表示がないため `complete` は状態を変えず、`capture` は撮影地点の通知のみ。画像の保存は利用側の描画接続が担当する。Core CLIは外部条件を自動補完しない。外部条件を持つ作品は利用側のアダプターから供給する。

`examples/minimal.json` は作品データ不要の最小例。ゲーム側の操作列は同じ文法だが、検査するJSONパスと選択肢IDは対象アプリケーションが定義する。

## CLIと画像撮影を接続する

`engine_debug_tools::cli::run` は `Runner<T>` と入力・出力を受け取る。利用側は `CliHost<T>` の `display`（本文等の出力）と `capture`（撮影または地点通知）を実装する。quiet指定時も検査結果・明示的なshow・撮影結果は出力する。撮影前には通常操作と演出の完了を待ち、撮影の失敗にも行番号・コマンド・現在地を付ける。Core CLIにも `--quiet` を指定できる。

`engine_capture::Capture::new(width, height)` で撮影環境を作り、`context()` へフォント・画像を登録する。`save(path, |ctx| { /* 通常UIを描画 */ })` は初回のフォント・配置更新を含む2回の描画を行い、RGBAのPNGを保存する。コールバック内ではゲームを進行させず、表示状態を読み取る。サイズは0およびGPU上限超過を拒否する。出力先の親フォルダは呼出し側が用意する。

撮影はデスクトップ向けの任意依存として接続する。GPUまたはソフトウェアGPUが必要で、ウィンドウ・音声・ゲームの素材読み込みは含まない。GPUなしのCLIにはこのcrateを依存させない。ブラウザ固有の入力・音声・リサイズ確認は利用側で行う。既存のコマンド形式・DebugTarget・Runnerの公開APIは変更していない。

GPUによる読み戻し・行パディング・連続撮影の確認：

```powershell
cargo test --locked -p engine-capture -- --ignored
```

## ゲームと同時開発する

```text
作業フォルダ/
├─ AdventureEngine/
└─ CorruptinAIGame/
```

ゲームはCargoの相対 `path` 依存でこのコードを利用する。ソースのコピーや常駐サーバーは不要。ゲームのWebAssemblyへコンパイルされるので、公開時に別のエンジンサーバーは不要。

開発中の変更は次のビルドに反映される。ゲーム側の通常Webビルドは `engine-revision.json` とHEAD・作業ツリーの一致を検査する。意図的な同時開発に限り、ゲームのビルド時に `CORRUPTIN_ENGINE_DEV=1` を設定する。完了後は解除し、共通側を先にコミットし、ゲーム側の `scripts/engine.ps1 -Action Record` で採用コミットを記録して検証する。詳細はゲーム側の `docs/shared-engine.md`。

## GitHubとの同期

`origin` は `https://github.com/rsfekamira-hue/AdventureEngine.git`、管理ブランチは `main`。別の環境ではゲームの隣へcloneする。

```powershell
git clone https://github.com/rsfekamira-hue/AdventureEngine.git
```

更新を取得するときは `git fetch origin`、main上で取り込むときは `git pull --ff-only` を使う。ゲームの採用コミットへ合わせる場合はゲーム側の `scripts/engine.ps1 -Action Checkout` を使う。

共通側の変更をコミットして `git push origin main` した後、ゲーム側で採用コミットを記録・検証する。ローカルの相対Cargo参照は維持している。CargoをGit依存に切り替える場合はゲーム側の全共通crateを同じURL・同じ `rev` へ揃え、`Cargo.lock` を更新する。配布ライセンスは未設定。

## 移動元

2026-10-23に `CorruptinAIGame` のコミット `923a1b6` を基に共通部品を分離。元の履歴はゲーム側に保持する。このリポジトリへゲームの原稿・画像・世界観資料は複製しない。
