# Padbench — Desktop (Tauri v2)

`apps/desktop` は Padbench（8BitDo Ultimate 2 対応）のデスクトップ UI です。
**Tauri v2 + TypeScript + Vite + React + Tailwind CSS** で、デザインは
[docs/DESIGN_TAURI.md](../../docs/DESIGN_TAURI.md)（案 C「昼の作業台と夜のステージ」）が正本です。

現在は**画面骨格まで**実装済みです: デザイントークン、共通コンポーネント 10 種、シェル
（上部バー / 左ナビ / 右レール）、デバイス画面（未接続）、入力モニター画面（ダークステージの
見本）。Rust 側は `app_info` コマンド 1 つだけで、**BLE 接続・デバイス読み書きはまだ実装して
いません**（次のフェーズ）。

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
    App.tsx             シェル + 画面の切替 + トースト + ダイアログ
    styles/tokens.css   デザイントークン（CSS 変数。実装上の値の正本）
    design/tokens.ts    tokens.css の TypeScript 側の写し（テストと参照用）
    design/contrast.ts  WCAG コントラスト比（テスト用）
    components/         Button / Field / Select / Switch / Tabs / Table /
                        Dialog / Toast / Banner / EmptyState + アイコン
    shell/              TopBar / SideNav / SafetyRail / 画面定義
    screens/            DevicesScreen / InputMonitorScreen / PlaceholderScreen
    input/              入力レポートの正規化（Y_SIGN）と見本データ
    lib/app-info.ts     invoke("app_info") のラッパー
    index.css           Tailwind v4 の @theme 写像とブラウザ既定面の調整
    test/setup.ts       Vitest + Testing Library のセットアップ
  src/**/*.test.*       トークン / 用語 / 入力正規化 / コンポーネント / 画面の回帰テスト
  src-tauri/
    Cargo.toml          独立ワークスペース（ルート Cargo.toml の members には入れない）
    tauri.conf.json     アプリ名 Padbench / identifier io.github.haruki0420.padbench / ウィンドウ 1280x800（最小 900x620）
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

### デザイン実装のメモ

- **トークンの正本は `src/styles/tokens.css`**。`src/index.css` の `@theme` は
  `var(--...)` を参照するだけで、値を二重に書いていません。`src/design/tokens.ts` は
  テストから同じ値を参照するための写しで、`tokens.test.ts` が CSS との一致、WCAG
  コントラスト（本文 4.5:1 / 操作輪郭 3:1）、単一アクセント、グラデーション禁止を検査します。
- **フォントは OS フォールバック**です。Noto Sans JP / Hack の woff2 サブセットは未同梱で、
  `src/styles/tokens.css` に `TODO(fonts)` として `@font-face` の追加手順を残しています。
  同梱するときは `apps/desktop/public/fonts/` に置き、`font-display: swap` と preload を
  付けてください（[docs/DESIGN_TAURI.md](../../docs/DESIGN_TAURI.md) §5.2.1）。
- **用語は [docs/DESIGN_TAURI.md](../../docs/DESIGN_TAURI.md) §6.3 が正本**です。
  「入力モニター」「現在値」「記録」「書き込み対応版」「次にすること」を使い、
  `terminology.test.ts` が旧表記（ライブ入力 / 設定値 / 録音など）と絵文字の混入を検出します。
- **アイコンはインライン SVG の線画だけ**（16px・1.5 stroke）。アイコンフォントと絵文字は
  使いません。ライセンス表記を増やさないため、アイコンセットも導入していません。
- `pnpm test` は **CSS の `?raw` import を使うため `vite.config.ts` で `test.css: true`** に
  しています。
