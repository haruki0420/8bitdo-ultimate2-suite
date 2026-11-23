# Padbench ロードマップ

対象は **8BitDo Ultimate 2 Bluetooth Controller**（内部 PID `0x600F` / 2.4G レシーバー `0x6011`）。公式ソフト（8BitDo Ultimate Software V2）と同等の機能を実現するまでの進捗管理用です。

- **前提**: 実機確認済みは **Linux（BlueZ・BLE `"80ND"`・MTU 200）** の一部機能のみ。Windows / macOS / Android / iOS 実機、USB / 2.4G 経路、BLE OTA 書込は未検証です。
- **方針**: デスクトップ GUI の正式実装は **Tauri v2**（`apps/desktop`）、egui 版（`crates/eightbitdo-gui`）は凍結。未確認仕様は証跡が揃ってから実装・公開します。

## マイルストーン全体像

| M | ゴール | 現在の状態 |
| --- | --- | --- |
| **M1** | デスクトップ版（Linux BLE）の普段使い完成 | 進行中（core / CLI・Tauri v2 骨格・P2 読取 BLE 完了） |
| **M2** | Windows / クロスプラットフォーム（BLE 版）の実機検証・配布 | 未着手（Windows は `cargo check` のみ） |
| **M3** | BLE OTA と追加 RE が必要な公式機能 | 準備完了（OTA は core のみ・btsnoop 待ち） |
| **M4** | Android / iOS 実機フル機能 ＆ USB / 2.4G 対応 | 一部先行（UniFFI 雛形・APK / unsigned IPA ビルドまで） |

## M1: デスクトップ版（Linux BLE）の普段使い完成

### 完了済み
- [x] `eightbitdo-protocol` / `eightbitdo-core` / `eightbitdo-cli`（2768B 設定読み書き・リマップ・プロファイル切替 `0x0015`・マクロ読み書き/復元・`session` REPL・per-user BLE 単一所有ロック）
- [x] Tauri v2 GUI 骨格（`apps/desktop` P0 足場・P1 初期デザイン・独立 `src-tauri`・capability / CSP・`desktop.yml`）
- [x] Tauri v2 P2 読取 BLE（スキャン・接続・バージョン・2768B 設定読取・入力モニター events）とスティック Y 軸符号 `Y_SIGN = -1` 実機確定

### 進行中・残作業
- [ ] デザイン監修（ライト/ダーク切替検討・コントローラ線画・日本語 UI 刷新・`design-review` / `impeccable` 点検）
- [ ] 設定概要・ログ・ライセンス全文画面（MIT / Apache-2.0 全文 + 同梱 5 フォント全文・著作権者行）
- [ ] P3 設定編集・22 ボタンリマップ・プロファイル切替（`0x0015`）
- [ ] P5 書込安全機構の横断適用（確認ダイアログ・二重送信防止・operation lock・記録中書込禁止・切断時 fail-closed・expected SHA-256・pre-write backup・読み戻し検証）
- [ ] P4 マクロ編集（20ms 記録・ステップ編集・公式順序維持・JSON / TOML 入出力）
- [ ] 小機能: プロファイル Copy / Delete、GUI バックアップ復元、24 LED 個別色編集、振動テスト、英語切替（i18n）
- [ ] Linux 実機検証（Tauri GUI 書込、`config set` 各項目、M3/M4・S2/S3、Switch 側リマップ実効・マクロ発動）

## M2: Windows / クロスプラットフォーム（BLE 版）

- [ ] Windows 10/11 での Tauri v2 ビルド確認（WebView2 + `btleplug` WinRT、Rust 1.88+）
- [ ] Windows target のテスト警告 4 件（Unix 専用テストヘルパー）整理と `clippy -D warnings` 通過
- [ ] Windows 実機での BLE 動作確認（読取・入力モニター・書込・排他ロック）
- [ ] 配布パッケージと CI matrix（未署名時はその旨明記）: Linux AppImage/deb、Windows MSI/NSIS、macOS dmg（環境整備後）

## M3: BLE OTA と追加 RE が必要な公式機能

- [ ] 実機 btsnoop 採取（公式 Android アプリ最優先、不可理由記録時のみ専用個体で Phase B + `ota-bench`）で ACK 形状・アドレス規約・フレーム長・MTU・Jump を確定
- [ ] `eightbitdo-core` OTA 残課題解消（ACK `offset`/`total_len` 照合、マルチセクション `.dat`、バッテリー・ダウングレード判定）
- [ ] 公式 FW 取得 API クライアント（`POST /firmware/select`）と CLI / GUI / UniFFI 配線・`"8bt"` 復旧フロー
- [ ] 追加 RE 項目: 空マクロ新規作成、マクロ実行/ラウンドモード格納先、LED 輝度/効果/左右同期、ターボ連射レート（`Turbo_3`/`AutoTurbo`）、キャリブレーション、バッテリー `%` スケール・充電ビット、キーボードキー割当可否、`slot_flags`/リマップ `flag` 意味、`0x0015` 永続性・実効・ACK 再現性

## M4: モバイル実機フル機能 ＆ USB / 2.4G

- [ ] Android: Tauri v2 モバイル + Kotlin `BluetoothGatt` プラグイン（代替: Compose + UniFFI 拡張）、Android 12+ 権限・MTU・実機読み書き検証、APK / AAB 整備
- [ ] iOS: macOS + Xcode ビルド確認（Tauri v2 モバイル + CoreBluetooth または SwiftUI + UniFFI）、iPhone 実機検証、TestFlight / App Store 準備
- [ ] USB / 2.4G: USB App Mode（TX `[0x81,0x04]` / RX `[0x02,0x04]`、Save `0x0123`）実機証跡、USB Boot TX フレーミング追加 RE（未確定のまま書込実装しない）、`hidapi`（`hid.rs`）と Linux udev ルール整備・実機検証

## 正本ドキュメント

- [IMPLEMENTATION_PLAN.md](IMPLEMENTATION_PLAN.md) / [FEATURE_MATRIX.md](FEATURE_MATRIX.md) / [TAURI_PLAN.md](TAURI_PLAN.md) / [MOBILE_NATIVE_PLAN.md](MOBILE_NATIVE_PLAN.md) / [CROSS_PLATFORM.md](CROSS_PLATFORM.md) / [OTA_VERIFICATION_PLAN.md](OTA_VERIFICATION_PLAN.md)
