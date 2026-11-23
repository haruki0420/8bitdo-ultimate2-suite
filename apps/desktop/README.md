# Padbench — Desktop (Tauri v2)

`apps/desktop` は Padbench（8BitDo Ultimate 2 対応）のデスクトップ UI です。
**Tauri v2 + TypeScript + Vite + React + Tailwind CSS** で、デザインは
**[docs/design/IMPORTED_STYLE_HYPERSTUDIO.md](../../docs/design/IMPORTED_STYLE_HYPERSTUDIO.md)**
（Imported style: Hyperstudio — 出典 [Refero Styles](https://styles.refero.design/style/8eb9c53e-d69c-497a-b640-610856cf3a60)）
が正本です。旧デザイン案 C のライト面トークンは置換済みで、経緯は
[docs/DESIGN_TAURI.md](../../docs/DESIGN_TAURI.md) §12 にあります。

スタイルの要点: near-black の `obsidian` キャンバス、1px `graphite` のヘアライン、
白 1 色のピル（`signal-white`）、`compass-gold` のアイコン線、`pulse-green` の
ライブ状態ドット、weight 400 の大きな文字。影・グラデーション・写真は使いません。

上部バーの副題は **「開発版（画面骨格）」** で、書き込み機能（P3）が入るまで能力を名乗りません。

現在は**画面骨格まで**実装済みです: デザイントークン、共通コンポーネント 11 種、シェル
（上部バー / 左ナビ / 右レール）、デバイス画面（未接続）、入力モニター画面（見本）。Rust 側は
`app_info` コマンド 1 つだけで、**BLE 接続・デバイス読み書きはまだ実装していません**（次のフェーズ）。

## 前提

- **Node.js `^20.19.0 || >=22.12.0`**（Vite 7 の `engines`）/ **pnpm 12.22.0**
  （`package.json` の `packageManager` と `engines` で固定。`npm` / `npx` は使わない）
- **Rust 1.90+**（`tauri` 2.12.7 の `rust-version`。`src-tauri/Cargo.toml` でも `rust-version = "1.90"`）。
  共通 core（ルート workspace）の MSRV は 1.85 のまま
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
pnpm install --frozen-lockfile
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

CI は [`.github/workflows/desktop.yml`](../../.github/workflows/desktop.yml) の
`desktop-web`（install → build → test）と `desktop-rust`（fmt / clippy / check）が担当します。
Node は `engines` と `packageManager` に合わせて固定し、Linux の WebKitGTK /
GTK / libsoup / librsvg は apt で導入します。

## 構成

```
apps/desktop/
  index.html, vite.config.ts, tsconfig.json, package.json
  pnpm-workspace.yaml  pnpm 12 の設定（esbuild のビルドスクリプト許可）
  app-icon.png         アイコンの原本（1024x1024、現状はプレースホルダ）
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
    capabilities/       core:default のみ（P2 以降で用途別 capability に分割する）
    src/lib.rs          app_info コマンドと run()
    src/main.rs         バイナリのエントリ
    icons/              プレースホルダ（`pnpm exec tauri icon app-icon.png` で再生成）
```

### 重要な設計メモ

- **`src-tauri` はルートワークスペースに入れない**。`apps/desktop/src-tauri/Cargo.toml` が
  空の `[workspace]` を宣言して独立したワークスペースルートになっています。これにより
  `cargo test --workspace`（リポジトリルート）が WebKitGTK 依存を取り込みません。
  [docs/TAURI_PLAN.md](../../docs/TAURI_PLAN.md) §4 も同じ方針（独立クレート。ルート
  workspace には追加しない）です。CI は `.github/workflows/desktop.yml` が別に実行します。
- `eightbitdo-core` は **path 依存**ですが `default-features = false`（`ble` フィーチャー
  OFF）で参照しています。BLE I/O を実装するフェーズで既定フィーチャーに切り替えてください。
- Tailwind は **v4** のため `tailwind.config.*` はありません（CSS の `@import` と
  `@theme` で設定、Vite プラグイン `@tailwindcss/vite` を使用）。
- pnpm 12 は依存のビルドスクリプトを既定で実行しないため、`pnpm-workspace.yaml` の
  `allowBuilds` で **esbuild** のみ許可しています。この設定がないと
  `pnpm install --frozen-lockfile` が `ERR_PNPM_IGNORED_BUILDS` で失敗します。
- アイコンは `tauri icon` で生成したプレースホルダです（Linux で使う PNG と
  `icon.ico` / `icon.icns` のみ残しています）。`app-icon.png` も**正式アートワークでは
  ありません**。正式なアートワークができたら `pnpm exec tauri icon app-icon.png` で
  再生成し、`icons/` と `app-icon.png` を差し替えてください。
- **CSP は設定済み**（`tauri.conf.json` の `app.security.csp`）:
  `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline';
  img-src 'self' data:; font-src 'self'; connect-src 'self' ipc: http://ipc.localhost;
  object-src 'none'; base-uri 'none'`。
  ローカル資産だけで動かし、外部 URL は読み込みません（`style-src 'unsafe-inline'` は
  React のインライン `style` 属性、`ipc:` / `http://ipc.localhost` は Tauri v2 の IPC 用）。
  外部リンクを開く処理は未実装です（P5 で OS のブラウザに渡す形で実装し、WebView 内では遷移しません）。
- **capability は `core:default` だけ**で、現時点では広めです（`core:default` は Tauri の
  基本 API をまとめて許可する）。P2 以降で用途別 capability に分割し、`fs` / `shell` /
  `http` プラグインは追加しません（[docs/TAURI_PLAN.md](../../docs/TAURI_PLAN.md) §3.2・§5）。

### デザイン実装のメモ

- **スタイルの正本は [docs/design/IMPORTED_STYLE_HYPERSTUDIO.md](../../docs/design/IMPORTED_STYLE_HYPERSTUDIO.md)**。
  色・タイポ・余白・角丸・コンポーネントの仕様と、Padbench への適用マッピング、
  実測コントラスト表があります。Refero / Hyperstudio は参考であり、提携・推奨関係はありません。
- **トークンの値は 4 か所で管理しています**（二重管理を避けられない構成です）:
  `src/styles/tokens.css`（色・寸法・モーションの値の正本）、`src/index.css` の `@theme`
  （タイポ・角丸・ブレークポイント・`--ease-standard` の値と、tokens.css の変数の写像）、
  `src/design/tokens.ts`（テストと参照用の写し）、`src/design/tokens.test.ts`（一致の検査）。
  値を変えるときは 4 か所を同時に直してください。`tokens.test.ts` は CSS と写しの一致、
  tokens.css の全変数が写しに存在すること、WCAG コントラスト（本文 4.5:1 / アイコン 3:1）、
  タイポ（weight 400・負のトラッキングは 34px 以上だけ）、角丸 4/8/99/9999、影なし、
  グラデーション禁止を検査します。
- `style-audit.test.ts` は実装ファイルを**クラス名と直書きの両方**で走査します
  （影・グラデ・色付きの塗り・角丸・アイコン色・weight 500 以上のほか、任意値ユーティリティ
  `p-[13px]` / `bg-[#...]`、tokens 以外の HEX・`rgb()`、許可リスト外のインライン `style`、
  直書きの `font-weight` を検出）。検査は文字列・正規表現ベースで、ビルド後の CSS や
  実描画までは見ません。
- **色**: `obsidian` `#101010`（地）/ `carbon` `#080808`（ダイアログ・トースト・ステージ）/
  `graphite` `#212121`（1px 境界）/ `iron` `#474747`（二次境界）/ `smoke` `#9c9c9c`（本文・メタ）/
  `ash` `#c1c1c1`（三次）/ `chalk` `#f3f3f3`（見出し）/ `signal-white` `#ffffff`（白ピル）/
  `compass-gold` `#6f6759`（アイコン線）/ `pulse-green` `#98ff38`（ライブのドットと
  入力モニターの軌跡線）。派生は `paper-ink` `#1a1a1a`（ステータスピル・選択行）と
  `danger` `#b42318`（危険の 1px 枠）だけです。
- **フォントは OS フォールバック**です。Inter / JetBrains Mono / IBM Plex Mono の woff2
  サブセットは未同梱で、`src/styles/tokens.css` に `TODO(fonts)` として `@font-face` の
  追加手順を残しています。日本語は Noto Sans JP を Inter の後ろに置きます。同梱するときは
  `apps/desktop/public/fonts/` に置き、`font-display: swap` と preload を付けてください。
- **用語は [docs/DESIGN_TAURI.md](../../docs/DESIGN_TAURI.md) §6.3 が正本**です。
  「入力モニター」「現在値」「記録」「書き込み対応版」「次にすること」を使い、
  `terminology.test.ts` が旧表記（ライブ入力 / 設定値 / 録音など）と絵文字の混入を検出します。
- **アイコンはインライン SVG の線画だけ**（16px・1.5 stroke）。色は `chalk` か
  `compass-gold` だけを使い、`smoke` などの副次色をアイコンに使いません。絵文字と
  アイコンフォントは使わず、ライセンス表記を増やさないためアイコンセットも導入していません。
- `pnpm test` は **CSS の `?raw` import を使うため `vite.config.ts` で `test.css: true`** に
  しています。
