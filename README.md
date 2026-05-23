# mimium-audio-plugin

Rust + Clack + Wry 構成の CLAP プラグインです。Webview 上の Monaco editor で mimium ソースを編集し、mimium-rs の Wasm JIT バックエンドでコンパイルしたプログラムをオーディオスレッドで実行します。

## 構成

- `plugin/`: CLAP プラグイン本体（Rust）
- `webview/`: Vite + React + Monaco の UI
- `xtask/`: `.clap` を生成する補助コマンド
- `packaging/clap-wrapper/`: clap-wrapper 用の CMake 設定

## 開発

1. `webview` の dev server 起動
   - `cd webview && pnpm dev`
2. Rust 側ビルド
   - `cargo build`

Debug ビルド時の GUI は `http://localhost:5173` を参照します。

## パッケージ

`.clap` 生成:

- `cargo run -p xtask -- package --release`

生成物は `target/package/release/mimium-clap-plugin.clap` に配置されます。

`clap-wrapper` 経由で `.vst3` 生成:

- `cargo run -p xtask -- package --release --format vst3`

`.clap` と `.vst3` を同時生成:

- `cargo run -p xtask -- package --release --all-formats`

### clap-wrapper の場所

`xtask` は以下の優先順で `clap-wrapper` を探します。

1. 環境変数 `CLAP_WRAPPER_ROOT`
2. `third_party/clap-wrapper`

例:

- `export CLAP_WRAPPER_ROOT=/path/to/clap-wrapper`

### clap-host でロードするときの注意

- `clap-host` が直接ロードできるのは CLAP 形式のみです（VST3 は不可）。
- 読み込み対象には `target/package/release/mimium-clap-plugin.clap` を使ってください。
- `target/wrapper-stage/release/Mimium CLAP Plugin.clap` は clap-wrapper 内部ステージ用のバンドルディレクトリです。`clap-host` に渡すと「The shared library was not found.」が出ることがあります。