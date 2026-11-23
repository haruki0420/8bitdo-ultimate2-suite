# 8BitDo Ultimate 2 Bluetooth Controller (PID `0x600F`) 通信プロトコル仕様書

公式 8BitDo Ultimate Software V2（Android `libadvancelib.so` / DEX、Windows `8BitDoAdvance.dll`）の静的解析と Linux BLE 実機観測に基づく通信プロトコル仕様です。

## 1. 同時接続トポロジーと App Mode 起動

ゲーム入力プレーン（Nintendo Switch への Bluetooth Classic または 2.4GHz レシーバ `0x6011`）を維持したまま、制御プレーンとして **BLE GATT（広告名 `"80ND"`、Service `FF10`）** または USB HID（Report ID `0x81`/`0x02`）で接続できる。

- **App Mode 起動**: 電源オン状態で **Profile ボタンを約 3 秒長押し**（Status LED 高速点滅で `"80ND"` 広告開始）。
- **keepalive とセッション運用**: 専用 keepalive コマンドはない。公式は接続時 `0x0007(0)` → 初期読取後 `0x0007(1)` + 20ms `0x0003`（書込中は停止）。実機では 20ms `0x0003` で **128.9 秒**維持を確認。per-user BLE 単一所有者ロック・degraded exit 3・常駐ループの詳細は [SESSION_OPERATIONS.md](SESSION_OPERATIONS.md) を参照。

## 2. VID / PID・HID フィルタ一覧（共通 `VID = 0x2DC8`）

| モデル / 役割 | PID (Hex) | BLE 名 | FW API `firmware_type` | 備考 |
| --- | :---: | --- | :---: | --- |
| **Ultimate 2 Bluetooth** | **`0x600F`** | `"80ND"` / `"8bt"` | `97` (`0x61`) | 本書の中心対象。USB XInput 時は `0x310B`、更新時 `0x3208` |
| **Ultimate 2 BT 2.4G Receiver** | **`0x6011`** | — | `98` (`0x62`) | 付属レシーバ |
| **Ultimate 2 Wireless / Adapter** | `0x6012` / `0x6013` | — | `94` / `95` | 1592B 設定構造体 |
| **Ultimate 2C Bluetooth** | `0x301A` | — | `108` (App) / `96` (Boot) | — |
| **XInput / Boot 切替 / IDLE** | `0x310B` / `0x3208` / `0x3109` | — | — | `0x310B` は Windows で `UsagePage=0xFF7A, Usage=0x0001` を開く |
| その他同一系 | `0x6007`, `0x6009..0x600D`, `0x6003`/`0x6006`, `0x3012`/`0x3013`, `0x3019`, `0x301E` 等 | — | — | `_CY` 系は `FF13`/`FF23` 使用 |

Windows HID 列挙フィルタ: `0x310B`/`0x2084`/`0x3004` → `UsagePage 0xFF7A / Usage 0x0001`、`0x5205`/`0x5206` → `0xFF00/0x0001`、`0x5201` 系 → `0x008C/0x0001`、`0x2039` 系 → `0x000C/0x0001`、`0x600F` 等は VID/PID 直接一致。USB HID レポート長は **65B 固定**（Report ID 1B + 64B）。

## 3. BLE GATT と MTU

| モード | 広告名 | Service UUID | Characteristic (Write+Notify) | CCCD |
| --- | --- | --- | --- | --- |
| **App Mode** | `"80ND"` | `0000FF10-0000-1000-8000-00805F9B34FB` | `0000FF11-...`（`_CY` / 実機フォールバック: **`FF13`**） | `00002902-...` (`[0x01, 0x00]`) |
| **Bootloader** | `"8bt"` | `0000FF20-0000-1000-8000-00805F9B34FB` | `0000FF21-...`（`_CY`: **`FF23`**） | `00002902-...` (`[0x01, 0x00]`) |

- **実機観測**: 検証個体は `"80ND"` / `FF10` だが characteristic は `FF13` のみ。実装は `FF11` 優先 → `FF13` フォールバックとする。
- **MTU とチャンク上限**: ペイロード上限は `MAX_ATT_MTU - 0x12`。公式書込は初期 `MAX_ATT_MTU = 92` のままのため上限 **74B**（本実装の full config write は既定 `min(MTU ペイロード, 74)`、`--write-chunk` 上限 180）。`cmd=0x0103` は BLE 64B / USB 32B（32B アライン）。`cmd=0x0007` 実行時はクライアント側 `MAX_ATT_MTU = 0x3F` (63) へ切替。

## 4. 16B ヘッダ (`cumstom_header_t`) とフレーミング

全マルチバイト整数は **Little-Endian (LE)**。

| Offset | 型 | フィールド | 内容 |
| :---: | --- | --- | --- |
| `+0x00` | `u16` | `cmd` | コマンド ID |
| `+0x02` | `u16` | `subcmd_or_param` | サブコマンド / 引数 / `(mode << 8) \| slot` |
| `+0x04` | `u16` | `chunk_len` | ペイロード長。**例外**: 読取要求 (`0x0002`/`0x0102`) は要求長、`EraseBT2Macro` (`0x0104`) は `0x1000`（payload 0B） |
| `+0x06` | `u16` | `crc16` | ペイロードの CRC-16/MODBUS（未使用時 `0x0000`、空 payload の `0x0104` は `0xFFFF`） |
| `+0x08` | `u32` | `total_len` | 転送総長（`0x000B` は type 1 で `4`、type 2 で `0x01020030`） |
| `+0x0C` | `u32` | `offset` | 開始オフセット（`0x000B` type 1 は `0x01017040`） |

### フレーミング規則

- **BLE TX**: App Mode `[0x04] + 16B header + payload` / Bootloader `[0x05] + 16B header + payload`。
- **BLE RX (`0x600F`)**: **先頭 prefix なし**（`rx[0..16]` がそのまま 16B ヘッダ。`0x202F`/`0x3019`/`0x9023` のみ先頭 2B スキップ）。
- **USB HID App**: TX `[0x81, 0x04] + 16B header + payload (最大 32B) + 0x00 pad`（計 65B）、RX Report ID 除去後 `[0x02, 0x04] + 16B header + payload`。
- **USB Boot**: RX のみ `[0x00, 0x05]`（`readPID`: `cmd=0x0000, subcmd=0x0008`）を確認済み。**USB Boot の TX フレーミングは未文書化**のため、証跡追加まで USB OTA 書込は実装停止とする。

### BLE 実機観測による確定ルール（`FF13`・MTU 200）

- 応答は `cmd=0x0004` 汎用エンベロープ（`subcmd` に要求 `cmd` をエコー）。`chunk_len=0`（`crc16=0xFFFF` 等）は ready ACK であり読み飛ばす。データ応答は `chunk_len > 0`・`total_len == 要求長`・非ゼロ `crc16` を MODBUS 照合する。
- **`ReadVersion` (`0x000B`)**: 4B ダミーペイロード（初回ゼロ、`crc16=0x2400`）を含む **21B 要求**が必須。応答は 20B（4B LE `i32`、実機 `0xC8 = 200`）。
- **`ReadConfig` (`0x0002`) / `ReadMacro` (`0x0102`)**: 要求ヘッダの `chunk_len = total_len`（0 だと ACK のみ）。**1 要求 = 1 チャンク（実機 142B = `0x8E`）**で、`offset += payload.len()` で再要求する（2768B は 20 要求）。最終チャンクも **142B 固定**（70B 有効 + 72B ゼロ）で返り、**142B 全体で CRC 検証**した後に 70B へ切り詰める。応答 `offset` は初回のみ `0`、2 回目以降は要求 `offset` をエコー。

## 5. 2 種類の CRC-16（初期値 `0xFFFF`、LSB-first、最終 XOR なし、空入力 `0xFFFF`）

| 関数 | 規格 | 多項式 | `"123456789"` | 用途 |
| --- | --- | :---: | :---: | --- |
| `ANSI_CRC_16_sh` | CRC-16/MCRF4XX | **`0x8408`** | **`0x6F91`** | 2768B 設定構造体（`+0x0C` 格納） |
| `crc16` | CRC-16/MODBUS | **`0xA001`** | **`0x4B37`** | パケットヘッダ `+0x06`・Bootloader 書込検証 |

## 6. App Mode コマンド一覧（TX prefix `0x04`）

| `cmd` | `subcmd` | `total_len` / `offset` | 方向 | 機能・要点 |
| :---: | :---: | --- | :---: | --- |
| `0x0001` | `0x0000` | `total_len=0x0AD0`（マクロヘッダのみ `0xD8`）、`offset`=構造体内位置 | H→C | 設定書込（全体 / 部分。詳細は [STRUCT_BT2_2768B.md](STRUCT_BT2_2768B.md) §1.2） |
| `0x0002` | `0x0000` | `chunk_len = total_len`（全体 `0x0AD0` / ヘッダ `0x74` / 現スロット `2`） | H→C | 設定読出（1 要求 = 1 チャンク 142B） |
| `0x0003` | `0x0000` | `total_len=0x0C`, `offset=0x0C` | 双方向 | リアルタイム入力レポート（12B、§7） |
| `0x0006` | `0x0067` (BLE)<br/>`0x0123` (USB) | `0` / `0` | H→C | **Save（Flash 永続化）**。公式 BLE は 2s/2 再送、USB は 3s。本実装は 3s・リトライなし |
| `0x0007` | `0` / `1` | `0` / `0` | H→C | `SetReportEnable`（クライアント `MAX_ATT_MTU=0x3F` 切替） |
| `0x000B` | `0x0000` | type 1: `total_len=chunk_len=4`, `offset=0x01017040` + 4B ゼロ | H→C | `ReadVersion`（type 2: `0x01020030`） |
| `0x000C` | `0x0067` | `total_len=4`, `offset=4` | H→C | `EnterBoot`（`"8bt"` へ再起動遷移） |
| `0x000E` | — | — | C→H | エラー通知 |
| `0x000F` | `0x000F` | `total_len=0`, `offset=4` | H→C | `DisConnectBluetooth` |
| `0x0010` / `0x0011` | `0x0001` / `state` | `0` / `0` | H→C | `SetPhonePlatform` / `SetConfigState` |
| `0x0012` | `0x0000` | — | C→H | `AllowConfigDone`（非同期通知） |
| `0x0013` | `0x0000` | `0` / `0` | H→C | Windows USB 用 `StartConfig`（最大 30 リトライ） |
| `0x0015` | `slot` (`0..2`) | `0` / `0` | H→C | `WriteSLOT`（プロファイル切替）。実機で 16B 封筒 ACK（1 回）と Save なし `cur_slot` 6→1 確認（永続性・実効・`00 81` の意味・再現性は未確認。[DEVICE_CONFIG_EVIDENCE.md](DEVICE_CONFIG_EVIDENCE.md)） |
| `0x0102` | `(mode<<8)\|slot` | `total_len=chunk_len=steps*10`, `offset=(macro_idx<<12)+chunk_off` | H→C | `ReadBT2Macro`（1 要求 = 1 チャンク 142B） |
| `0x0103` | `(mode<<8)\|slot` | `total_len=(macro_idx<<12)+align32(steps*10)`（絶対終端）, `offset=(macro_idx<<12)+idx*chunk` | H→C | `WriteBT2Macro`（BLE 64B / USB 32B・32B アライン、CRC-16/MODBUS）。順序・実機詳細は [STRUCT_BT2_2768B.md](STRUCT_BT2_2768B.md) §7.3 |
| `0x0104` | `(mode<<8)\|slot` | `total_len=chunk_len=0x1000`, `crc16=0xFFFF`, `offset=macro_idx<<12`, payload 0B | H→C | `EraseBT2Macro`（4KB セクタ消去） |

## 7. リアルタイム入力レポート（`0x0003`, 12B LE）

| Offset | 型 | フィールド | 定義 |
| :---: | --- | --- | --- |
| `0x00..0x03` | `u32` | `Key` | 32-bit ボタンマスク（[STRUCT_BT2_2768B.md](STRUCT_BT2_2768B.md) §5.2 と同一ビット配置） |
| `0x04..0x07` | `u8` × 4 | `LX, LY, RX, RY` | 中心 `127`/`128`（両方 `{127, 128}` は `(0,0)` 不感帯、正規化 `val - 127`） |
| `0x08` / `0x09` | `u8` / `u8` | `Trigger_R` / `Trigger_L` | `0..255`（`is3E`/`is3Xbox` のみ L/R 逆） |
| `0x0A` / `0x0B` | `u8` / `u8` | `Battery` / `Reserved` | バッテリー生値（スケール・充電状態未確認）/ `0x00` |

## 8. BLE OTA / ファームウェア更新仕様

### 8.1 配信 API と `.dat` 構造

- **エンドポイント**: `POST https://support.8bitdo.{com,cn}/restapi/link/v1/client/firmware/select?firmware_type={type}&beta={0|1}&soft_type=5`（ヘッダ `Content_Type: application/x-www-form-urlencoded`、応答 `{"data": "[{...}]"}` の 2 段階 JSON）。Android 実装は `firmware.md5` を `contentLength` 文字列の MD5 と比較するクセがある。
- **`.dat` 28B (`0x1C`) ヘッダ**: `+0x00 u32 firmwareFile_Version`、`+0x04 u32 load_base_addr`、`+0x08 u32 image_data_len`、`+0x0C..0x17` メタデータ（`WriteVersion` は `+0x00..0x18` の 24B）。**現実装（`ota.rs`）は単一セクションのみ対応**し、`file_size != 28 + image_len`（マルチセクション）を拒否する（公式ファーム対応の blocker）。

### 8.2 Bootloader コマンド（TX prefix `0x05`、`"8bt"` `FF20`/`FF21`）

| `cmd` | `subcmd` | `offset` | `total_len` / `chunk_len` | `crc16` | Timeout | 機能 |
| :---: | :---: | --- | --- | :---: | :---: | --- |
| `0x0008` | `0x0067` | `0` | `total_len=2` | なし | `5000 ms` | `readPID`（USB Boot は `cmd=0x0000, subcmd=0x0008`） |
| `0x0005` | `0x0067` | `0x01017040` (type 1) | `total_len=4` | なし | `5000 ms` | `readVersionInBoot` |
| `0x0004` | `0x0000` | `load_base + sector_off` | `total_len=0x1000` (4KB) | なし (`0`) | **`10000 ms`** | `EraseFlash`（ACK 前に Write 禁止） |
| `0x0003` | `0x0000` | `load_base + sector_off + chunk_off` | `total_len=image_len`, `chunk_len=0x40` (64B) | **あり** | `1000 ms` | `WriteFirmware`（64B 固定・`.dat + 0x1C`） |
| `0x00C4` | `0x0000` | `0` | `total_len=image_len`, `chunk_len=0x18` (24B) | なし | `3000 ms` | `WriteVersion` |
| `0x0002` | `0x0000` | `0x01017040` (type 1) / `0x08014034` | `total_len=chunk_len=4` | **あり** | `0 ms` | `WriteFirmwareVersion` |
| `0x0002` | `0x0000` | `0x0801401C` | `total_len=0x0C` (12B) | **あり** | `5000 ms` | `WriteAPPID`（**type 1 はスキップ**） |
| `0x0006` | `0x0000` | `0x01018000` (type 1) | `total_len=0` | なし | `5000 ms` | `jumpApp`（App Mode 復帰） |

失敗時は BLE なら `"8bt"` へ再接続して `readPID` → 先頭から全セクタ消去・再書込で復旧する（USB OTA 書込は TX 未文書化のため保留）。

### 8.3 実装状況（`crates/eightbitdo-core/src/ota.rs`）と未検証事項（旧 §8.5）

BLE 専用状態機械 `OtaUpdate`（Mock 52〜61 テスト）と段階実行 API（`eightbitdo-core/ota-bench` 既定 OFF）を実装済みだが、**CLI / GUI / UniFFI 未配線**。`allow_unverified_wire = false`（既定）、`require_completion_ack = false`（既定）、`OTA_MIN_MTU = 84`、type 1 暫定ウィンドウ `0x0101_0000..0x0102_0000`、リトライなし。デスクトップは `BleBootTransport` のみ許可し、モバイルは `mobile-boot` feature でのみ `MobileBootLink` を提供する。

**実機 btsnoop 待ちの未検証事項（採取手順は [OTA_VERIFICATION_PLAN.md](OTA_VERIFICATION_PLAN.md)、静的解析証跡は [OTA_BLE_EVIDENCE.md](OTA_BLE_EVIDENCE.md)）**:
1. Bootloader RX フレーミング（prefix なし仮定）
2. `readPID` / `readVersionInBoot` 応答形状
3. ready ACK と完了 ACK の区別
4. PID → `gamepad_type` 対応（ホワイトリスト規約 vs characteristic index）
5. Erase / Write アドレス規約（仕様 `load_base + off` vs 証跡の相対 offset）
6. `WriteVersion 0x00C4` payload 長（仕様 24B vs 証跡 16B）
7. `EraseFlash` の `chunk_len`（実装 `0` vs 証跡 `0x1000`）
8. `WriteFirmware` 実フレーム長（仕様 64B vs 証跡 16B 問題）
9. `enterBoot 0x000C` 成立と挙動
10. タイムアウト差（仕様 5s vs Android 2s）
11. Bootloader MTU（81B フレーム通過可否）
12. **マルチセクション `.dat`（blocker）**
13. **遅延 / 重複 ACK の誤帰属（CLI 配線前に `offset` / `total_len` 照合追加が必須）**
14. `load_base_addr` 来歴の trust boundary（署名・ハッシュ検証は呼び出し側責務）
15. バッテリー残量 / ダウングレード判定 / FW 取得検証
