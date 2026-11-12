# AdventureEngine

作品データを持たないRust製のイベント・描画・デバッグ部品。ゲームとは独立したGitリポジトリとして管理する。

C++版のイベント実行コアは `cpp/` にある。C++20標準ライブラリだけで、Flow、条件、パラメーター更新、イベント間の呼び出しと復帰、演出完了の受付を処理する。ファイル形式の読み込みと表示は利用側が担当する。単独で検証する場合は `cmake -S cpp -B build-cpp` で構成し、ビルドして CTest を実行する。

`-DADVENTURE_ENABLE_DEBUG_JUMP=ON` でデバッグ用の経路探索、ジャンプ前の状態復元、型付きパラメーター編集を有効にできる。探索は通常の選択・文章送りを実行し、上限内で到達できない場合の強制移動を結果で区別する。

正本：[rsfekamira-hue/AdventureEngine](https://github.com/rsfekamira-hue/AdventureEngine)。共通コアロジック・描画・デバッグの変更と履歴はこのリポジトリへ保存する。

## 構成と依存

| crate | 責務 |
| --- | --- |
| `engine-core` | Event・Flow・条件・共有パラメーター、文字送り、画像配置、パーティクル |
| `render-contract` | GPU・ブラウザに依存しないスプライト描画データ |
| `sprite-renderer` | `render-contract` を受け取るwgpu描画器 |
| `engine-renderer-2d` | 描画命令の実行。UIはegui、`gpu` featureの画像は専用wgpuスプライト。eframeの起動処理は含まない |
| `engine-debug-tools` | コマンド解析、JSON状態検査、上限付き再生、CLI実行ループ、quiet制御・診断、`DebugTarget` / `CliHost` 契約 |
| `engine-capture` | ウィンドウ不要のegui/wgpu描画、画像読み戻し、PNG保存 |
| `engine-debug-cli` | Core JSONを直接読み込む単独実行ツール |

`engine-core` はゲーム・描画器・デバッグツールを参照しない。`engine-debug-tools` もゲームやCoreへ依存しない。CLIやゲーム側アダプターが公開入力と読み取りAPIを接続する。

画像の登録・差し替え・解放は `TextureStore` が `TextureCommand` を受けて行う。
`Renderer2D` はストアを借用して描画し、画像の所有・取得・寿命管理を行わない。
取得する場面・通信・待機・キャッシュ予算は利用側と `engine-image-resources` で管理する。

### 画像の直接描画

`engine-renderer-2d` の `gpu` featureを有効にし、環境のegui-wgpu `RenderState` から
`TextureStore::with_gpu(state)` を作って通常の描画へ渡す。WindowsとWebは同じ実装を使う。
ヘッドレス撮影では `Capture::render_state()` で同じバックエンドを接続できる。

RGBA8の画像をそのままsRGBテクスチャへ転送し、専用の頂点・フラグメントシェーダーで描画する。
eguiの画像登録・画像メッシュは使わず、CPUの全画素色変換、Compute変換、変換先テクスチャも不要。
アップロードと描画は既存の同一Device/Queue上で順序付け、CPUでGPU完了を待たない。
未表示の先読み画像も転送をsubmitする。GPUの寸法上限超過は登録前にエラーにする。

透明な輪郭の色にじみを防ぐため、シェーダーが4テクセルを線形色空間でアルファ乗算してから補間する。
画面へ描かれるピクセルだけを処理し、素材全体を毎フレーム走査しない。UIと同じ色合成規則を維持する。
egui-wgpuの描画コールバックは重ね順・クリップ・既存レンダーパスへの接続だけに用いる。
このライブラリ境界の必須trait以外に、画像配送のtrait/dyn抽象化は設けない。

`textures/gpu.rs` は画像の割当・転送・所有権による解放、`sprites.rs` はパイプラインと小さな頂点バッファを担当する。
GPUハンドルはWebのSend/Sync制約に合わせてeguiのコールバック資源スロットへ置くが、
画像の寿命は `TextureStore` のleaseで管理する。描画器側に画像キャッシュの責務はない。

互換経路の `TextureStore::default()` は従来のegui画像を使用する。
`TextureStore::apply()` は `Result<(), String>` を返すため、呼出し側は失敗を処理する。
一括準備する場合も未乗算RGBAのまま `Register` を渡す。`insert(ColorImage)` は既存UI用の互換入口。

GPUテストでは旧描画との画素比較（透明度・補間・UV・回転・クリップ・UI重なり）、
2048/4096角の直接転送、上限超過・差し替え・解放・ストア破棄を検証する。
`cargo test -p engine-renderer-2d --features gpu -- --nocapture`。

### GPUデジタルパネル

`gpu` featureの `DrawCommand::Digital(DigitalDraw)` は、半透明の面・厚みのある斜め角の枠・
データ線・回転リング・走査・文字・分割進捗バーを専用wgpuシェーダーで描く。
eguiは描画順とクリップの接続にのみ使用し、パネルの図形・文字メッシュ生成には使用しない。

`DigitalSurface` は `Panel`・`Scan`・`Reconstruct`・`Confirmation`・`Press`。作品固有の意味は利用側が割り当てる。
`Confirmation` は角形の封印が内側へ閉じる面、`Press` は押し込み・波紋・確定状態を持つ面。
どちらも `phase` で一度だけ再生し、1で確定状態を保持する。入力や合意の判定は行わない。
軽減表示では `phase = 1`・`seconds = 0` を渡す。色はRGBA指定を共有し、文字は面と一緒に押し込む。
`rect`・RGBA `color`・`phase`・任意の `progress`・相対配置の `DigitalText` を指定する。
装飾の時計は `seconds`、全体の透明度は `color[3]`。軽減設定では `seconds = 0` に固定する。
待機・完了・フェード時間は利用側の責務で、時計を止めれば装飾も静止する。
画像は既存の `Sprite` 命令を前後に重ねられる。画像素材の追加は必須ではない。

日本語を含む文字は初回のみ32pxでR8アトラスへ焼き、以後は再利用する。
頂点バッファは同一フレーム内の各パネルで分離し、容量が足りる間は更新だけを行う。
アトラスは2048角・同時表示1764種類まで。累積文字数が容量を超える場合はフレーム境界で再構成し、
現在の全パネルで必要な文字を先に登録して、後続パネルによる上書きを防ぐ。
フォント変更は `Renderer2D` の作り直しで反映する。通常の `Rect`・`Text` 命令は従来どおり。

GPUテストは複数パネルの色・クリップ・半透明合成・装飾の時間変化と再現性・文字の再利用を確認する。
`cargo test -p engine-renderer-2d --features gpu digital::tests`。

### GPUパーティクル

`DrawCommand::Particles(ParticleDraw)` は、検証済みの `Arc<Vec<engine_core::particles::ParticleEmitter>>`、
時刻・表示領域・色だけを渡す。Compute対応のGPUと `TextureStore::with_gpu` が必要。
WebはWebGPUが対象で、WebGL向けのCPUフォールバックは設けない。

`particles_compute.wgsl` は1粒子・残像につき1スレッド（64スレッド/ワークグループ）。
seedと絶対時刻から寿命・軌道・文字番号を直接求め、固定長のインスタンスバッファへ書く。
積分履歴・ソート・衝突・間接描画・GPUからCPUへの読み戻しは通常再生に不要。
一時停止・再実行・シークも同じ時刻なら同じ結果になる。

`particles_render.wgsl` は1 emitterにつき1回のインスタンス描画。文字・フレーズは
定義の初回表示時に固定サイズのR8アトラスへ焼き、フレームごとの文字組み・フォント再生成はしない。
発光は同じ四角形のフラグメント処理で合成する。CPU側は1 emitterにつき176バイトのuniformを更新する。
定義はフレーム間で同じArcを保持する。バッファとアトラスは再利用し、定義が使われなくなると回収する。

発生・移動の基準矩形とクリップは別で、粒子は人物などの基準矩形の外へ出せる。
最終的な画面境界と前後関係は既存のDrawListで管理する。
公開済みのCPU `ParticleEmitter::sample` は参照計算・互換APIとして残すが、GPU再生では呼ばない。

テストは実GPUで参照値との一致、時刻変更・再実行、バッファ再利用、人物枠からのはみ出し、
UIとの重なり、文字アトラスの再利用・回収を確認する。

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

### 画像の非同期ロード接続（2026-11-01）

`engine-image-resources` はOS・実行器・GPUに依存しない画像要求と保持方針を提供する。
`ImageManager::set_targets` に必須画像と順序付きの先読み候補を渡し、`next_request` の
`ImageRequest` を利用側の具体的なローダーへ配送する。`ImageResult` を `accept` で受け取り、
`take_commands` の登録・解放メッセージをテクスチャ保管へ渡す。trait・dyn・汎用メッセージバスは使わない。
未完了要求は1件で、重複・古い結果・不要になった結果を排除する。RGBA容量でキャッシュを制限し、
現在の必須画像は保護する。予算0は現在場面だけの保持で、先読みを無効にする。
各管理インスタンスは独立した結果配送先を持つ。別セッションの返信を混在させない。

`engine-renderer-2d::textures::TextureStore` はeguiテクスチャの登録・所有・解放だけを担当する。
`Renderer2D` は画像を所有せず、描画時にストアを借用する。登録された画像は未表示でも
eguiのtexture delta経由で転送される。登録済みは描画順序上利用可能という意味で、GPU実行完了の通知ではない。

API移行：`Renderer2D::new(ctx, font, images)` は `new(ctx, font)` へ変更。
画像の登録・寸法取得・保持は `TextureStore` へ移し、`render(ui, frame, &textures)` を使う。
イベント形式・進行コアは変更しない。

検証：全workspace・全featureテスト成功。未表示テクスチャの登録deltaと解放、要求の優先順位・
結果照合・容量上限・失敗の再試行抑止を検査。実GPUの利用先検証はゲーム側の制作記録を参照。

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
