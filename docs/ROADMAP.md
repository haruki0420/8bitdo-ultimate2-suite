# Padbench ロードマップ（公式機能パリティまでの展望と TODO）

対象機種は **8BitDo Ultimate 2 Bluetooth Controller**（内部 PID `0x600F` / 2.4G レシーバー `0x6011`）です。本書は、公式 Windows / Android ソフト（8BitDo Ultimate Software V2）でできることを Padbench で実現するまでの全体計画と進捗チェックリストをまとめたものです。

- **本書の位置づけ**: 進捗管理と全体像の俯瞰用です。プロトコル仕様・設計・実機検証手順・配布方針の正本は末尾の「正本ドキュメント一覧」を参照してください。
- **実機検証の前提**: 現時点で実機確認済みなのは **Linux（BlueZ・BLE `"80ND"`・MTU 200）** の一部機能に限られます。Windows / macOS / Android / iOS の実機動作、USB / 2.4G 経路、BLE OTA のフラッシュ書き込みは未検証です。
- **方針**: デスクトップ GUI の正式実装は **Tauri v2**（`apps/desktop`）とし、既存の egui 版（`crates/eightbitdo-gui`）は比較・撤退用として凍結します。未確認の仕様や未検証の挙動は確定済みとして扱わず、証跡が揃ってから実装・公開します。

---

## マイルストーン全体像

| マイルストーン | ゴール | 現在の状態 |
| --- | --- | --- |
| **M1** | デスクトップ版（Linux BLE）の普段使い完成 | 進行中（コア / CLI・Tauri v2 骨格・P2 読み取り BLE 完了） |
| **M2** | Windows / クロスプラットフォーム（BLE 版）の実機検証・配布 | 未着手（Windows は `cargo check` のみ確認済み） |
| **M3** | BLE ファームウェア更新（OTA）と追加 RE が必要な公式機能 | 準備完了（OTA は core のみ実装・実機 btsnoop 待ち） |
| **M4** | Android 実機フル機能 ＆ USB / 2.4G 有線・ドングル対応 | 一部先行（UniFFI 雛形・APK / unsigned IPA ビルド確認まで） |

---

## M1: デスクトップ版（Linux BLE）の普段使い完成

Linux 環境で BLE 経由の設定変更・リマップ・マクロ編集を日常的に安全に使える状態にします。

### 完了済み

- [x] 共通プロトコル・コア・CLI の構築（`eightbitdo-protocol` / `eightbitdo-core` / `eightbitdo-cli`、2768 バイト設定の読み書き・リマップ・プロファイル切替 `0x0015`・マクロ読み書き/復元・常駐 `session` REPL・per-user BLE 単一所有ロック）
- [x] Tauri v2 デスクトップ GUI の骨格構築（`apps/desktop` の P0 足場・P1 初期デザイン・独立 `src-tauri`・capability / CSP・`.github/workflows/desktop.yml`）
- [x] Tauri v2 P2 読み取り BLE 実装（スキャン・接続・バージョン・2768 バイト設定読取・入力モニター events）およびスティック Y 軸の符号 `Y_SIGN = -1` 実機確定

### 進行中・直近の作業

- [ ] デザイン監修の実施（ライトテーマ既定＋ダークテーマ切替、コントローラ線画 SVG、日本語 UI 刷新、`design-review` / `impeccable` による点検）
- [ ] 設定概要・ログ・ライセンス全文画面の実装（アプリの MIT / Apache-2.0 全文および同梱 5 フォントの全文・著作権者行の表示）
- [ ] P3 設定編集・22 ボタンリマップ・プロファイル切替（`0x0015`）の Tauri 版実装
- [ ] P5 書き込み安全機構の横断適用（確認ダイアログ・二重送信防止・operation lock・記録中の書き込み禁止・切断時の fail-closed・expected SHA-256 照合・pre-write backup・読み戻し検証）
- [ ] P4 マクロ編集機能の Tauri 版実装（20 ms 記録・ステップ編集・Header → Save → Erase → Write → Save の書き込み順序維持・JSON / TOML 入出力）

### 残りの小機能・実機検証

- [ ] プロファイル Copy / Delete の `ConfigEdit` 化および CLI / GUI 露出（`Bt2Config::copy_slot` / `delete_slot` の配線）
- [ ] GUI バックアップ復元（Restore）UI の実装（設定・マクロのファイル読み込みと安全フロー経由の復元）
- [ ] 24 LED 個別色編集 UI の実装（`Single_Theme.colors[24]` の個別編集と左右対応の実機確認）
- [ ] 振動テスト UI の実装
- [ ] 英語切替（i18n）の実装（未翻訳キー 0 件・用語集との整合確認）
- [ ] 設定・マクロ書き込みの Linux 実機検証（Tauri GUI からの書き込み、`config set` 各フィールド、M3 / M4・S2 / S3、Switch 側でのリマップ実効とマクロ発動確認）

---

## M2: Windows / クロスプラットフォーム（BLE 版）の実機検証・配布

Linux 以外のデスクトップ OS で Tauri v2 アプリをビルド・検証し、利用者が導入できる配布パッケージと CI を整えます。

- [ ] Windows 10 / 11 での Tauri v2 ビルド確認（WebView2 + `btleplug` WinRT バックエンド。Windows の実効 MSRV は Rust 1.88 以上）
- [ ] Windows target のテスト警告 4 件（`eightbitdo-core` の Unix 専用テストヘルパー）を整理し、`--all-targets` で `clippy -D warnings` を通過させる
- [ ] Windows 実機での BLE 動作確認（スキャン・接続・読み取り・入力モニター・書き込み・排他ロックの挙動）
- [ ] 配布パッケージ生成と CI matrix の整備（署名・公証の資格情報が無い間は未署名ビルドを成果物とし、署名の有無を明記する）
  - [ ] Linux: AppImage / deb（WebKitGTK 依存の明記と起動確認）
  - [ ] Windows: MSI / NSIS（インストールと起動確認）
  - [ ] macOS: dmg（macOS 環境・ランナー整備後に WKWebView + CoreBluetooth のビルド・実機確認・署名/公証を実施）

---

## M3: BLE ファームウェア更新（OTA）と追加 RE が必要な公式機能

未確定の wire 仕様と公式バイナリの追加解析（RE）を確定させたうえで、BLE OTA と残りの公式機能を実装します。未確認の値は推測で実装しません。

### BLE ファームウェア更新（OTA）

- [ ] 実機 btsnoop 採取による BLE OTA wire の確定（公式 Android アプリ + HCI スヌープログを最優先で試行し、不可理由を記録できた場合のみ専用個体で Phase B + `ota-bench` チェックポイントを実行）
  - [ ] ACK 形状（ready ACK / 完了 ACK の区別と `offset` / `total_len`）の確定
  - [ ] Erase / Write のアドレス規約・`chunk_len`・`WriteFirmware` / `WriteVersion` フレーム長・Bootloader MTU・Jump 挙動の確定
- [ ] `eightbitdo-core` OTA の残課題解消（ACK の `offset` / `total_len` 照合、マルチセクション `.dat` 対応、バッテリー・ダウングレード判定）
- [ ] 公式ファームウェア取得 API クライアント（`POST /firmware/select`）と整合性検証の実装
- [ ] BLE OTA の CLI / GUI（Tauri 版ファームウェア更新画面）/ UniFFI への配線および中断時の `"8bt"` ブートローダー復旧フロー実装

### 追加 RE が必要な公式機能

- [ ] 空マクロの新規作成（erase + step data clear の手順確定と実装）
- [ ] マクロ実行モード（Run Once / Hold to Run / Click Start/Stop / Click to Replay）およびラウンドモード（Single / Repeated / Multiple / Infinite）の格納先確定と UI 実装
- [ ] LED Brightness・ライティング効果名（Star / Breathing 等）・左右同期の格納先確定と実装
- [ ] ターボ連射レート / 3 段階（`Turbo_3` / `AutoTurbo`）のセマンティクス確定と専用 UI の要否判断
- [ ] キャリブレーションの保存先・コマンド特定と実装要否の判断
- [ ] バッテリー残量 `%` のスケール（0–100 / 0–255）と充電状態ビットの確定および UI 表示
- [ ] キーボードキー割当が `0x600F` の `key[22]` で有効かどうかの確定（非対応なら N/A として記録）
- [ ] `slot_flags`・リマップ `flag` の意味確定および `0x0015` プロファイル切替の再接続・電源断を跨ぐ永続性・実効・ACK 形状再現性の確定

---

## M4: Android 実機フル機能 ＆ USB / 2.4G 有線・ドングル対応

モバイル実機でのフル機能利用と、公式 Windows ソフトの主経路である USB / 2.4G HID 接続を実現します。

### Android

- [ ] Tauri v2 モバイル＋Kotlin `BluetoothGatt` プラグインから `eightbitdo-core` へ `Transport` を注入する構成の実装（代替案として既存 Jetpack Compose + UniFFI 版へ全フィールド読取・設定書込・リマップ・マクロ・OTA を全面移植）
- [ ] Android 12 以降の権限（`BLUETOOTH_SCAN` / `BLUETOOTH_CONNECT`）および MTU 交渉・ライフサイクル制御の実機確認
- [ ] Android 実機での BLE スキャン・読み取り・設定/マクロ書き込み検証
- [ ] APK / AAB 配布パッケージの整備

### iOS

- [ ] macOS + Xcode 環境でのビルド確認（Tauri v2 モバイル＋Swift `CoreBluetooth` プラグイン、または代替の SwiftUI + UniFFI 版）
- [ ] iPhone 実機での BLE 動作確認および署名・TestFlight / App Store 配布準備

### USB / 2.4G（有線・ドングル）

- [ ] USB App Mode（TX `[0x81, 0x04]` / RX `[0x02, 0x04]`、Save `0x0123` 等）の実機証跡採取
- [ ] USB Boot TX フレーミングの追加 RE（証跡が確定するまで USB OTA 書き込みは実装しない）
- [ ] `hidapi` トランスポート（`crates/eightbitdo-core/src/hid.rs`）の実装および Linux udev ルール（`packaging/linux/`）の整備
- [ ] USB / 2.4G 経由の設定・マクロ読み書きおよび USB OTA の実装・実機検証

---

## 正本ドキュメント一覧

個別の詳細仕様・設計判断・検証記録は以下を正本とします。

- [IMPLEMENTATION_PLAN.md](IMPLEMENTATION_PLAN.md) — 全体アーキテクチャ、クレート構成、Phase 1〜5 の実装状況と残課題
- [FEATURE_MATRIX.md](FEATURE_MATRIX.md) — 公式 Windows / Android ソフトとの機能比較表、RE 証拠・実装・実機検証ラベル
- [TAURI_PLAN.md](TAURI_PLAN.md) — Tauri v2 GUI 移行計画（P0〜P8）、セキュリティ境界、クロスプラットフォーム方針、日本語 UI 用語集
- [MOBILE_NATIVE_PLAN.md](MOBILE_NATIVE_PLAN.md) — モバイルネイティブ版（Android / iOS）の機能ギャップ監査、UniFFI API 契約案、工数・実装順序
- [CROSS_PLATFORM.md](CROSS_PLATFORM.md) — Linux / Windows / macOS / モバイルのビルド・コンパイル確認結果と制約
- [OTA_VERIFICATION_PLAN.md](OTA_VERIFICATION_PLAN.md) — BLE OTA 実機 btsnoop 採取手順、Phase A / Phase B の安全条件、判定項目
