# BLE OTA 解析ノート（`"80ND"` / `"8bt"`）— 証跡

- 対象: 8BitDo Ultimate Software V2 Android v2.01（`classes*.dex`・`lib/x86_64/libadvancelib.so`、x86 / arm64 裏取り）。公式バイナリはリポジトリに含めない。
- 手法・日付: `readelf` / `objdump` / androguard 4.1.4 xref（2026-11-22、実機・BLE 未接触）。
- 判定ラベル: **確定** = 逆アセンブル直接確認 / **推測** = コードから導出・実機未確認 / **未確認** = 本解析で決定不可。
- 関連: [PROTOCOL_SPEC.md](PROTOCOL_SPEC.md) §8 / [OTA_VERIFICATION_PLAN.md](OTA_VERIFICATION_PLAN.md) / `crates/eightbitdo-core/src/ota.rs`。

> 本ノートは Android v2.01 のコード挙動の証跡であり、実機 OTA の成立を保証しない。

## 1. 確定したシーケンス（コード上の順序）

`HIDBoot_startUpdate` (`0x3d530`) → 先頭 28B コピー → `resetdata` (`0x3b620`) → `readSerial` (`0x3bf00`)。

1. `readSerial 0x00C3`（3000ms）でセクタごとの CRC を検証し、一致セクタはスキップ。
2. 不一致セクタのみ `EraseFlash 0x0004`（10000ms、`0x3bfd0`）→ `WriteFirmware 0x0003`（1000ms、`0x3c9f0`）× 64 回（64B × 64 = 4KB セクタ、`ceil(image_len / 0x1000)` セクタ）。
3. 最終セクタ後 `WriteVersion 0x00C4`（3000ms、`0x3c070`）→ `0x0007` reset（3000ms、`0x3ce90`）。

- 送信 cmd は **`0x00C3` / `0x0004` / `0x0003` / `0x00C4` / `0x0007` の 5 種のみ**。
- **デッドコード確定**: `WriteFirmwareVersion` (`0x3d200`) / `WriteAPPID` / `jumpApp` (`0x3d340`) / `readPID` (`0x3d490`) / `readVersionInBoot` (`0x3b580`) / `enterBoot` (`0x357d0`) は `.so` / dex ともに呼び出し元 0 件。

## 2. 各コマンドの実値と `PROTOCOL_SPEC` との差

| 項目 | Android v2.01 静的解析（確定） | 本実装 / `PROTOCOL_SPEC` §8 | 状態 |
| --- | --- | --- | --- |
| `EraseFlash 0x0004` (`0x2d130`) | `offset = sector_off`（相対）、`total_len = 0x1000`、宣言 `chunk_len = 0x1000`、payload 0B / 16B、`crc16 = 0xFFFF`（type 1 のみ） | `offset = load_base + sector_off`、`chunk_len = 0` | **矛盾・未確認** |
| `WriteFirmware 0x0003` (`0x2d350`) | `offset = sect_off + writeoff`（相対）、コピー元に `+0x1C` なし。さらに `Generate_CustomDataCmd` (`0x27b91`) が `cmd==3 && subcmd==0` を **16B（ヘッダのみ）** にする分岐（3 ABI 一致） | `offset = load_base + off`、`.dat[0x1C..]` から 64B payload（81B フレーム） | **矛盾・未確認** |
| `WriteVersion 0x00C4` (`0x2cfb0`) | payload NULL のため `chunk_len = 0`・`crc16 = 0xFFFF` の **16B（ヘッダのみ）**（`0x3019`/`0x9023` のみ 4B） | 24B ヘッダ送信（41B フレーム） | **不一致・未確認** |
| タイムアウト (`Channel+0x64`) | `0x0003`: 1000ms / `0x0004`: 10000ms / `0x00C3`/`0x00C4`/`0x0007`: 3000ms / `0x0005`・`0x0008`・`WriteAPPID`: **2000ms** / `WriteFirmwareVersion`・`jumpApp`: **0（タイマなし）** | `readPID` / `readVersionInBoot` は **5000ms** | **差あり・未確認** |
| `gamepad_type` 決定 | characteristic index（App: `FF11`=0 / `FF13`=1、Boot: `FF21`=0 / `FF23`=1）。type 1 は `0x01017040` / `0x01018000`、type 0 は `0x08014034` / `0x08014000` | PID `0x600F` → type 1 のホワイトリスト規約 | **実機未確認** |

## 3. `.dat` ヘッダと失敗時挙動（確定）

- **28B (`0x1C`) ヘッダ**: `+0x00 u32` `firmwareFile_Version`、`+0x04 u32` `load_base_addr`（汎用経路の wire `offset` には未加算）、`+0x08 u32` `image_len`、`+0x0C..0x1B` メタデータ。サイズ・署名・CRC 検証なし。`firmware_src_length > image_len + 0x1C` なら 2 セクション目の 28B ヘッダを読む。DL 時 MD5 はファイル内容ではなく `contentLength` 文字列の MD5。
- **失敗時復旧**: 全 `*_overtime` が `mov al, 1; ret` で再送・通知・切断なし（`isUpdateing` 再スキャンも dead）。`0x00C3` スキップによる再実行レジューム成立は推測・未確認。

## 4. 未確認事項（優先順）

実機 btsnoop 採取手順は [OTA_VERIFICATION_PLAN.md](OTA_VERIFICATION_PLAN.md) を参照。

1. `cmd=0x0003, subcmd=0` の実フレーム長（16B 問題）と先頭 `+0x1C` 欠落の有無。
2. `0x00C3` 応答レイアウト（4KB CRC が `payload[2..3]` か）。
3. `0x00C4` の実 wire（16B か 24B か）と Windows 実装との差。
4. `"8bt"` 接続時の `gamepad_type` と `readPID` の対応。
5. Bootloader の MTU（81B フレーム通過可否）とマルチセクション `.dat` の実在。
6. Erase / Write の ready ACK と完了 ACK の区別、タイムアウト時状態と `0x0007` reset 後の再列挙時間。
