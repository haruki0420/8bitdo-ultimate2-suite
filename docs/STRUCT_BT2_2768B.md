# 8BitDo Ultimate 2 Bluetooth (`0x600F`) 2768B 設定構造体 & マクロ完全バイナリ仕様書

対象: **8BitDo Ultimate 2 Bluetooth Controller**（PID `0x600F`、設定サイズ `0x0AD0` = **2768 バイト**、`record_ultimatebt2`）。全オフセット・ビット定義・型は公式 Android APK v2.01（`libadvancelib.so` / `classes.dex`）および Windows v1.38（`8BitDoAdvance.dll`）の逆解析に基づく。

> **エンディアン**: 特記なき限りすべて **Little-Endian (LE)**。**例外**: プロファイル名・マクロ名のみ **UTF-16BE**。

## 1. 全体メモリマップ（2768B / `0x000`〜`0xACF`）

3 つのプロファイルスロット（`slot = 0, 1, 2`）を SoA（Structure of Arrays）で配置する。

| オフセット範囲 | サイズ | 配列構成 | 要素長 | フィールド名 | 内容 |
| --- | ---: | --- | ---: | --- | --- |
| `0x000`〜`0x00B` | 12B | `[u32; 3]` | 4B | `slot_flags[3]` | スロットフラグ（実機は `0x20200911` / `0xFFFFFFFF`。意味未確定） |
| `0x00C`〜`0x00F` | 4B | `u32` | 4B | `crc16` | `ANSI_CRC_16_sh` 格納（下位 16bit 使用・上位 16bit=0） |
| `0x010`〜`0x011` | 2B | `u16` | 2B | `gamepad_mode` | 接続/動作モード（`GAMEPAD_MODE_SWIFT`） |
| `0x012`〜`0x013` | 2B | `u16` | 2B | `cur_slot` | アクティブスロット（`0..=2`, `4=NONE`。実機で `6` 観測・未確定） |
| `0x014`〜`0x073` | 96B | `[[u8; 32]; 3]` | 32B | `file_names[3]` | プロファイル名（UTF-16BE、最大 16 文字） |
| `0x074`〜`0x097` | 36B | `[RumbleRecord; 3]` | 12B (`0x0C`) | `rumble[3]` | 左右モーター振動強度 |
| `0x098`〜`0x0AF` | 24B | `[JoyRecord; 3]` | 8B (`0x08`) | `joy[3]` | 左右スティック デッドゾーン/有効範囲 |
| `0x0B0`〜`0x0C7` | 24B | `[TriggerRecord; 3]` | 8B (`0x08`) | `trigger[3]` | LT/RT トリガー デッドゾーン/有効範囲 |
| `0x0C8`〜`0x0DF` | 24B | `[JoyTriggerSpecialRecord; 3]` | 8B (`0x08`) | `special[3]` | 特殊機能ビットマスク |
| `0x0E0`〜`0x1F3` | 276B | `[MapKeyRecord; 3]` | 92B (`0x5C`) | `map_key[3]` | ボタンリマップ（`flag` 4B + `key[22]` 88B） |
| `0x1F4`〜`0x68B` | 1176B | `[[u8; 392]; 3]` | 392B (`0x188`) | *(レガシー/予約 A)* | 旧世代マクロ枠の名残（`copyConfig`/`deleteCurslot` のみ参照。バイト保全必須） |
| `0x68C`〜`0x913` | 648B | `[MacroPro3Struct; 3]` | 216B (`0xD8`) | `macros[3]` | マクロ管理ヘッダ（4 マクロ × 52B） |
| `0x914`〜`0x92B` | 24B | `[[u8; 8]; 3]` | 8B (`0x08`) | *(レガシー/予約 B)* | 予約領域（`copyConfig`/`deleteCurslot` のみ参照。バイト保全必須） |
| `0x92C`〜`0x94F` | 36B | `[SixAxis; 3]` | 12B (`0x0C`) | `six_axis[3]` | 6 軸ジャイロ設定 |
| `0x950`〜`0x973` | 36B | `[DynamicTheme; 3]` | 12B (`0x0C`) | `dynamic_theme[3]` | RGB: ダイナミックテーマ |
| `0x974`〜`0x9A3` | 48B | `[HotWheelTheme; 3]` | 16B (`0x10`) | `hotwheel_theme[3]` | RGB: ホットホイールテーマ＋速度 |
| `0x9A4`〜`0xACF` | 300B | `[SingleTheme; 3]` | 100B (`0x64`) | `single_theme[3]` | RGB: 個別 24 LED カラー配列 |

### 1.2 読み書き・部分書き込みパラメータ一覧

読出は **`cmd=0x0002, subcmd=0x0000`**、書込は **`cmd=0x0001, subcmd=0x0000`**。Rumble / SixAxis / Dynamic のスロット乗数はすべて **`0x0C` (12B)**。

| 操作 | 関数 (`libadvancelib.so` / `8BitDoAdvance.dll`) | `cmd` | `subcmd` | `offset` | `len` | `total_len` |
| --- | --- | :---: | :---: | --- | --- | --- |
| 全体読込 / ヘッダ / 現スロット | `getUT2_ReadCofigChannel` / `head` / `Curslot` | `0x0002` | `0x0000` | `0` / `0` / `0x12` | `0xAD0` / `0x74` / `2` | 同左 |
| 全体書込 / スロット切替書込 | `getBT2_WriteCofigChannel` / `WriteSLOTChannel` | `0x0001` | `0x0000` | `0x0000` / `0x0012` | `0x0AD0` / `2` | `0x0AD0` |
| プロファイル名書込 | `getBT2_WriteFileName(slot)` | `0x0001` | `0x0000` | `0x0014 + slot*0x20` | `0x0020` | `0x0AD0` |
| 振動 / スティック / トリガー / 特殊 | `WriteRumble` / `JoyStick` / `Trigger` / `Special` | `0x0001` | `0x0000` | `0x74+s*0xC` / `0x98+s*8` / `0xB0+s*8` / `0xC8+s*8` | `0xC` / `8` / `8` / `8` | `0x0AD0` |
| 単一ボタンリマップ | `getBT2_WriteMapKey(slot, idx)` | `0x0001` | `0x0000` | `0x00E4 + slot*0x5C + idx*4` | `0x0004` | `0x0AD0` |
| マクロヘッダ / マクロ名単体 | `getBT2_WriteMacroi` / `MacroName` | `0x0001` | `0x0000` | `0x68C+s*0xD8` / `0x694+s*0xD8+m*0x34` | `0xD8` / `0x20` | **`0x00D8`** / `0x0AD0` |
| ジャイロ書込（※注1） | `writeUltimateBT2sixaxis(slot)` *(DLL 正)* | `0x0001` | `0x0000` | `0x092C + slot*0x0C` | `0x000C` | `0x0AD0` |
| RGB Dynamic / HotWheel / Single | `WriteDyNamic` / `HotWheel` / `Single` | `0x0001` | `0x0000` | `0x950+s*0xC` / `0x974+s*0x10` / `0x9A4+s*0x64` | `0xC` / `0x10` / `0x64` | `0x0AD0` |
| 設定保存 (BLE / USB) | `getSaveCustomConfigChannel` / DLL 末尾 | `0x0006` | `0x0067` / `0x0123` | `0x0000` | `0` | `0` |

> **※注1**: Android `libadvancelib.so` の `getBT2_WriteSixAxis` はバグにより `offset=0, total_len=0x0C` を送る。独自実装は Windows DLL 準拠（`offset = 0x92C + slot*0x0C, total_len = 0x0AD0`）を使うこと。
> **書込分割長・Save・実機 flag**: 公式 BLE 書込上限は **74B**（`MAX_ATT_MTU 92 - 0x12`）、完了時は必ず `0x0006/0x0067` Save を 1 回送る。実機（Linux BLE・MTU 200）では full 2768B（38 write + 1 Save = 39 packets）で read-back 一致を確認済み。実機レコード flag は `0x20200911` / `0xFFFFFFFF` / gyro slot0 `0x20190000`（opaque）のため、raw 検証はこれら観測値集合と完全消去 slot のみ受理する（[DEVICE_CONFIG_EVIDENCE.md](DEVICE_CONFIG_EVIDENCE.md)）。

## 2. ヘッダ・グローバル状態 (`+0x000..+0x013`, 20B)

- **`slot_flags[3]` (`+0x000..+0x00B`)**: `deleteCurslot` は `0` を書く。実機では `[0x20200911, 0x20200911, 0xFFFFFFFF]` を観測（意味未確定）。
- **`crc16` (`+0x00C..+0x00F`)**: `ANSI_CRC_16_sh`（CRC-16/MCRF4XX、多項式 `0x8408`、初期値 `0xFFFF`、LSB-first、最終 XOR なし、空入力 `0xFFFF`）。公式は **`+0x00C..+0x00F` をゼロクリアせず**全 2768B に対して計算し上書き格納するため、読出時の in-place 再計算と stored が一致しない場合がある（破損とは限らない）。
- **`gamepad_mode` (`+0x010`, `u16`)**: `0`=Switch / `1`=GAMEPAD_J_M / `2`=MAC_M / `3`=XINPUT_M / `4`=EXTEND_M / `5`=NONE。下位 8bit がマクロ `subcmd = (gamepad_mode << 8) | slot` に使われる。
- **`cur_slot` (`+0x012`, `u16`)**: `0..=2`（スロット 1..3）/ `3`=MAX / `4`=NONE。実機で `6` を観測（意味未確定、raw `config write` は警告して保持）。

## 3. プロファイル名 (`+0x014..+0x073`, 3 × 32B)

`offset = 0x014 + slot * 0x20`。**UTF-16BE**、最大 16 文字（32B）、未使用部は `0x00` パディング。デコード時は 2B 単位で `[0x00, 0x00]` 終端。

## 4. 振動・スティック・トリガー・特殊機能 (`+0x074..+0x0DF`)

- **`RumbleRecord` (`0x074 + slot*0x0C`, 12B)**: `+0x00 u32 flag`、`+0x04 f32 zoom_out[0]`（左）、`+0x08 f32 zoom_out[1]`（右）。6 段階: `0.0` (`0x00000000`) / `0.2` (`0x3E4CCCCD`) / `0.4` (`0x3ECCCCCD`) / `0.6` (`0x3F19999A`) / `0.8` (`0x3F4CCCCD`) / `1.0` (`0x3F800000`, 既定)。
- **`JoyRecord` (`0x098 + slot*0x08`, 8B)**: `+0x00 u32 flag`、`+0x04..+0x07 u8` `left_start` (`0..=127`, 作成時既定 **17 = `0x11`**) / `left_end` (`0..=128`, 既定 `128`) / `right_start` / `right_end`（`flag != 1` 時は `[0, 128, 0, 128]` 扱い）。
- **`TriggerRecord` (`0x0B0 + slot*0x08`, 8B)**: `+0x00 u32 flag`、`+0x04..+0x07 u8` `left_start`（`gamepad_mode == 0` 時のみ既定 **77 = `0x4D`**、他は `0`）/ `left_end`（既定 `255`）/ `right_start` / `right_end`。
- **`JoyTriggerSpecialRecord` (`0x0C8 + slot*0x08`, 8B)**: `+0x00 u32 flag`、`+0x04 u32 feature` ビットマスク:

| Bit | マスク | 機能 | Bit | マスク | 機能 |
| :---: | :---: | --- | :---: | :---: | --- |
| 0 / 1 | `0x01` / `0x02` | 左スティック X / Y 反転 | 11 | `0x0800` | モーション感度フラグ |
| 2 / 3 | `0x04` / `0x08` | 右スティック X / Y 反転 | 12 | `0x1000` | ゼロデッドゾーンモード |
| 4 | `0x10` | 左右スティック入替 | 13 | `0x2000` | 4 方向入力モード (4-Way D-Pad) |
| 7 | `0x80` | L2 / R2 入替 | 14 / 15 | `0x4000` / `0x8000` | トリガー振動 / 本体振動有効 |
| 8 | `0x100` | 左スティック / D-Pad 入替 | 20 / 21 | `0x0010_0000` / `0x0020_0000` | `joyShare == 1` / `2` |
| 10 | `0x400` | 右スティック・トリガー反転/切替 | 5, 6, 9 | — | 未使用（0 維持） |

## 5. ボタンリマップ (`MapKeyRecord`, `+0x0E0..+0x1F3`) と 32-bit `KeyMap`

`offset = 0x0E0 + slot * 0x5C`（92B = `+0x00 u32 flag` + `+0x04 [u32; 22] key`）。1 ボタン = 1 つの単一 32-bit `KeyMap` 値（無効 = `0`、`227` は用途未確認のため書かない）。実機確認済み（`config remap A B` → `B A`、39 packets、flag 不変）。

### 5.1 `key[22]` 配列インデックス（Non-3E / Ultimate 2 順）

| Idx | Rel | 物理ボタン | 既定値 | Idx | Rel | 物理ボタン | 既定値 |
| :---: | :---: | --- | --- | :---: | :---: | --- | --- |
| 0 | `+0x04` | **A** (Switch: B) | `0x0000_2000` | 11 | `+0x30` | **Start / +** | `0x0000_0001` |
| 1 | `+0x08` | **B** (Switch: A) | `0x0000_1000` | 12 | `+0x34` | **Share / Star** | `123` (`KeyMap_Share`) |
| 2 | `+0x0C` | **X** (Switch: Y) | `0x0000_0010` | 13 | `+0x38` | **Logo / Home** | `124` (`KeyMap_Logo`) |
| 3 | `+0x10` | **Y** (Switch: X) | `0x0000_0020` | 14 | `+0x3C` | **D-Pad Up** | `0x0000_0200` |
| 4 | `+0x14` | **L1 / LB** | `0x0000_0400` | 15 | `+0x40` | **D-Pad Down** | `0x0000_0100` |
| 5 | `+0x18` | **R1 / RB** | `0x0000_0800` | 16 | `+0x44` | **D-Pad Left** | `0x0000_0080` |
| 6 | `+0x1C` | **L2 / LT** | `0x0000_4000` | 17 | `+0x48` | **D-Pad Right** | `0x0000_0040` |
| 7 | `+0x20` | **R2 / RT** | `0x0000_8000` | 18 | `+0x4C` | **P1 (PL / 背面左)** | `0x0200_0000` or `0` |
| 8 | `+0x24` | **L3 / LS** | `0x0000_0002` | 19 | `+0x50` | **P2 (PR / 背面右)** | `0x0400_0000` or `0` |
| 9 | `+0x28` | **R3 / RS** | `0x0000_0004` | 20 | `+0x54` | **P3 (L4 バンパー)** | `0x0020_0000` or `0` |
| 10 | `+0x2C` | **Select / -** | `0x0000_0008` | 21 | `+0x58` | **P4 (R4 バンパー)** | `0x4000_0000` or `0` |

### 5.2 32-bit `KeyMap` 全 36 定数

**単一ビット定数（27 種）**:

| Bit | Hex | 定数名 | Bit | Hex | 定数名 | Bit | Hex | 定数名 |
| :---: | :---: | --- | :---: | :---: | --- | :---: | :---: | --- |
| 0 | `0x0000_0001` | `Start` | 9 | `0x0000_0200` | `Up` | 18 | `0x0004_0000` | `BT_CON` |
| 1 | `0x0000_0002` | `L3` | 10 | `0x0000_0400` | `L1` | 21 | `0x0020_0000` | `P3` (L4) |
| 2 | `0x0000_0004` | `R3` | 11 | `0x0000_0800` | `R1` | 22 | `0x0040_0000` | `ScreenShot` |
| 3 | `0x0000_0008` | `Select` | 12 | `0x0000_1000` | `B` | 23 | `0x0080_0000` | `Turbo_3` |
| 4 | `0x0000_0010` | `X` | 13 | `0x0000_2000` | `A` | 24 | `0x0100_0000` | `Turbo_Auto` |
| 5 | `0x0000_0020` | `Y` | 14 | `0x0000_4000` | `L2` | 25 | `0x0200_0000` | `P1` (PL) |
| 6 | `0x0000_0040` | `Right` | 15 | `0x0000_8000` | `R2` | 26 | `0x0400_0000` | `P2` (PR) |
| 7 | `0x0000_0080` | `Left` | 16 | `0x0001_0000` | `Turbo` | 27 | `0x0800_0000` | `Swap` |
| 8 | `0x0000_0100` | `Down` | 17 | `0x0002_0000` | `Home` | 30 | `0x4000_0000` | `P4` (R4) |

**非ビットマスク定数（9 種）**: `0`=`KeyMap_N`（無効）、`119..=122`=`KeyMap_M1..M4`、`123`=`KeyMap_Share`、`124`=`KeyMap_Logo`/`View`、`227` (`0xE3`)=`KeyMap_My_NUll`（用途未確認・書込禁止）。

## 6. 6 軸ジャイロ (`+0x92C..+0x94F`) と RGB (`+0x950..+0xACF`)

- **`SixAxis` (`0x92C + slot*0x0C`, 12B)**: `+0x00 u32 flag`、`+0x04 u32 key_map`、`+0x08 u8 trigger_mode`（`0`=Hold, `1`=Toggle）、`+0x09 u8 sensitivity`（`1..=15`, 既定 `5`）、`+0x0A u8 dead_compensate`（`0..=255`, 既定 `40`）、`+0x0B u8 mapping_type`（`0`=Left Stick, `1`=Right Stick, `2`=Mouse）。
- **RGB 色形式**: `u32` LE の ARGB（`0xAARRGGBB`、メモリ順 `[B, G, R, A]`）。
  - `Dynamic_Theme` (`0x950 + slot*0x0C`, 12B): `flag`, `color` (既定 `0xFF260032`), `bg_color` (既定 `0xFF084E50`)。
  - `HotWheel_Theme` (`0x974 + slot*0x10`, 16B): `flag`, `color` (`0xFFCC6200`), `bg_color` (`0xFFCC0000`), `+0x0C u8 speed` (`0..=15`, 既定 `7`), `+0x0D..+0x0F` pad。
  - `Single_Theme` (`0x9A4 + slot*0x64`, 100B): `flag`, `+0x04 [u32; 24] colors`（既定 `0xFF7E3200`）。

## 7. マクロ管理ヘッダ (`+0x68C..+0x913`) と 10B ステップ / Flash 仕様

### 7.1 `Macro_Pro3_Struct` (216B = `0xD8` / slot) と `MacroFunPro3Record` (52B = `0x34` / macro)

- スロット先頭 `0x68C + slot*0xD8`: `+0x00 u32 flag`、`+0x04 u8 total_cnt` (`0..=4`)、`+0x05..+0x07` pad、`+0x08..+0xD7 records[4]`。
- 各レコード（開始 `0x694 + slot*0xD8 + m*0x34`）: `+0x00..+0x1F` `filename` (UTF-16BE 32B)、`+0x20 u8 gamepad_mode`、`+0x21 u8 special_flag`、`+0x22 u16 max_steps` (`0..=200`、絶対 offset `0x6B6 + slot*0xD8 + m*0x34`)、`+0x24 u16 offset`、`+0x26..+0x27` pad、`+0x28 u32 key_map`、`+0x2C u32 cycles_num` (`0`=無限/ホールド)、`+0x30 u32 interval_ms`。

### 7.2 `MacroDatas` 10 バイト構造（最大 200 ステップ = 2000B）

| Offset | 型 | フィールド | 値域・補足 |
| :---: | --- | --- | --- |
| `+0x00..+0x01` | `u16` | `ms_times` | 持続時間（**1ms 単位**） |
| `+0x02..+0x03` | `u16` | `keys` | `KeyMap` 下位 16bit（Start..R2） |
| `+0x04` / `+0x05` | `u8` / `u8` | `rt_value` / `lt_value` | R2 / L2 アナログ値（`0x00..0xFF`、`u16` で下位=RT・上位=LT） |
| `+0x06` / `+0x07` | `u8` / `u8` | `left_x` / `left_y` | 左スティック（`0x00`=左/上、`0x7F`=中立、`0xFF`=右/下） |
| `+0x08` / `+0x09` | `u8` / `u8` | `right_x` / `right_y` | 右スティック（同上） |

### 7.3 Flash アドレス・アライメント・書き込み順序

- セクタアドレス: `offset = (macro_idx as u32) << 12`（0x0000 / 0x1000 / 0x2000 / 0x3000）。`subcmd = ((gamepad_mode as u16) << 8) | (slot as u16)`。
- **Write (`0x0103`) の `total_len`（実機確認済み）**: **絶対終端 `((macro_idx as u32) << 12) + ((max_steps * 10 + 31) & !31)`**。相対値（sector base なし）では ACK のみで永続化されない（BLE・slot S1・M1/M2 で確認、M3/M4・S2/S3 は外挿、USB 未実装）。
- **書き込み順序（厳守）**:
  1. **Header (`0x0001`)**: `offset = 0x68C + slot*0xD8`, **`total_len = 0xD8`**, 216B を `min(142, mtu - 20)` 分割（`+0x00C` CRC は wire 非送信）、ACK 2s。
  2. **Header Save (`0x0006`)**: BLE `subcmd = 0x0067`、ACK 3s・リトライなし（Header/Save 失敗時は Erase 非送信）。
  3. **Erase (`0x0104`)**: `offset = macro_idx << 12`, `total_len = chunk_len = 0x1000`, `crc16 = 0xFFFF`, payload 0B、ACK 2s → 100ms 待機（暫定）。
  4. **Write (`0x0103`)**: `offset = (macro_idx << 12) + chunk_index * chunk_size`（32B/64B・32B アライン）、各チャンク CRC-16/MODBUS、ACK 2s → 全 Write 後 50ms 待機（暫定）。
  5. **Final Save (`0x0006`)**: slot バッチにつき 1 回（ACK 3s・リトライなし）→ 300ms 待機（暫定）。
  6. **Read-back**: `0x0002` で 216B ヘッダ、`0x0102`（`total_len = chunk_len = max_steps * 10`、1 要求 = 1 チャンク 142B）でステップ一致検証。空きマクロ（`max_steps == 0` / `0xFFFF`）への `macro write` は `EmptyMacroWrite` で拒否。書込中は `0x0003` を送らない。

## 8. Ultimate 2 Wireless (`0x6012`, 1592B) との差分

`0x000..0x1F3`（ヘッダ〜リマップ）は同一。Wireless 版は **レガシー領域 A（`0x1F4..0x68B`, 1176B = `0x498`）が存在せず**、以降のマクロヘッダ〜RGB Single がすべて **`-0x498`** 前詰めになる（合計 `0x638` = 1592B）。また USB Save subcmd は BT2 `0x0123` に対し Wireless `0x0064`。
