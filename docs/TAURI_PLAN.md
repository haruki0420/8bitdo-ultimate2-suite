# Tauri v2 GUI 移行計画・クロスプラットフォーム方針

- 決定日: **2026-11-23**（PR #32 で P0 / P1 まで main にマージ済み、P2 以降未着手）
- 正本関係: GUI の実装方式・移行手順は**本書**、デザイン・トークン・日本語用語集は [DESIGN_TAURI.md](DESIGN_TAURI.md)（現行スタイルは [design/IMPORTED_STYLE_HYPERSTUDIO.md](design/IMPORTED_STYLE_HYPERSTUDIO.md)）、配布は [DISTRIBUTION_POLICY.md](DISTRIBUTION_POLICY.md)、レビューは [REVIEW_TIERS.md](REVIEW_TIERS.md)。

## 1. 決定事項と背景

デスクトップ GUI は **Tauri v2（Rust backend + TypeScript / Vite / React / Tailwind CSS、pnpm）** でゼロベースに作り直す。既存の egui 版（`crates/eightbitdo-gui`）は比較・撤退用として凍結し、`eightbitdo-protocol` / `eightbitdo-core` / `eightbitdo-cli` のロジックはそのまま再利用する。

- **移行理由**: CSS によるデザイン品質・DevTools での反復速度・表現幅・Tauri v2 モバイルでの UI 共通化を優先する。
- **手放す egui の利点**: 単一 Pure Rust バイナリ、WebKitGTK 非依存、IPC 境界のない即時描画・デバッグ簡便性。
- **変えない原則**: 配布/ライセンス方針、レビュー Tier、書き込み安全機構（pre-write backup / read-back / TOCTOU / 確認ダイアログ）、プロトコル仕様。

## 2. アーキテクチャとセキュリティ境界

- **責務分離**:
  - **Web UI（`apps/desktop/src`）**: 表示と入力のみ。BLE・ファイル・OS 機能を直接呼ばない。状態は Rust 側を正とする。
  - **Tauri IPC**: 要求/応答は commands、連続入力・進捗は events（20ms 入力通知は Rust 側で最新値に間引いて emit）。
  - **Rust backend（`apps/desktop/src-tauri`）**: **ルート Cargo workspace に含めない独立クレート**（WebKitGTK 依存と MSRV 1.90+ を既存 5 クレートから分離）。`eightbitdo-core` のセッション・per-user BLE ロック・keepalive・書込安全検証を保持する。
- **セキュリティ境界**:
  - capability は最小限とし、P2 以降に用途別へ分割する（現状 P0 は `core:default`）。汎用 BLE コマンドは作らない。
  - **`fs` / `shell` / `http` プラグインは追加しない**（ファイル選択は Rust 側ダイアログに限定、HTTP は Rust 側で実行）。capability 拡張は **Tier 3**。
  - **CSP（設定済み）**: `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self' ipc: http://ipc.localhost; object-src 'none'; base-uri 'none'`。外部 URL を読み込まず、外部リンクは OS ブラウザで開く。
  - 書き込み確認済みフラグを IPC で受け取らず、Rust 側で再検証する。

## 3. フェーズ計画（P0〜P8）

| P | 内容 | 状態 / 主な成果物・完了条件 | Tier |
| --- | --- | --- | --- |
| **P0** | 決定・足場 | **完了（#32）**。`apps/desktop` 雛形、独立 `src-tauri`、CSP、`desktop.yml`、バージョン固定（Node `^20.19.0 \|\| >=22.12.0` / pnpm 12.22.0 / Rust 1.90+） | 2 |
| **P1** | デザイン | **完了（#32）**。Hyperstudio スタイル採用、トークン・共通コンポーネント 11 種・シェル・静的 2 画面 | 2 |
| **P2** | デバイス / 入力モニター | 未着手。BLE 読取移植・入力モニター events・ログ。スティック Y 軸は `Y_SIGN` 定数で切替可能にし実機確認後に固定 | 2–3 |
| **P3** | 設定編集 / リマップ | 未着手。設定タブ・22 ボタンリマップ・プロファイル切替・backup / read-back / TOCTOU | **3** |
| **P4** | マクロ | 未着手。20ms 記録・ステップ編集・公式書込順序維持・JSON / TOML I/O | **3** |
| **P5** | 書込安全機構横断化 | 未着手。確認ダイアログ・二重送信防止・operation lock・記録中書込禁止・切断 fail-closed・expected SHA-256 | **3** |
| **P6** | ライセンス / i18n | 未着手。MIT/Apache + 同梱フォント全文画面、未翻訳キー 0、日英切替、`THIRD_PARTY_LICENSES.md` 同期 | 2 |
| **P7** | パッケージ / CI | 未着手。Linux AppImage/deb、Windows MSI/NSIS、macOS dmg（資格情報不在時は未署名と明記） | 2–3 |
| **P8** | Tauri モバイル | 未着手。Android / iOS 初期化 + ネイティブ BLE プラグイン（Kotlin / Swift）から `Transport` 注入 | **3** |

## 4. クロスプラットフォーム方針

| OS | UI / BLE | 配布形式 | 本ホスト検証可否・代替 |
| --- | --- | --- | --- |
| Linux x86_64 | WebKitGTK / `btleplug` (BlueZ) | AppImage / deb | **可能**（実機 BLE 証跡は Linux のみ） |
| Windows 10/11 | WebView2 / `btleplug` (WinRT) | MSI / NSIS | 一部（`cargo check` のみ、Rust **1.88+** 必須） |
| macOS | WKWebView / `btleplug` (CoreBluetooth) | dmg（署名・公証） | **不可**（SDK 不在）。実機未検証 |
| Android | Tauri WebView / Kotlin BluetoothGatt | APK / AAB | 未検証。実機確認まで既存 UniFFI + Compose 版を維持 |
| iOS | Tauri WKWebView / Swift CoreBluetooth | IPA | **不可**（macOS + Xcode 必須）。既存 UniFFI + SwiftUI 版を維持 |

> 未確認の OS・実機挙動は断定せず「未検証」と明記し、確認後に本書と [CROSS_PLATFORM.md](CROSS_PLATFORM.md) へ追記する。

## 5. リスク・受け入れ条件・未決定事項

- **主なリスクと対策**: WebView OS 差（標準 CSS/DOM のみ使用）、Linux WebKitGTK 依存（AppImage 同梱・問題時は deb / egui 退避）、モバイル BLE プラグイン工数（P8 を最後に配置）、egui 二重保守（P2 以降 egui 凍結）、入力モニター性能（Rust 間引き + `requestAnimationFrame` + canvas）。
- **受け入れ条件（目標値）**: egui 6 タブの機能一致、8 状態定義、キーボード完結・WCAG AA・200% ズーム耐性、[DESIGN_TAURI.md](DESIGN_TAURI.md) 用語集違反 0・未翻訳 0、操作応答 100ms 以内・入力遅延 100ms 以下・60fps、全書込経路での backup / read-back / TOCTOU / operation lock / fail-closed。
- **未決定事項**: egui 版の削除時期、Tauri updater の採否、macOS / Windows コード署名資格情報の整備時期、P8 後の `mobile/` UniFFI 版の扱い。
