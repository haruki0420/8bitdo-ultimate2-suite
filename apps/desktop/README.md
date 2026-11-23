# 8BitDo Ultimate 2 — Desktop (Tauri v2)

`apps/desktop` は 8BitDo Ultimate 2 スイートのデスクトップ UI です。このスキャフォールドでは
**Tauri v2 + TypeScript + Vite + React + Tailwind CSS** の土台と、Rust 側の `app_info`
コマンド 1 つだけを提供します。**BLE 接続・デバイス読み書きはまだ実装していません**（次のフェーズ）。

## 前提

- Node.js 20+ / **pnpm**（`npm` / `npx` は使わない）
- Rust 1.77+（stable）
- Linux: `webkit2gtk-4.1`, `gtk3`, `libsoup-3.0`, `librsvg2`, `libssl`, `pkg-config`, `build-essential`

本スキャフォールドを実際にビルド・起動確認した環境（Linux x86_64）:

| ツール | バージョン |
| --- | --- |
| rustc / cargo | 1.100.0 |
| Node.js | 26.13.1 |
| pnpm | 12.22.0 |
| webkit2gtk-4.1 | 2.54.3 |
| tauri (Rust crate) | 2.x（`Cargo.lock` で固定） |
| Tauri CLI (`@tauri-apps/cli`) | 2.12.6 |
| Vite / React / TypeScript / Tailwind | 7.3 / 19.3 / 5.9 / 4.3 |

Debian/Ubuntu 系の例:

```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential pkg-config \
  libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev
```

## セットアップと開発

```bash
cd apps/desktop
pnpm install --frozen-lockfile   # 初回は --frozen-lockfile を外して lockfile を生成
pnpm exec tauri dev              # Vite dev server + Rust ビルド（ウィンドウが開く）
```

X11 で動かす場合（Wayland を避ける）:

```bash
env -u WAYLAND_DISPLAY pnpm exec tauri dev
```

フロントエンドだけを確認する場合は `pnpm dev`（http://localhost:1420）。ただし
`app_info` は Tauri IPC 経由なので、ブラウザ単体では「Rust バックエンドに接続できません」
という警告が表示されます（想定どおりの挙動）。

## ビルドと検証

```bash
cd apps/desktop
pnpm build                             # tsc --noEmit + vite build
pnpm test                              # vitest run
pnpm exec tauri build --debug --no-bundle   # Rust バイナリのビルド（バンドルなし）
cargo check --manifest-path src-tauri/Cargo.toml   # Rust のみを型チェック
```

`--no-bundle` は AppImage / deb 作成をスキップします。バンドルする場合は
`pnpm exec tauri build`（Linux では `linuxdeploy` などが必要）。

## 構成

```
apps/desktop/
  index.html, vite.config.ts, tsconfig.json, package.json
  pnpm-workspace.yaml  pnpm 12 の設定（esbuild のビルドスクリプト許可）
  app-icon.png         アイコンの原本（1024x1024）
  src/
    main.tsx            React エントリ
    App.tsx             上部バー + 左レール + コンテンツ（デバイス）
    lib/app-info.ts     invoke("app_info") のラッパー
    index.css           Tailwind v4（@import "tailwindcss"）
    test/setup.ts       Vitest + Testing Library のセットアップ
    App.test.tsx        レンダリング / 失敗時警告のテスト
  src-tauri/
    Cargo.toml          独立ワークスペース（ルート Cargo.toml の members には入れない）
    tauri.conf.json     アプリ名 / identifier / ウィンドウ 1120x800（最小 900x620）
    capabilities/       core:default のみ
    src/lib.rs          app_info コマンドと run()
    src/main.rs         バイナリのエントリ
    icons/              プレースホルダ（`pnpm exec tauri icon app-icon.png` で再生成）
```

### 重要な設計メモ

- **`src-tauri` はルートワークスペースに入れない**。`apps/desktop/src-tauri/Cargo.toml` が
  空の `[workspace]` を宣言して独立したワークスペースルートになっています。これにより
  `cargo test --workspace`（リポジトリルート）が WebKitGTK 依存を取り込みません。
  ※ [docs/TAURI_PLAN.md](../../docs/TAURI_PLAN.md) の P0 には「src-tauri を workspace に
  追加する」と書かれていますが、このスキャフォールドでは追加していません（独立クレート）。
- `eightbitdo-core` は **path 依存**ですが `default-features = false`（`ble` フィーチャー
  OFF）で参照しています。BLE I/O を実装するフェーズで既定フィーチャーに切り替えてください。
- Tailwind は **v4** のため `tailwind.config.*` はありません（CSS の `@import` と
  `@theme` で設定、Vite プラグイン `@tailwindcss/vite` を使用）。
- pnpm 12 は依存のビルドスクリプトを既定で実行しないため、`pnpm-workspace.yaml` の
  `allowBuilds` で **esbuild** のみ許可しています。この設定がないと
  `pnpm install --frozen-lockfile` が `ERR_PNPM_IGNORED_BUILDS` で失敗します。
- アイコンは `tauri icon` で生成したプレースホルダです（Linux で使う PNG と
  `icon.ico` / `icon.icns` のみ残しています）。正式なアートワークができたら
  `pnpm exec tauri icon app-icon.png` で再生成してください。
- CSP は未設定（`null`）。デバイス I/O を追加する際に必要に応じて絞ってください。
