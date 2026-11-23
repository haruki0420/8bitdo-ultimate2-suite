# 公式機能カバレッジ・マトリクス（Ultimate 2 Bluetooth / `0x600F`）

- 基準: `main` `c169392`（2026-11-23、PR #17・#26 反映）。比較対象は公式 Windows V1.38（`8BitDoAdvance.dll`）と Android V2.01（`classes.dex` / `libadvancelib.so`）の静的解析。
- 実機検証環境は **Linux（BlueZ）・BLE `"80ND"`・MTU 200・74B chunk のみ**。他 OS・USB / 2.4G は未検証。
- 原則: **RE 未確認のビット・レンジ・セマンティクスを「実装済み」「実機確認済み」と書かない**。

## 1. ラベル定義

- **RE 証拠**: **RE 確認済み**（UI 文言＋バイナリ関数/構造体の両方確認）/ **推測**（片方のみ）/ **未確認**（証拠なし・追加 RE 待ち）/ **Web 情報のみ**。
- **実装状況**: 書き込みは安全機構（pre-write backup / CRC 再計算 / 予約領域保持 / Save / read-back / TOCTOU）を通る前提。
- **実機検証**: **実機確認済み（Linux BLE）** / **実機未検証** / **実機評価不能**（下位 RE 未確定）。

## 2. 機能領域別マトリクス

| 機能領域 | 公式機能・RE 証拠 | 実装状況（core / CLI / egui GUI） | 実機（Linux BLE） | 主な残作業・追加 RE |
| --- | --- | --- | --- | --- |
| **ボタンリマップ** | 22 ボタン（`+0x0E4`、`KeyMap`、P1=PL / P2=PR / P3=L4 / P4=R4、無効=`0`、227 は用途未確認）、特殊機能 13 ビット（一部推測）、マクロ起動キー: **RE 確認済み**。`flag` 実機値 `0x20200911`/`0xFFFFFFFF` は **未確認** | core + CLI（`config remap` / `map show`）+ GUI（22 ボタン + 特殊機能 13 ビット）。CLI `config set` に `special.*` なし。227 は拒否 | **確認済み（書込・read-back のみ）**: `remap A B`→`B A`。実押下出力変化は未確認 | 実押下ビット観測、`flag` 意味確定、CLI `special.*` 追加 |
| **スティック** | 左右デッドゾーン（`JoyRecord` `+0x98`、start 0..127 / end 0..128）: **RE 確認済み**。感度カーブ（`+0x1F4..+0x68C` 領域?）は **未確認**、キャリブレーションは **推測** | core + CLI（`joy.*`、型付きは `flag=1` 副作用）+ GUI | 実機未検証 | 実機デッドゾーン効果確認、カーブ・キャリブレーション RE |
| **トリガー** | L/R 範囲（`TriggerRecord` `+0xB0`）、L2/R2 入替（`Special` bit7）: **RE 確認済み**。ヘアトリガーは **推測**、アナログ/デジタル切替は **未確認**（物理スイッチのみの可能性） | core + CLI（`trigger.*`）+ GUI | 実機未検証 | 実機範囲効果確認、ヘアトリガー / 切替可否 RE |
| **振動** | 左右モーター強度 0〜5 段階（`RumbleRecord` `+0x74`、f32 0.0〜1.0）: **RE 確認済み**。振動テスト・本体/トリガー有効化ビットは **推測** | core + CLI（`rumble.level`）+ GUI。振動テスト UI 未実装 | 実機未検証 | 実機強度確認、振動テスト UI |
| **ジャイロ / 6 軸** | `SixAxisRecord`（`+0x92C`、Hold/Toggle・有効ボタン・感度 1..15・補償 0..255・マップ先 L/R/Mouse）: **RE 確認済み**（Windows 準拠。Android `offset=0` バグは非踏襲）。Switch 限定制約は **Web 情報のみ** | core + CLI（`gyro.*`）+ GUI | 実機未検証 | 実機ジャイロ動作確認、`gamepad_mode` 依存確認 |
| **LED / RGB** | Dynamic (`+0x950`) / HotWheel (`+0x974`, speed 0..15) / Single (`+0x9A4`, 24×ARGB): **RE 確認済み**。Brightness・左右同期・OFF は **未確認**、効果名・Windows Dynamic Lighting は **推測** | core + CLI（`led.*`、Single は 24 LED 同色一括）+ GUI | 実機未検証 | 24 LED 個別 UI、Brightness / 効果名 / 左右同期 RE |
| **プロファイル** | 名前（`+0x14` UTF-16BE 32B）・`cur_slot`（`+0x12`）・`0x0015` 切替・Copy/Delete: **RE 確認済み**。`slot_flags` は **未確認**（opaque） | 名前・`gamepad_mode`・`cur_slot`: core + CLI + GUI。`0x0015`: core + CLI (`config profile`) + REPL (`profile switch`)、Save なし。Copy/Delete: protocol ヘルパのみ | **`0x0015` 一部確認**: ACK 16B 封筒（1 回）と `cur_slot` 6→1 確認（永続性・実効・再現性は未確認） | `0x0015` 永続性・実効確認、Copy/Delete 露出、GUI 切替 UI、`slot_flags` 確定 |
| **ターボ / 連射** | `Turbo` (bit16) / `AutoTurbo` (bit24) / `Turbo_3` (bit23): **RE 確認済み**。連射レート・3 段階の意味は **未確認** | リマップ先として core + CLI + GUI に実装。専用 UI なし | 実機未検証 | レート / 3 段階 RE、実機連射確認 |
| **マクロ** | `Macro_Pro3_Struct`（`+0x68C`、4×52B）、10B/step（最大 200）、`0x0102/0x0103/0x0104` + Save: **RE 確認済み**。空マクロ作成・実行 4 モード・ラウンドモードは **未確認** | core + CLI（backup/write/restore、JSON/TOML）+ GUI（20ms 録音・編集・書込）。空マクロ作成は拒否。ヘッダ `total_len=0xD8` 既定 | **確認済み（S1 の M1/M2）**: 書込・read-back・復元一致。Switch 側発動は未確認 | M3/M4・S2/S3、Switch 発動確認、空マクロ作成・実行モード RE、GUI restore / JSON I/O |
| **OTA** | `ReadVersion` (`0x000B`)・コマンド列は **RE 確認済み**。wire 詳細は **未確認**（[PROTOCOL_SPEC.md](PROTOCOL_SPEC.md) §8.5）。USB OTA は TX 未文書化で **未確認** | Version 読取は実装済み。BLE OTA は **core のみ・未配線**（`allow_unverified_wire=false`、`ota-bench` あり）。FW 取得 API 未実装 | Version 読取（`0xC8`）確認済み。OTA は評価不能 | btsnoop 採取、ACK `offset`/`total_len` 照合、マルチセクション `.dat`、配線 |
| **backup / restore** | 公式明示 UI 未確認（Copy/Delete 相当）。本リポジトリ独自で pre-write backup 実装 | CLI: config / macro の backup & restore。GUI: backup のみ（restore 未実装） | CLI backup / restore 確認済み | GUI restore UI、JSON/TOML I/O |
| **USB / 2.4G** | USB App Mode は **推測**、USB Boot TX は **未確認** | **未実装（保留）** | 評価不能 | USB App Mode 実機証跡、USB Boot TX RE |
| **その他** | バッテリー生値（`0x0003` `+0x0A`、スケール・充電ビット **未確認**）、Polling Rate は **`0x600F` 非対応確定**、`gamepad_mode` 書換挙動・キャリブレーション・キーボード割当は **未確認 / 推測** | バッテリー生値表示、`gamepad_mode` 書込のみ | バッテリー生値表示のみ確認 | スケール・充電ビット・キャリブレーション・キーボード割当可否 RE |

## 3. 実装構造の根拠と優先残作業

- **書き込み基盤（`config_ops.rs`）**: 部分書込は使わず常に 2768B 全量 `0x0001`（`min(MTU, 74)`）→ BLE Save（`0x0006/0x0067`）→ read-back 検証。PR #17 で raw flag 観測値集合（`0`/`1`/`0x20200911`/`0xFFFFFFFF`、gyro は `0x20190000` も）と完全消去 slot を受理。
- **モバイル（`eightbitdo-uniffi`）**: 読取 API + ローカル検証（`prepare_config_write`）のみ公開。書込・OTA は未公開。
- **優先残作業**: ①GUI 書込の実機確認（Tier 3）②BLE OTA の btsnoop 確定・ACK 照合・マルチセクション `.dat`・配線 ③`0x0015` 永続性/実効と Copy/Delete 露出 ④`config set` 各項目・M3/M4・S2/S3・Switch 側実効の確認 ⑤追加 RE 項目（ターボ・LED 輝度/効果・マクロ実行モード・空マクロ・キャリブレーション・バッテリー・キーボード割当）⑥USB/2.4G と他 OS 実機確認。
- **更新ルール**: main のコミット済みコードを基準とし、実機追記時は OS・経路・MTU・chunk・read-back・SHA-256 を併記する。
