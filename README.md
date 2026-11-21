# 8BitDo Ultimate 2 Cross-Platform Suite

[![CI](https://github.com/haruki0420/8bitdo-ultimate2-suite/actions/workflows/ci.yml/badge.svg)](https://github.com/haruki0420/8bitdo-ultimate2-suite/actions/workflows/ci.yml)

**8BitDo Ultimate 2 Bluetooth Controller**（内部 PID `0x600F` / 2.4G レシーバー `0x6011`）向けのオープンソース・クロスプラットフォーム設定／マクロ編集／ファームウェアアップデートスイート（設計・仕様リポジトリ）。

Nintendo Switch へ Bluetooth Classic または 2.4G アダプターで接続したまま、**Linux / macOS / Windows / Android / iOS** から **BLE GATT（アドバタイズ名 `"80ND"`）** 経由で同時接続し、リアルタイムにマクロやプロファイルを編集・適用することを目指しています。

---

## 主な特徴（設計目標）

- **Switch 同時接続でのリアルタイム設定**: コントローラを Switch に繋いだまま Profile ボタンを 3 秒長押しし、PC やスマホから BLE GATT で接続・編集可能（USB / 2.4G HID 接続にも対応）。
- **高精度マクロエディタ**:
  - コントローラ実機入力からのリアルタイム録音（`cmd 0x0007` / `0x0003`）
  - **1ms 単位**のステップ時間編集、フル **8-bit アナログスティック／トリガー値**指定
  - JSON / TOML 形式でのマクロ・プロファイル インポート／エクスポート
- **2768 バイト (`0xAD0`) 設定構造体のフルサポート**:
  - プロファイルスロット 1〜3、ボタンリマップ（22 キー + 拡張 32-bit `KeyMap`）、スティック＆トリガーデッドゾーン、振動、6軸ジャイロ、RGB ライティング（Dynamic / HotWheel / Single）、マクロ
- **PC & モバイル対応ファームウェアアップデート**:
  - PC（BLE / USB）だけでなく、**Android / iOS からの BLE Bootloader OTA（`"8bt"`, `0000FF20`/`0000FF21`）** および Android USB OTG 更新に対応
- **クロスプラットフォーム・ネイティブ構成**:
  - 共通プロトコル＆コア: **Rust** (`eightbitdo-protocol`, `eightbitdo-core`, `eightbitdo-uniffi`)
  - PC (Linux / macOS / Windows): Pure Rust CLI + ネイティブ GUI（`egui`、`btleplug` + `hidapi`）
  - モバイル: **iOS (SwiftUI + CoreBluetooth)** / **Android (Jetpack Compose + Android BLE / USB OTG)**

---

## ドキュメント一覧

公式バイナリの静的解析から検証した完全仕様と実装計画を `docs/` にまとめています。

| ドキュメント | 内容 |
| --- | --- |
| **[docs/PROTOCOL_SPEC.md](docs/PROTOCOL_SPEC.md)** | デバイス VID/PID、BLE GATT UUID・MTU、16B カスタムヘッダ、BLE/USB HID フレーミング、2 種の CRC-16、全コマンド、リアルタイム入力ストリーム、BLE OTA ブートローダー＆ファームウェア API 仕様 |
| **[docs/STRUCT_BT2_2768B.md](docs/STRUCT_BT2_2768B.md)** | 2768 バイト（`0xAD0`）設定構造体 (`record_ultimatebt2`) の全バイトオフセット、32-bit `KeyMap`、10 バイト `MacroDatas` ステップ構造、Flash 書き込みシーケンス |
| **[docs/IMPLEMENTATION_PLAN.md](docs/IMPLEMENTATION_PLAN.md)** | Rust + SwiftUI + Jetpack Compose のアーキテクチャ設計、ワークスペース構成、Phase 1〜5 ロードマップ、安全検証手順 |
| **[AGENTS.md](AGENTS.md)** | AI エージェント／開発者向け引き継ぎガイド（[CLAUDE.md](CLAUDE.md) からも参照） |
| **[CONTRIBUTING.md](CONTRIBUTING.md)** | ブランチ運用、必須 CI、独立レビュー（Tier 1〜3 / SHA 紐づきの `review-gate` 成功）の手順 |
| **[docs/BRANCH_PROTECTION.md](docs/BRANCH_PROTECTION.md)** | `main` の canonical ruleset、`review-gate` の付け方・監査とゲートの限界 |
| **[docs/REVIEW_TIERS.md](docs/REVIEW_TIERS.md)** | リスクに応じた独立レビューの Tier 1〜3、レビュアーの役割、故障リストの書き方 |

---

## 現在のステータス

- `main` は **PR + CI + 独立レビュー（SHA 紐づきの `review-gate` 成功）** で保護済みです（`main-protection`, id `27785806`, 2026-11-21 検証）。レビュー深度は [docs/REVIEW_TIERS.md](docs/REVIEW_TIERS.md)、詳細は [CONTRIBUTING.md](CONTRIBUTING.md) と [docs/BRANCH_PROTECTION.md](docs/BRANCH_PROTECTION.md) を参照してください。
- [x] 公式アプリ（Android v2.01 / Windows v1.38）の通信プロトコル・バイナリ構造・OTA シーケンスの解析完了
- [x] 完全仕様書・実装計画書・AI 引き継ぎドキュメント作成
- [x] Phase 1: `eightbitdo-protocol`（Rust プロトコル・構造体・CRC・マクロ、単体テスト 74 件 + doc test 4 件）
- [ ] Phase 2: `eightbitdo-core` + `eightbitdo-cli`（BLE / USB 実機通信＆検証）
- [ ] Phase 3: `eightbitdo-gui`（デスクトップネイティブ GUI）
- [ ] Phase 4: BLE OTA / USB ファームウェアアップデーター
- [ ] Phase 5: `eightbitdo-uniffi` + iOS (SwiftUI) / Android (Jetpack Compose) アプリ

---

## 注意事項

- 本プロジェクトは非公式のオープンソースプロジェクトであり、8BitDo とは一切関係ありません。
- 著作権保護された公式ソフトウェアのバイナリ・ファームウェアファイル自体は本リポジトリに含まれていません。
