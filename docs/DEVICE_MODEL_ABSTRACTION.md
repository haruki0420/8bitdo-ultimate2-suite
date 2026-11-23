# 機種抽象化（Device Model Abstraction）設計

- 目的: 8BitDo Ultimate 2 Bluetooth（`0x600F`）に固定されている現行実装を、別機種（Ultimate 2 Wireless `0x6012` / Ultimate 2C `0x301A` / Pro 2 等）へ拡張できる構造へ移行する設計（PR #29 で main にマージ済み。実装コードは含まない）。
- 正本関係: プロトコル値は [PROTOCOL_SPEC.md](PROTOCOL_SPEC.md)、BT2 構造体は [STRUCT_BT2_2768B.md](STRUCT_BT2_2768B.md)、機能状況は [FEATURE_MATRIX.md](FEATURE_MATRIX.md)、全体フェーズは [IMPLEMENTATION_PLAN.md](IMPLEMENTATION_PLAN.md) §5。本書は機種をまたぐ構造と移行手順のみを定義する。
- 用語: 本書で「プロファイル」は機種 1 つ分の不変データ（`DeviceProfile`）を指し、コントローラ内のユーザー設定 1 個（`cur_slot` 0〜2）は**設定スロット**と書く。断定できない事項は「要検証」と記す。

## 1. 設計の要約と原則

1. **データ中心の抽象化**: 機種ごとの `trait Device` 実装は作らず、`&'static DeviceProfile`（静的レジストリ `const PROFILES: &[&DeviceProfile]`）に「識別・レイアウト・コマンド集合・能力」の 4 軸を集約する。アルゴリズム差がある箇所だけ小さな enum（`ConfigCodec` 等）で分岐する。
2. **能力は三値（`Supported` / `Unsupported` / `Unverified`）**: `bool` にしない。「推測実装禁止」を型とテストで強制し、`Unverified` は UI で理由付き無効表示にする。
3. **境界は capability descriptor**: UI（Tauri v2）・CLI・モバイル（UniFFI）には `DeviceProfile` を直接渡さず、`serde` 可能な所有型 descriptor（`schema_version: 1`）を渡す。UI は汎用フォームを自動生成せず、既存機能モジュールの表示・有効化だけをフラグで切り替える。
4. **抽象化しないもの（YAGNI）**: 16B ヘッダ、2 種の CRC 実装、`Transport` trait、`Session` の単一フライト / 再同期、BLE 単一所有者ロック、OTA 安全ゲート、ACK 待ち時間（機種差の証拠がないため共通層に残す）。
5. **型は共有、オフセットはデータ**: `Bt2Config` を機種ごとに複製しない。共有レコード型（`RumbleRecord` 等）を保ちつつ、`ConfigLayout` のセクション表からオフセットを引く。Ultimate 2 Wireless（1592B）とのレイアウト差はレガシー領域 A（1176B）の有無のみ（[STRUCT_BT2_2768B.md](STRUCT_BT2_2768B.md) §10）。

## 2. ハードコード棚卸し（base `81b0ac1`、非テストコード 53 件）

`crates/` 配下の機種依存箇所 53 件（参考掲載の `mock.rs` 1 件を除く）の分類と抽象化先:

| 分類 | 件数 | 主な箇所 | 抽象化先 |
| --- | --- | --- | --- |
| **L** レイアウト | 23 | `config_bt2.rs`（`CONFIG_SIZE=0x0AD0`、`OFF_*` 17 種、`Bt2Config`）、`macro_data.rs`、`ButtonIndex`、`session.rs`、`config_ops.rs`（`RESERVED_REGIONS`・`ConfigEdit`）、`macro_ops.rs`、CLI / GUI / UniFFI の DTO | `ConfigLayout`（セクション表） |
| **I** 識別 | 9 | `VENDOR_ID=0x2DC8`、`"80ND"` / `"8bt"`、`FF10`〜`FF23`、`ble.rs`、`UT2_BOOT_PID=0x600F`、CLI `--name`、GUI 接続文言 | `DeviceId` / `ProductId` / `BleIdentity` |
| **C** コマンド | 7 | `ReadVersion`（`0x000B` / `VERSION_ADDR=0x01017040`）、`macro_subcmd`、OTA アドレス、`GamepadType::Type1` | `CommandSet` |
| **F** 機能・件数 | 7 | `SLOT_COUNT=3`、`BUTTON_COUNT=22`、`MACROS_PER_SLOT=4`、GUI 6 タブ固定、backup manifest 定数 | `ConfigLayout` 件数 + `Capabilities` |
| **P** 製品名・文言 | 5 | crate doc、エラー文言、CLI `about`、GUI タイトル・既定バックアップ名 | descriptor + UI リソース |
| **T** タイミング | 2 | マクロチャンク上限（BLE 64B / USB 32B / 32B アライン）、CRC 2 種 | 共通層に残す |

## 3. `DeviceProfile` と周辺データ構造

### 3.1 構成要素

- **`DeviceProfile`**（`eightbitdo-protocol::device`、`&'static` 不変データ）:
  - `id: DeviceId`（安定 ASCII、例: `"bt2_2768b"`） / `display_name` / `vendor_id: u16` / `product_ids: &'static [ProductId]`（役割 `App` / `Boot` / `Receiver` / `OsEnumeration` 付き。1 機種が `0x600F` / `0x6011` / `0x310B` / `0x3208` の複数 PID を持つため配列化）
  - `ble: BleIdentity`（App / Boot の名前・service UUID・優先順 characteristic UUID 配列。`eightbitdo-protocol` に `uuid` 1.26.1 を追加し `serde` feature へ集約。1.29 系は MSRV 1.89 のため使わない）
  - `config: ConfigLayout`（`size`、`crc: ConfigCrc`（当面 `Mcrf4xx` のみ）、`slots`、`macros_per_slot`、`buttons`、`led_count`、`profile_name`、`sections: &'static [Section]`）
  - `commands: CommandSet`（設定 / バージョン / マクロ / スロット切替 `Option<u16>` / 入力 / `ota: Option<OtaCommands>` / `save: SaveCommands { ble, usb }` / `packet_crc`）
  - `capabilities: Capabilities` / `transports` / `detect` / `evidence`
- **セクション表の整合性**: 各 `Section`（`id`, `offset`, `element_size`, `stride`, `count`）の終端は `assert!(count > 0)` のうえ `offset + stride * (count - 1) + element_size` で計算し、全セクションが `[0, size)` を隙間・重複なく覆うことを全プロファイル横断テストで検証する。

### 3.2 能力三値と証跡規則

```rust
pub struct Capability {
    pub support: Support,                       // Supported | Unsupported | Unverified
    pub evidence: Option<&'static str>,         // 実装 + 実機の完全証跡
    pub re_evidence: Option<&'static str>,      // RE（静的解析）または非対応の確定根拠
    pub partial_evidence: Option<&'static str>, // 部分的な実機観測（Unverified のみ）
}
```

| `support` | `evidence` | `re_evidence` | `partial_evidence` | UI での扱い |
| --- | --- | --- | --- | --- |
| `Supported` | **必須** | 任意 | `None` | 表示・有効 |
| `Unverified` | **`None` 固定** | RE 済みなら `Some` | 部分観測のみ `Some` | 表示・無効（理由表示、書込不可） |
| `Unsupported` | `None` | **必須** | `None` | 非表示（または未対応表示） |

アンカー書式は `FILE#<節番号>`（例: `DEVICE_CONFIG_EVIDENCE#7`）に統一し、ファイルと見出し番号の実在をテストで検証する。新規プロファイルは全能力 `Unverified` を既定とする。

**BT2（`bt2_2768b`）の初期能力**:
- **`Supported`**: `macros`（S1 M1/M2、`FEATURE_MATRIX#10`）、`realtime_input`（`SESSION_OPERATIONS#4`）、`remap`（`FEATURE_MATRIX#2`）、`profiles`（読取、`DEVICE_CONFIG_EVIDENCE#1`）、`profile_switch`（`0x0015` 1 回観測、`DEVICE_CONFIG_EVIDENCE#7`）。
- **`Unverified`**: `profile_edit`、`gyro`、`led`、`special`、`turbo`、`rumble`、`stick_range`、`trigger_range`、`battery`（生値のみ部分観測、`partial_evidence`）、`ota`。

## 4. 検出パイプラインと未知デバイス

BLE App Mode には PID 読み出しコマンドが文書化されていないため、**名前一致だけで書込を開かない**。

1. `scan` の名前・サービス UUID で `PROFILES` を照合（0 件 → `unknown`、2 件以上 → `--model <DeviceId>` 指定がなければ曖昧エラーで停止）。
2. 接続後、`ReadVersion` → 初回 `ReadConfig` の応答 `total_len` を `profile.config.size` と照合（`confirm_layout`）。不一致なら **`eightbitdo_core::Error::LayoutMismatch`** で fail-closed し書込経路を開かない。
3. `--model` 上書きは検出の曖昧さ解消のみに使い、**`total_len` 形状検査は免除しない**。
4. **`unknown` プロファイル**: `info`（`ReadVersion`）のみ許可。`config` 読取はサイズ未確定で拒否し、書込系は **`eightbitdo_core::Error::UnsupportedModel`** で拒否する。

## 5. モジュール分離・外部露出・命名

- **配置と 1 行ルール**: **「機種名や PID が出るコードは `models/` にだけ置き、それ以外は `device` の語彙だけを使う」**（`grep` テストで検査）。
  - `eightbitdo-protocol::device`: 機種非依存の語彙・`PROFILES` レジストリ。
  - `eightbitdo-protocol::models::bt2_2768b`: BT2 定数・`config.rs`（旧 `config_bt2.rs`）・`ButtonIndex`（旧パスは re-export で互換維持）。
  - `eightbitdo-core::device`: プロファイル解決・`confirm_layout`・descriptor 生成。
- **外部露出**:
  - **JSON descriptor**: `schema_version: 1`、`profile_id`、`display_name`、`vendor_id`、`product_ids`（役割付き）、`transports`、`config`（サイズ・`crc`・件数）、`capabilities`（構造体・三値 + 証跡）。フィールド追加は version 据え置き、削除・意味変更で上げる。
  - **Tauri**: `#[tauri::command] fn device_capabilities(...) -> Result<String, String>` で JSON を返し、フロントは descriptor を 1 回取得して表示・有効化を制御する。
  - **UniFFI**: 既存 DTO は維持し、descriptor record を追加する。
  - **CLI**: `eightbitdo models`、`eightbitdo capabilities [--json]`、`eightbitdo --model <id>` を追加。`scan --check` の契約や既存終了コードは変えない。
- **命名**: クレート名 `eightbitdo-*` は当面維持する（アプリ表示名のみ **Padbench**）。将来 8BitDo 以外の実機対応が 2 機種以上揃った段階で、全面改名ではなく汎用ファサード（`padbench-core` 仮）を 1 枚足す。

## 6. BT2 移行計画（S1〜S8 / P0〜P4）とテスト戦略

原則: **移設と意味変更を分離**し、既存 API は re-export で維持、**既存テストを 1 件も書き換えない**（base `81b0ac1` 実測: protocol 100 unit + 4 doc、core 264 unit + 1 doc / all-features 273 + 1。各 PR ではその時点の base 実測値と比較）。

| Phase | 段階 | 内容 | レビュー Tier | 見積 |
| --- | --- | --- | --- | --- |
| **P0（完了）** | — | 棚卸し・設計（本書。コード差分ゼロ） | Tier 2 | 完了 |
| **P1** | **S1** | `protocol::device` 型定義 + `uuid` 1.26.1 追加（`PROFILES` 空、MSRV 1.85 緑） | Tier 2 | 1–2 日 |
| **P1** | **S2** | `models::bt2_2768b::PROFILE` 追加 + 既存定数との一致テスト | Tier 2 | 1–2 日 |
| **P1** | **S3** | `config_bt2` / `ButtonIndex` 移設 + re-export、`OFF_` 直接参照の撤去 | Tier 2 | 2 日 |
| **P1** | **S4** | `Session` のプロファイル保持 + `total_len` 不一致 fail-closed | **Tier 3** | 2–3 日 |
| **P2** | **S5** | `core::device` / descriptor + CLI `models`・`capabilities` + UniFFI 追加 | **Tier 3**（UniFFI 境界） | 2–3 日 |
| **P2** | **S6** | Tauri UI の capability 駆動（3 パターンテスト。egui 版は凍結で触らない） | Tier 2 | 3–4 日 |
| **P3** | **S7** | `docs/ADDING_A_MODEL.md` + `cfg(test)` ダミープロファイルの dry-run | Tier 2 | 1–2 日 |
| — | **S8** | 2 機種目（Ultimate 2 Wireless 1592B 候補。全能力 `Unverified` で開始） | **Tier 3** | 別 PR |
| **P4** | — | 他社機種・商標・配布ポリシー影響の調査のみ（実装なし） | **Tier 3**（法務・商標） | 2–3 日 |

- **横断テスト（`for profile in PROFILES`）**: ID / PID 一意性、セクション被覆（重複・隙間なし）、`stride != element_size` 終端計算、`bt2_layout_matches_constants`、descriptor round-trip、`capability_evidence_rules`、新規プロファイル既定 `Unverified`、未知プロファイル `info` のみ、`models/` 外の機種名 0 件、BT2 と Wireless のレイアウト差分一致。
- **ゴールデンベクタ**: 自分のエンコーダ出力を期待値にせず、実機キャプチャまたは RE 確定 wire を使用する（新規プロファイル追加時のみ `protocol/tests/golden/<profile_id>/` を使用）。
- **ロールバック**: S1〜S3 は追加 + re-export、S4 は既定 BT2 維持のため各段階で独立 revert 可能。S4 は移行前後の送信フレームバイト一致をテストで固定する。

## 7. 主なリスクと要検証事項

- **リスク**: 過剰設計（4 軸に限定）、未確認の推測実装（三値 + 証跡テスト）、UI 複雑化（汎用フォームを作らない）、同名・同サイズ別機種の誤選択（`total_len` + CRC / 予約領域検査で軽減するが同名同レイアウトの完全識別は原理的に限界あり）、re-export の恒久化（S8 完了時に削除 PR を予定）。
- **要検証事項（オープンクエスチョン）**:
  1. Ultimate 2C / Pro 2 / Ultimate 3 が App Mode 名 `"80ND"` を広告するか。
  2. BLE App Mode で機種確定できるコマンドが存在するか（公式アプリ btsnoop で確認）。
  3. Ultimate 2 Wireless（`0x6012`）の `key[22]` 順・LED 数・`slot_flags` が BT2 と同一か。
  4. 別機種で CRC・`macro_subcmd`・10B マクロステップ・OTA アドレスが異なるか。
  5. Tauri の descriptor 消費で追加要求が出るか、CLI `--name` 互換と `Padbench` バイナリ名移行の扱い。
