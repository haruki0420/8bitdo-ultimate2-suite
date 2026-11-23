# Padbench — Desktop (Tauri v2)

`apps/desktop` は Padbench（8BitDo Ultimate 2 対応）の正式なデスクトップ UI です。Tauri v2 + TypeScript + Vite + React + Tailwind CSS（v4）で構成します。

デザインの正本は [docs/design/IMPORTED_STYLE_HYPERSTUDIO.md](../../docs/design/IMPORTED_STYLE_HYPERSTUDIO.md)（参考: [Refero Styles](https://styles.refero.design/style/8eb9c53e-d69c-497a-b640-610856cf3a60)、提携・推奨関係なし）です。旧案からの移行経緯は [docs/DESIGN_TAURI.md](../../docs/DESIGN_TAURI.md) にあります。

**現状（P0 / P1 完了）**: デザイントークン、共通コンポーネント 11 種、シェル（上部バー / 左ナビ / 右レール）、静的 2 画面（デバイス未接続・入力モニター見本）まで実装済みです。Rust 側は `app_info` コマンドのみで、**BLE 接続とデバイス読み書きは未実装（P2 以降）**です。上部バーの副題は書き込み機能（P3）が入るまで「開発版（画面骨格）」とします。

## 前提

- **Node.js**: `^20.19.0 || >=22.12.0` / **pnpm**: `12.22.0`（`npm` / `npx` は使わない）
- **Rust**: **1.90+**（`tauri` 2.12.7 要件。ルート workspace の MSRV 1.85 とは独立）
- **Linux 依存**: `libwebkit2gtk-4.1-dev` `build-essential` `pkg-config` `libgtk-3-dev` `libayatana-appindicator3-dev` `librsvg2-dev` `libssl-dev`

動作確認済み環境（Linux x86_64）: rustc 1.100.0 / Node.js 26.13.1 / pnpm 12.22.0 / webkit2gtk-4.1 2.54.3 / `@tauri-apps/cli` 2.12.6 / Vite 7.3 / React 19.3 / TS 5.9 / Tailwind 4.3。

## 開発・ビルド・検証

```bash
cd apps/desktop
pnpm install --frozen-lockfile
pnpm exec tauri dev                         # 開発起動（Wayland 回避時は env -u WAYLAND_DISPLAY）
pnpm dev                                    # Web のみ（http://localhost:1420。IPC 未接続警告は想定内）
```

```bash
pnpm build                                  # tsc --noEmit + vite build
pnpm test                                   # vitest run
pnpm exec tauri build --debug --no-bundle   # バンドルなしの Rust ビルド
cargo check --manifest-path src-tauri/Cargo.toml
```

CI は [`.github/workflows/desktop.yml`](../../.github/workflows/desktop.yml) の `desktop-web`（install / build / test）と `desktop-rust`（fmt / clippy / check）で実行します。

## ディレクトリ構成

```text
apps/desktop/
  package.json, pnpm-workspace.yaml, vite.config.ts, tsconfig.json, index.html
  app-icon.png                  アイコン原本（プレースホルダ）
  src/
    main.tsx, App.tsx           エントリ・シェル・トースト・ダイアログ
    styles/tokens.css           デザイントークン（値の正本）
    index.css                   Tailwind v4 の @theme 写像
    design/{tokens,contrast}.ts TS 側の写しと WCAG コントラスト計算
    components/                 共通コンポーネント 11 種 + インライン SVG アイコン
    shell/                      TopBar / SideNav / SafetyRail / 画面定義
    screens/                    DevicesScreen / InputMonitorScreen / PlaceholderScreen
    input/                      入力レポート正規化（Y_SIGN）と見本データ
    lib/app-info.ts             invoke("app_info") ラッパー
  src-tauri/
    Cargo.toml                  独立ワークスペース（ルート workspace には含めない）
    tauri.conf.json             Padbench / io.github.haruki0420.padbench / 1280x800（最小 900x620）
    capabilities/               現状は core:default のみ（P2 以降で分割）
    src/{lib,main}.rs           app_info コマンドとエントリ
```

## 設計・セキュリティ・スタイルの要点

- **独立ワークスペース**: `src-tauri/Cargo.toml` に空の `[workspace]` を置き、ルート `cargo test --workspace` が WebKitGTK 依存を取り込まない構成にしています。`eightbitdo-core` は現状 `default-features = false`（BLE OFF）の path 依存です。
- **pnpm 12 設定**: `pnpm-workspace.yaml` の `allowBuilds` で `esbuild` のみ許可しています（未設定だと `--frozen-lockfile` が失敗する）。
- **CSP と capability**: `tauri.conf.json` にローカル資産のみ許可する CSP（`default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self' ipc: http://ipc.localhost; object-src 'none'; base-uri 'none'`）を設定済みです。`fs` / `shell` / `http` プラグインは追加しません（[docs/TAURI_PLAN.md](../../docs/TAURI_PLAN.md) §3.2）。
- **アイコン**: 現状はプレースホルダです。正式版ができたら `pnpm exec tauri icon app-icon.png` で再生成します。
- **トークンの同期**: `src/styles/tokens.css`、`src/index.css`（`@theme`）、`src/design/tokens.ts`、`src/design/tokens.test.ts` の 4 か所を同時に更新します。`tokens.test.ts` と `style-audit.test.ts` でコントラスト（本文 4.5:1 / アイコン 3:1）、weight 400、角丸（4/8/99/9999）、影・グラデ禁止、直書き色や任意値ユーティリティの混入を自動検査します（`?raw` import のため `vite.config.ts` で `test.css: true`）。
- **パレットと見た目**: `obsidian` `#101010` / `carbon` `#080808` / `graphite` `#212121` / `iron` `#474747` / `smoke` `#9c9c9c` / `ash` `#c1c1c1` / `chalk` `#f3f3f3` / `signal-white` `#ffffff` / `compass-gold` `#6f6759` / `pulse-green` `#98ff38`（派生は `paper-ink` `#1a1a1a` と `danger` `#b42318` のみ）。アイコンは 16px・1.5 stroke のインライン SVG 線画のみで、色は `chalk` か `compass-gold` を使います。
- **フォント**: 現状は OS フォールバックです。Inter / JetBrains Mono / IBM Plex Mono の woff2 同梱は P6 で対応予定です（`tokens.css` に `TODO(fonts)` あり）。
- **用語**: [docs/DESIGN_TAURI.md](../../docs/DESIGN_TAURI.md) の用語表を正本とし、`terminology.test.ts` で旧表記（ライブ入力・設定値・録音など）と絵文字の混入を防いでいます。
