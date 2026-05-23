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