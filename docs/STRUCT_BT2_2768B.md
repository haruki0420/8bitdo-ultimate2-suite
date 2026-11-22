# 8BitDo Ultimate 2 Bluetooth Controller (`0x600F`) 2768B 設定構造体 & マクロ完全バイナリ仕様書

本書は **8BitDo Ultimate 2 Bluetooth Controller**（BLE/USB/2.4G, PID `0x600F`, 内部設定サイズ `0x0AD0` = **2768 バイト**）のコンフィグ構造体 `record_ultimatebt2` およびフラッシュメモリ上のマクロデータ構造の完全バイナリ仕様である。

全オフセット・ビット定義・型・プロトコル引数は、公式 Android APK v2.01（`libadvancelib.so` 逆コンパイル結果および `classes.dex` Dalvik 逆アセンブル）および Windows 版 v1.38（`8BitDoAdvance.dll` 逆アセンブル）から直接検証した値に基づく（推測なし）。

> **エンディアン表記**: 特に明記がない限り、すべての複数バイト整数・浮動小数点数は **Little-Endian (LE)** とする（**例外**: プロファイル名・マクロ名のみ **UTF-16BE**）。

---

## 1. 全体メモリマップ（2768 バイト / `0x000`〜`0xACF`）

### 1.1 セクション配置表

構造体全体は **3 つのプロファイルスロット（`slot = 0, 1, 2`）** を持ち、各機能セクションは「3 スロット分の固定長配列」として配置されている（SoA: Structure of Arrays 配置）。

| オフセット範囲 | サイズ | 配列構成 | 要素サイズ | 構造体 / フィールド名 | 内容概要 |
| --- | ---: | --- | ---: | --- | --- |
| `0x000`〜`0x00B` | 12B | `[u32; 3]` | 4B | `slot_flags[3]` | 各スロットのフラグ（実機では `0x20200911` / `0xFFFFFFFF` を観測。非 0 が有効の可能性があるが、正確な意味は未確定） |
| `0x00C`〜`0x00F` | 4B | `u32` | 4B | `crc16` | CRC-16 (`ANSI_CRC_16_sh`) 格納領域（下位 16bit 使用・上位 16bit=0） |
| `0x010`〜`0x011` | 2B | `u16` | 2B | `gamepad_mode` | 現在の接続/動作モード (`GAMEPAD_MODE_SWIFT`) |
| `0x012`〜`0x013` | 2B | `u16` | 2B | `cur_slot` | 現在のアクティブスロット (`0..=2`, `4 = NONE`) |
| `0x014`〜`0x073` | 96B | `[[u8; 32]; 3]` | 32B (`0x20`) | `file_names[3]` | プロファイル名（UTF-16BE、最大16文字） |
| `0x074`〜`0x097` | 36B | `[RumbleRecord; 3]` | 12B (`0x0C`) | `rumble[3]` | 左右モーター振動強度 |
| `0x098`〜`0x0AF` | 24B | `[JoyRecord; 3]` | 8B (`0x08`) | `joy[3]` | 左右スティック デッドゾーン/有効範囲 |
| `0x0B0`〜`0x0C7` | 24B | `[TriggerRecord; 3]` | 8B (`0x08`) | `trigger[3]` | LT/RT トリガー デッドゾーン/有効範囲 |
| `0x0C8`〜`0x0DF` | 24B | `[JoyTriggerSpecialRecord; 3]` | 8B (`0x08`) | `special[3]` | スティック/トリガー/振動等の特殊機能ビットマスク |
| `0x0E0`〜`0x1F3` | 276B | `[MapKeyRecord; 3]` | 92B (`0x5C`) | `map_key[3]` | ボタンリマッピング（`flag` 4B + 22ボタン × `u32` 88B） |
| `0x1F4`〜`0x68B` | 1176B | `[[u8; 392]; 3]` | 392B (`0x188`) | *(レガシー/予約領域 A)* | 3 × 392B（`copyConfig`/`deleteCurslot` のみ参照。§1.3 参照） |
| `0x68C`〜`0x913` | 648B | `[MacroPro3Struct; 3]` | 216B (`0xD8`) | `macros[3]` | マクロ管理ヘッダ（スロットあたり 4 マクロ × 52B） |
| `0x914`〜`0x92B` | 24B | `[[u8; 8]; 3]` | 8B (`0x08`) | *(レガシー/予約領域 B)* | 3 × 8B（`copyConfig`/`deleteCurslot` のみ参照。§1.3 参照） |
| `0x92C`〜`0x94F` | 36B | `[SixAxis; 3]` | 12B (`0x0C`) | `six_axis[3]` | 6軸ジャイロ/モーション設定 |
| `0x950`〜`0x973` | 36B | `[DynamicTheme; 3]` | 12B (`0x0C`) | `dynamic_theme[3]` | RGB: ダイナミック/レインボー系テーマ色 |
| `0x974`〜`0x9A3` | 48B | `[HotWheelTheme; 3]` | 16B (`0x10`) | `hotwheel_theme[3]` | RGB: ホットホイール（回転リング）テーマ色＋速度 |
| `0x9A4`〜`0xACF` | 300B | `[SingleTheme; 3]` | 100B (`0x64`) | `single_theme[3]` | RGB: 個別 24 LED カラー配列 |
| **合計** | **2768B** | — | — | `0x0000`〜`0x0ACF` | `0x9A4 + 3 * 0x64 = 0x0AD0` (2768 バイト) |

### 1.2 読み書き・部分書き込みパラメータ一覧

16 バイトカスタムヘッダの `cmd` / `subcmd` / `offset` / `total_len` / `len` 対応表は以下の通り（詳細は [PROTOCOL_SPEC.md](PROTOCOL_SPEC.md) 参照）。

> **重要（`PROTOCOL_SPEC.md` との突合・補正結果）**:
> `libadvancelib.so` の `PTR_record_ultimatebt2` は `u8*`（バイトポインタ）であり、Windows 版 `8BitDoAdvance.dll` と合わせて **Rumble (`0x74 + slot * 0x0C`)、SixAxis (`0x92C + slot * 0x0C`)、Dynamic (`0x950 + slot * 0x0C`) のスロット乗数はすべて `0x0C` (12 バイト)** である（`slot * 4` ではない）。
>
> **`cmd` / `subcmd` の補正（x86_64 `libadvancelib.so` 再確認済み）**:
> 設定の読み出しは **`cmd = 0x0002`, `subcmd = 0x0000`**、書き込みは **`cmd = 0x0001`, `subcmd = 0x0000`** が正しい。旧版の本書にあった `subcmd = 0x0101` / `0x0102` は誤記である。`getUT2_ReadCofigChannel` はチャネル構造体の `+0x4C` に `0x0002` を、`getBT2_WriteCofigChannel` は `0x0001` を設定し、`+0x4E`（subcmd）はどちらも `0` のままである。個別書込（`getBT2_WriteRumble` 等）も同様に `cmd = 0x0001`, `subcmd = 0` を設定する。

| 操作 | 関数名 (`libadvancelib.so` / `8BitDoAdvance.dll`) | `cmd` | `subcmd` | `offset` | `len` (1回/全体) | `total_len` |
| --- | --- | :---: | :---: | --- | --- | --- |
| 全体読込 | `getUT2_ReadCofigChannel` | `0x0002` | `0x0000` | `0x0000` | `0x0AD0` | `0x0AD0` |
| ヘッダ読込 | `getBT2_head_CofigChannel` | `0x0002` | `0x0000` | `0x0000` | `0x0074` (116B) | `0x0074` |
| 現在スロット読込 | `getBT2_ReadCurslot_CofigChannel` | `0x0002` | `0x0000` | `0x0012` | `0x0002` (2B) | `0x0002` |
| 全体書込 | `getBT2_WriteCofigChannel` / `writeUltimateBT2` | `0x0001` | `0x0000` | `0x0000` | `0x0AD0` | `0x0AD0` |
| スロット切替書込 | `getBT2_WriteSLOTChannel(slot)` | `0x0001` | `0x0000` | `0x0012` | `0x0002` | `0x0AD0` |
| プロファイル名書込 | `getBT2_WriteFileName(slot)` | `0x0001` | `0x0000` | `0x0014 + slot * 0x20` | `0x0020` (32B) | `0x0AD0` |
| 振動書込 | `getBT2_WriteRumble(slot)` | `0x0001` | `0x0000` | `0x0074 + slot * 0x0C` | `0x000C` (12B) | `0x0AD0` |
| スティック書込 | `getBT2_WriteJoyStick(slot)` | `0x0001` | `0x0000` | `0x0098 + slot * 0x08` | `0x0008` (8B) | `0x0AD0` |
| トリガー書込 | `getBT2_WriteTrigger(slot)` | `0x0001` | `0x0000` | `0x00B0 + slot * 0x08` | `0x0008` (8B) | `0x0AD0` |
| 特殊機能書込 | `getBT2_WriteSpecial(slot)` | `0x0001` | `0x0000` | `0x00C8 + slot * 0x08` | `0x0008` (8B) | `0x0AD0` |
| 単一ボタンリマップ | `getBT2_WriteMapKey(slot, idx)` | `0x0001` | `0x0000` | `0x00E4 + slot * 0x5C + idx * 4` | `0x0004` (4B) | `0x0AD0` |
| マクロヘッダ書込 | `getBT2_WriteMacroi(slot)` / `writeBT2Macro` | `0x0001` | `0x0000` | `0x068C + slot * 0xD8` | `0x00D8` (216B) | `0x00D8` |
| マクロ名単体書込 | `writeUltimateBT2MacroName(slot, m)` | `0x0001` | `0x0000` | `0x0694 + slot * 0xD8 + m * 0x34` | `0x0020` (32B) | `0x0AD0` |
| ジャイロ書込 | `writeUltimateBT2sixaxis(slot)` *(DLL正)* | `0x0001` | `0x0000` | `0x092C + slot * 0x0C` *(※注1)* | `0x000C` (12B) | `0x0AD0` *(※注1)* |
| RGB Dynamic書込 | `getBT2_WriteDyNamic(slot)` | `0x0001` | `0x0000` | `0x0950 + slot * 0x0C` | `0x000C` (12B) | `0x0AD0` |
| RGB HotWheel書込 | `getBT2_WriteHotWheel(slot)` | `0x0001` | `0x0000` | `0x0974 + slot * 0x10` | `0x0010` (16B) | `0x0AD0` |
| RGB Single書込 | `getBT2_WriteSingle(slot)` | `0x0001` | `0x0000` | `0x09A4 + slot * 0x64` | `0x0064` (100B) | `0x0AD0` |
| 設定保存 (BLE) | `getSaveCustomConfigChannel` | `0x0006` | `0x0067` | `0x0000` | `0` | `0` |
| 設定保存 (USB) | `8BitDoAdvance.dll` 各書込末尾 | `0x0006` | `0x0123` | `0x0000` | `0` | `0` |

> **※注1 (`getBT2_WriteSixAxis` における Android `.so` のバグと Windows DLL の正しい値)**:
> Android 版 `libadvancelib.so` の `getBT2_WriteSixAxis` は `.rodata 0x1d6a0` (`{0x0, 0xC, 0xC, 0xC}`) を読み込む実装ミスによりヘッダに `offset = 0, total_len = 0x0C, len = 0x0C` を入れている（ペイロードポインタ自体は `base + slot * 0x0C + 0x92C` を指す）。一方 Windows 版 `8BitDoAdvance.dll` の `_writeUltimateBT2sixaxis@2772` では正しく **`offset = 0x92C + slot * 0x0C`, `total_len = 0x0AD0`, `len = 0x0C`** を送信している。独自実装では Windows DLL 準拠（`offset = 0x92C + slot * 0x0C, total_len = 0x0AD0`）を用いること。

### 1.3 `+0x1F4..+0x68B` (3 × 392B) および `+0x914..+0x92B` (3 × 8B) の解析結果

`libadvancelib.so`・`8BitDoAdvance.dll`・`classes.dex` の全関数を走査した結果、この 2 領域に対する解釈は以下の通り確定した。

1. **`+0x1F4..+0x68B`（合計 `0x498` = 1176 バイト = 3 スロット × `0x188` [392B]）**:
   - **アクセス箇所**: `S_UltimateBT2Advance_copyConfig` での `memcpy(dst + dst_slot*0x188 + 0x1F4, src + src_slot*0x188 + 0x1F4, 0x188)`、および `S_UltimateBT2Advance_deleteCurslot` での先頭 `u32` クリア（`*(u32*)(base + slot*0x188 + 0x1F4) = 0`）**のみ**。
   - **役割**: 他のどの `get*` / `set*` / `getBT2_Write*` JNI 関数からも読み書きされていない。旧世代（Pro 2 / Ultimate BT `2324B = 0x914` 系）における `MacroFunRecord`（`flag` 4B + `totalCnt` 4B + 4 × `MacroKeyRecord` 相当のレガシーマクロ格納枠）の名残であり、BT2 ファームウェアが `0x68C` から始まる新形式 `Macro_Pro3_Struct` を採用した際に既存ヘッダ配置との互換またはパディングとして残された領域である。
   - **Ultimate 2 Wireless (`0x638` = 1592B) との決定的な違い**: Wireless 版ではこの 1176B (`0x498`) ブロックが**完全に削除**されている（§10 参照）。
2. **`+0x914..+0x92B`（合計 `0x18` = 24 バイト = 3 スロット × `0x08` [8B]）**:
   - **アクセス箇所**: `copyConfig` での `*(u64*)(base + dst*8 + 0x914) = *(u64*)(base + src*8 + 0x914)`、および `deleteCurslot` での `*(u32*)(base + slot*8 + 0x914) = 0` **のみ**。
   - **役割**: UI・JNI・個別書込コマンドのいずれからも参照されない予約／未使用領域（Ultimate 2 Wireless では `+0x47C..+0x493` に対応位置が存在する）。
3. **実装上の必須ルール**:
   - コントローラから読み出した元の 2768 バイトバッファを保持し、これら 2 領域の既存バイト値を壊さずに書き戻すこと（新規生成時はゼロ埋め、`copy_slot` 時は 392B / 8B をそのままコピー）。

---

## 2. ヘッダ・グローバル状態 (`+0x000..+0x013`, 20 バイト)

```c
struct Bt2Header {
    uint32_t slot_flags[3]; // +0x000..+0x00B: 実機では 0x20200911 / 0xFFFFFFFF を観測。非 0 が有効の可能性があるが、正確な意味は未確定
    uint32_t crc16;         // +0x00C..+0x00F: ANSI_CRC_16_sh の戻り値 (u16をu32拡張)
    uint16_t gamepad_mode;  // +0x010..+0x011: GAMEPAD_MODE_SWIFT enum
    uint16_t cur_slot;      // +0x012..+0x013: 0..2 (active), 4=NONE
};
```

### 2.1 `slot_flags[3]` (`+0x000..+0x00B`) と `crc16` (`+0x00C..+0x00F`)

- `getFlag(slot)` / `setFlag(slot, val)` は配列インデックス `0..3` を受け付けるが、実際のプロファイルスロットは `0..=2`（`CONFIG_SLOT_SWIFT_1..3`）の 3 つであり、インデックス 3 の位置（`+0x00C..+0x00F`）は **CRC-16 フィールド** として使用される。
- `deleteCurslot(slot)` は `slot_flags[slot] = 0`（および各セクションの `flag = 0`、ファイル名 32B クリア、`cur_slot = 4`）を設定する。
- **CRC-16 アルゴリズム (`ANSI_CRC_16_sh`)**:
  - 多項式: `0x8408`（リバース済み、`CRC-16/ARC` または `CRC-16/MODBUS` 系。初期値 `0xFFFF`、最終 XOR なし、LSB-first）。
  ```rust
  pub fn ansi_crc16_sh(data: &[u8]) -> u16 {
      if data.is_empty() { return 0xFFFF; }
      let mut crc: u16 = 0xFFFF;
      for &b in data {
          crc ^= b as u16;
          for _ in 0..8 {
              if (crc & 1) != 0 {
                  crc = (crc >> 1) ^ 0x8408;
              } else {
                  crc >>= 1;
              }
          }
      }
      crc
  }
  ```
- **計算範囲と格納タイミングに関する重要注意**:
  - Android `writeBT2*`（`libadvancelib.so`）および Windows `_ANSI_CRC_16_UltimateBT2@2768`（`8BitDoAdvance.dll`）は、いずれも **`+0x00C..+0x00F` をゼロクリアしないまま** `ansi_crc16_sh(&buf[0..0x0AD0])` を全 2768 バイトに対して計算し、戻り値を `u32` として `+0x00C..+0x00F` に上書き格納している。
  - したがって公式ツールが書き込んだバッファを再読み込みした場合、`+0x00C` 自身も含めて計算されていた過去の値に依存するため、読み出し時の検証用自己整合 CRC としては機能しない場合がある。新規バッファ生成時および書き戻し時は公式仕様に合わせて全 2768 バイトで計算し `+0x00C` に格納すること（なお部分書込コマンド `getBT2_WriteRumble` 等は部分領域のみを送信するため、`+0x00C` はメモリ上でのみ更新され全体書込や `WriteUltimateBT2Name` 時に反映される）。

> **実機観測・未確定（2026-11-21）**:
> - Ultimate 2 Bluetooth から取得した 2768B バッファでは `slot_flags = [0x20200911, 0x20200911, 0xFFFFFFFF]`、`crc16` は stored `0x2C55` / in-place 再計算 `0x6A05` だった。このバッファの SHA-256 は `1dca47facfc1c0394a8893ac0a5b7ef80678c51081001886a4bf17ce2f59c600` である。
> - この SHA-256 は `eightbitdo backup --raw <FILE>` で取得した 2768B ダンプ（上記と同じバッファ）に由来する。
> - `slot_flags` は `1` / `0` ではなく大きな非 0 値である。公式 `deleteCurslot` は `slot_flags[slot]` に `0` を書き、`copyConfig` は値をコピーするだけであるため、有効判定は「非 0」である可能性がある。単純な有効/空フラグとみなす解釈は実機観測と合わない可能性があり、正確な意味は未確定である。
> - stored CRC と in-place 再計算が一致しないのは、上記のとおり公式 `ANSI_CRC_16_sh` が既存 CRC フィールドを含めて再計算する方式上、直ちに破損を意味しない。CLI はこの 2 値を情報表示するだけにしている。

### 2.2 `gamepad_mode` (`+0x010..+0x011`, `u16`)

`Ultimate2AdvanceUI$GAMEPAD_MODE_SWIFT` の enum 序数値：

| 値 | Enum 定数名 | 意味・プラットフォーム |
| :---: | --- | --- |
| `0` | `GAMEPAD_MODE_SWIFT_Switch` | Nintendo Switch モード |
| `1` | `GAMEPAD_MODE_SWIFT_GAMEPAD_J_M` | XInput / 標準ゲームパッドモード（Jモード） |
| `2` | `GAMEPAD_MODE_SWIFT_MAC_M` | Apple / macOS モード |
| `3` | `GAMEPAD_MODE_SWIFT_XINPUT_M` | XInput モード（※マクロ変換等で `mode == 3` 分岐あり） |
| `4` | `GAMEPAD_MODE_SWIFT_EXTEND_M` | 拡張モード |
| `5` | `GAMEPAD_MODE_SWIFT_GAMEPAD_NONE` | 未選択 / なし |

> `getBT2_EraseMacro` / `getBT2_WriteMacro` / `getBT2_ReadMacro` は `+0x010` の下位 8bit を `subcmd` の上位バイト `(gamepad_mode << 8) | slot` として使用する。

### 2.3 `cur_slot` (`+0x012..+0x013`, `u16`)

`Ultimate2AdvanceUI$CONFIG_SLOT_STATE_SWIFT` の enum 序数値：

| 値 | Enum 定数名 | 意味 |
| :---: | --- | --- |
| `0` | `CONFIG_SLOT_SWIFT_1` | スロット 1（プロファイル 1） |
| `1` | `CONFIG_SLOT_SWIFT_2` | スロット 2（プロファイル 2） |
| `2` | `CONFIG_SLOT_SWIFT_3` | スロット 3（プロファイル 3） |
| `3` | `CONFIG_SLOT_SWIFT_MAX` | スロット数上限マーカー |
| `4` | `CONFIG_SLOT_SWIFT_NONE` | スロット未選択 / 削除状態（オフ状態） |

> **実機観測・未確定（2026-11-21）**: Ultimate 2 Bluetooth で `cur_slot = 6` を観測した。enum の `0..=2` / `4` の範囲外であり、意味は未確定である。CLI は `0..=2` を `profile N`、`4` を `none`、それ以外を `unknown (6)` のように表示する。`deleteCurslot` が `cur_slot = 4` を書くことは解析済みだが、実機で 6 が観測される理由（別状態・未初期化・ファームウェア固有値など）は断定できない。

---

## 3. プロファイル名 (`+0x014..+0x073`, 3 × 32B)

- **配置**: `offset = 0x014 + slot * 0x20`（各 32 バイト固定長）。
- **文字エンコーディング**: **UTF-16BE（ビッグエンディアン）**。
  - **根拠 1**: `S_Files.string2byte(String)` は各 `char c` を `bytes[2*i] = (c >> 8) & 0xFF`（上位バイト先頭）、`bytes[2*i + 1] = c & 0xFF`（下位バイト後続）としてエンコードする。
  - **根拠 2**: `S_Files.unicodebyte2String(byte[])` は `bytetoChar(b[2*i], b[2*i+1])` = `(b[2*i] << 8) | b[2*i+1]` でデコードする。
  - **根拠 3**: `S_UltimateBT2Advance_copyConfig` はプロファイル複製時に `buf[dst_slot*0x20 + 0x1d] = dst_slot + '1'` を直接書き換える。`0x1d - 0x14 = 9`（5 文字目・インデックス 4 の**奇数バイト目＝下位バイト**）であり、UTF-16BE で `"Slot1"` 等（5 文字目が数字）の下位バイトを書き換えている直接証拠である。
- **長さ制限・終端処理**:
  - 最大 **16 文字**（32 バイト）。
  - 16 文字未満の場合、残りのバイトは `0x00` でゼロパディングする。
  - デコード時は 2 バイト単位（`[b0, b1]`）で走査し、`b0 == 0 && b1 == 0` が現れた時点で文字列終端とする（16 文字フル使用時は終端ヌル文字なし）。

---

## 4. 振動・スティック・トリガー・特殊機能 (`+0x074..+0x0DF`)

### 4.1 振動設定 `RumbleRecord` (`+0x074..+0x097`, 3 × 12B)

各スロット 12 バイト (`0x0C`)、`offset = 0x074 + slot * 0x0C`。

| 相対オフセット | サイズ | 型 | フィールド名 | 説明 |
| :---: | :---: | --- | --- | --- |
| `+0x00` | 4B | `u32` | `flag` | 有効フラグ (`1` = カスタム有効, `0` = デフォルト/無効) |
| `+0x04` | 4B | `f32` | `zoom_out[0]` | 左モーター倍率（IEEE-754 単精度 float LE） |
| `+0x08` | 4B | `f32` | `zoom_out[1]` | 右モーター倍率（IEEE-754 単精度 float LE） |

- **`zoom_out` 値域（6 段階）**:
  `BasicAdvanceUI.getS_VibrationValue` / `getVibrationValue` により、UI レベル `0..=5` と以下の float 値が一対一で対応する（`flag != 1` 時のデフォルトは左右とも UI レベル `5` = `1.0f`）。
  - `0.0f` (`0x00000000`) ↔ レベル 0 (OFF)
  - `0.2f` (`0x3E4CCCCD`) ↔ レベル 1
  - `0.4f` (`0x3ECCCCCD`) ↔ レベル 2
  - `0.6f` (`0x3F19999A`) ↔ レベル 3
  - `0.8f` (`0x3F4CCCCD`) ↔ レベル 4
  - `1.0f` (`0x3F800000`) ↔ レベル 5 (100%, デフォルト)

### 4.2 スティック設定 `JoyRecord` (`+0x098..+0x0AF`, 3 × 8B)

各スロット 8 バイト (`0x08`)、`offset = 0x098 + slot * 0x08`。

| 相対オフセット | サイズ | 型 | フィールド名 | 説明・デフォルト値 |
| :---: | :---: | --- | --- | --- |
| `+0x00` | 4B | `u32` | `flag` | 有効フラグ (`1` = カスタム有効, `0` = 無効) |
| `+0x04` | 1B | `u8` | `left_start_value` | 左スティック 内側デッドゾーン (`0..=127`, デフォルト `0`) |
| `+0x05` | 1B | `u8` | `left_end_value` | 左スティック 外側有効境界 (`0..=128`, デフォルト `128` = `0x80`) |
| `+0x06` | 1B | `u8` | `right_start_value` | 右スティック 内側デッドゾーン (`0..=127`, デフォルト `0`) |
| `+0x07` | 1B | `u8` | `right_end_value` | 右スティック 外側有効境界 (`0..=128`, デフォルト `128` = `0x80`) |

> `flag != 1` の場合、`BasicAdvanceUI.getS_Sticks` は `[0, 128, 0, 128]`（デッドゾーンなし・フルレンジ）として扱う。

### 4.3 トリガー設定 `TriggerRecord` (`+0x0B0..+0x0C7`, 3 × 8B)

各スロット 8 バイト (`0x08`)、`offset = 0x0B0 + slot * 0x08`。

| 相対オフセット | サイズ | 型 | フィールド名 | 説明・デフォルト値 |
| :---: | :---: | --- | --- | --- |
| `+0x00` | 4B | `u32` | `flag` | 有効フラグ (`1` = カスタム有効, `0` = 無効) |
| `+0x04` | 1B | `u8` | `left_start_value` | LT (L2) 開始デッドゾーン (`0..=255`, デフォルト `0`, ヘアトリガーモード時 `77` = `0x4D`) |
| `+0x05` | 1B | `u8` | `left_end_value` | LT (L2) 終了/最大点 (`0..=255`, デフォルト `255` = `0xFF`) |
| `+0x06` | 1B | `u8` | `right_start_value` | RT (R2) 開始デッドゾーン (`0..=255`, デフォルト `0`, ヘアトリガーモード時 `77` = `0x4D`) |
| `+0x07` | 1B | `u8` | `right_end_value` | RT (R2) 終了/最大点 (`0..=255`, デフォルト `255` = `0xFF`) |

### 4.4 特殊機能フラグ `JoyTriggerSpecialRecord` (`+0x0C8..+0x0DF`, 3 × 8B)

各スロット 8 バイト (`0x08`)、`offset = 0x0C8 + slot * 0x08`。

| 相対オフセット | サイズ | 型 | フィールド名 | 説明 |
| :---: | :---: | --- | --- | --- |
| `+0x00` | 4B | `u32` | `flag` | 有効フラグ (`1` = 有効, `0` = 無効) |
| `+0x04` | 4B | `u32` | `feature` | 特殊機能ビットマスク（Java側フィールド名綴りは `featrue`） |

#### `feature` ビットマスク詳細（`BasicAdvanceUI.getSpecialFeature` / `getS_SpecialFeature` 検証済み）

| ビット | マスク値 | Java フラグ変数 | 機能内容 |
| :---: | :---: | --- | --- |
| bit 0 | `0x0000_0001` | `S_Sticks.is_Lx_Filp` | 左スティック X 軸反転 |
| bit 1 | `0x0000_0002` | `S_Sticks.is_Ly_Filp` | 左スティック Y 軸反転 |
| bit 2 | `0x0000_0004` | `S_Sticks.is_Rx_Filp` | 右スティック X 軸反転 |
| bit 3 | `0x0000_0008` | `S_Sticks.is_Ry_Filp` | 右スティック Y 軸反転 |
| bit 4 | `0x0000_0010` | `S_Sticks.isLR_Filp` | 左/右スティック入れ替え (Swap L/R Sticks) |
| bit 5..6 | `0x0000_0060` | *(未使用)* | `getS_SpecialFeature` で参照されず（0 維持） |
| bit 7 | `0x0000_0080` | `S_Triggers.isLR_Filp` | L2 / R2 トリガー入れ替え (Swap LT/RT) |
| bit 8 | `0x0000_0100` | `S_Sticks.isDpad_Filp` | 左スティック / 十字キー(D-Pad) 入れ替え |
| bit 9 | `0x0000_0200` | *(未使用)* | `getS_SpecialFeature` で参照されず（0 維持） |
| bit 10 | `0x0000_0400` | `S_Sticks.isRS_Trigger_Filp` | 右スティック・トリガー反転/切替フラグ |
| bit 11 | `0x0000_0800` | `S_Vibration.isMotionSensitivity` | モーション感度フラグ |
| bit 12 | `0x0000_1000` | `S_Sticks.DeadZore` | ゼロデッドゾーン（非デッドゾーン）モード |
| bit 13 | `0x0000_2000` | `S_Sticks.Four` | 4 方向入力モード (4-Way D-Pad) |
| bit 14 | `0x0000_4000` | `S_Vibration.isTrigger` | トリガー振動（インパルストリガー）有効 |
| bit 15 | `0x0000_8000` | `S_Vibration.isVibration` | 本体振動モーター有効 |
| bit 20 | `0x0010_0000` | `S_Sticks.joyShare == 1` | ジョイスティック共有/スナップ系モード 1 |
| bit 21 | `0x0020_0000` | `S_Sticks.joyShare == 2` | ジョイスティック共有/スナップ系モード 2 |

---

## 5. ボタンリマッピング (`MapKeyRecord`, `+0x0E0..+0x1F3`) と 32-bit `KeyMap` 完全表

### 5.1 `MapKeyRecord` 構造（3 × 92B, `0x5C`）

`offset = 0x0E0 + slot * 0x5C`。

| 相対オフセット | 全体オフセット (slot `s`) | サイズ | 型 | フィールド名 | 説明 |
| :---: | :---: | :---: | --- | --- | --- |
| `+0x00` | `0x0E0 + s * 0x5C` | 4B | `u32` | `flag` | 有効フラグ (`1` = リマップ有効, `0` = デフォルト) |
| `+0x04..+0x5B` | `0x0E4 + s * 0x5C` | 88B | `[u32; 22]` | `key[22]` | 22 個の物理ボタンに対応するマッピング先 `KeyMap` 値 |

- 部分書込 `getBT2_WriteMapKey(slot, key_idx)` は `flag` を除く **`key[key_idx]` 単体（4B）** を `offset = 0x0E4 + slot * 0x5C + key_idx * 4`, `len = 4`, `total_len = 0x0AD0` で書き込む。

### 5.2 `key[22]` 配列インデックス対応表（Ultimate 2 / Non-3E レイアウト）

`BasicAdvanceUI.getMappings()` および `getS_Mappings(MapKeyRecord)` による全 22 インデックス（`0..=21`）の物理ボタン対応は以下の通り。

| Index | 相対 Offset | 物理ボタン (UI/KeyMap) | デフォルト値 (非Switch時) | Switch モード時の注意 (`isSwitchMode`) |
| :---: | :---: | --- | --- | --- |
| `0` | `+0x04` | **A** (物理下ボタン / Switch: **B**) | `KeyMap_A` (`0x0000_2000`) | Switch 表記では配列 Index `1` と A/B が交差表示 |
| `1` | `+0x08` | **B** (物理右ボタン / Switch: **A**) | `KeyMap_B` (`0x0000_1000`) | Switch 表記では配列 Index `0` と A/B が交差表示 |
| `2` | `+0x0C` | **X** (物理左ボタン / Switch: **Y**) | `KeyMap_X` (`0x0000_0010`) | Switch 表記では配列 Index `3` と X/Y が交差表示 |
| `3` | `+0x10` | **Y** (物理上ボタン / Switch: **X**) | `KeyMap_Y` (`0x0000_0020`) | Switch 表記では配列 Index `2` と X/Y が交差表示 |
| `4` | `+0x14` | **L1 / LB** | `KeyMap_L1` (`0x0000_0400`) | 同じ |
| `5` | `+0x18` | **R1 / RB** | `KeyMap_R1` (`0x0000_0800`) | 同じ |
| `6` | `+0x1C` | **L2 / LT** | `KeyMap_L2` (`0x0000_4000`) | 同じ |
| `7` | `+0x20` | **R2 / RT** | `KeyMap_R2` (`0x0000_8000`) | 同じ |
| `8` | `+0x24` | **L3 / LS** (左スティック押し込み) | `KeyMap_L3` (`0x0000_0002`) | 同じ |
| `9` | `+0x28` | **R3 / RS** (右スティック押し込み) | `KeyMap_R3` (`0x0000_0004`) | 同じ |
| `10` | `+0x2C` | **Select / View / Minus (-)** | `KeyMap_Select` (`0x0000_0008`) | 同じ |
| `11` | `+0x30` | **Start / Menu / Plus (+)** | `KeyMap_Start` (`0x0000_0001`) | 同じ |
| `12` | `+0x34` | **Share / Star / Turbo / Screenshot** | `KeyMap_Share` (`123`) | Switch 時はデフォルト `KeyMap_ScreenShot`、XInput 時 `KeyMap_Turbo` 等として UI 表示 |
| `13` | `+0x38` | **Logo / Home** | `KeyMap_Logo` (`124`) | UI 表示は `KeyMap_Home` 相当 |
| `14` | `+0x3C` | **D-Pad Up** | `KeyMap_Up` (`0x0000_0200`) | 同じ |
| `15` | `+0x40` | **D-Pad Down** | `KeyMap_Down` (`0x0000_0100`) | 同じ |
| `16` | `+0x44` | **D-Pad Left** | `KeyMap_Left` (`0x0000_0080`) | 同じ |
| `17` | `+0x48` | **D-Pad Right** | `KeyMap_Right` (`0x0000_0040`) | 同じ |
| `18` | `+0x4C` | **P1 (背面パドル左 / PR1)** | `KeyMap_P1` (`0x0200_0000`) または `KeyMap_N` (`0`) | 未割当時は `0` |
| `19` | `+0x50` | **P2 (背面パドル右 / PR2)** | `KeyMap_P2` (`0x0400_0000`) または `KeyMap_N` (`0`) | 未割当時は `0` |
| `20` | `+0x54` | **P3 (追加L4バンパー / PL4)** | `KeyMap_P3` (`0x0020_0000`) または `KeyMap_N` (`0`) | 未割当時は `0` |
| `21` | `+0x58` | **P4 (追加R4バンパー / PR4)** | `KeyMap_P4` (`0x4000_0000`) または `KeyMap_N` (`0`) | 未割当時は `0` |

> **※3E系（8BitDo Ultimate 3-mode / `is3E() == true`）との配列順の違いに注意**:
> `getMappings()` の Non-3E（Ultimate 2 Bluetooth を含む）分岐は上記の 22 要素順 `[A, B, X, Y, L1, R1, L2, R2, L3, R3, Select, Start, Share, Logo, Up, Down, Left, Right, P1, P2, P3, P4]` である（3E 系は `[L3, R3, P3, P4, Up..]` の 20 要素）。

### 5.3 32-bit `KeyMap` 定数・ビットマスク完全表

`com.bbitdo.advanceandroidv2.mode.mapping.KeyMap` の全 36 静的フィールド（10進数・16進数・ビット位置）を以下に示す。

#### 単一ビット定数（ビットマスク合成・マクロ `keys`・特殊割り当て共通）

| ビット | 16進値 (`u32`) | 10進値 | 定数名 | ボタン / 機能 |
| :---: | :---: | ---: | --- | --- |
| bit 0 | `0x0000_0001` | 1 | `KeyMap_Start` | Start / Menu / + |
| bit 1 | `0x0000_0002` | 2 | `KeyMap_L3` | L3 (左スティック押込) |
| bit 2 | `0x0000_0004` | 4 | `KeyMap_R3` | R3 (右スティック押込) |
| bit 3 | `0x0000_0008` | 8 | `KeyMap_Select` | Select / View / - |
| bit 4 | `0x0000_0010` | 16 | `KeyMap_X` | X |
| bit 5 | `0x0000_0020` | 32 | `KeyMap_Y` | Y |
| bit 6 | `0x0000_0040` | 64 | `KeyMap_Right` | D-Pad Right |
| bit 7 | `0x0000_0080` | 128 | `KeyMap_Left` | D-Pad Left |
| bit 8 | `0x0000_0100` | 256 | `KeyMap_Down` | D-Pad Down |
| bit 9 | `0x0000_0200` | 512 | `KeyMap_Up` | D-Pad Up |
| bit 10 | `0x0000_0400` | 1,024 | `KeyMap_L1` | L1 / LB |
| bit 11 | `0x0000_0800` | 2,048 | `KeyMap_R1` | R1 / RB |
| bit 12 | `0x0000_1000` | 4,096 | `KeyMap_B` | B |
| bit 13 | `0x0000_2000` | 8,192 | `KeyMap_A` | A |
| bit 14 | `0x0000_4000` | 16,384 | `KeyMap_L2` | L2 / LT |
| bit 15 | `0x0000_8000` | 32,768 | `KeyMap_R2` | R2 / RT |
| bit 16 | `0x0001_0000` | 65,536 | `KeyMap_Turbo` | 連射 (Turbo / 手動連射) |
| bit 17 | `0x0002_0000` | 131,072 | `KeyMap_Home` | Home |
| bit 18 | `0x0004_0000` | 262,144 | `KeyMap_BT_CON` | Bluetooth ペアリング/接続系 |
| bit 21 | `0x0020_0000` | 2,097,152 | `KeyMap_P3` | P3 (追加バンパー L4) |
| bit 22 | `0x0040_0000` | 4,194,304 | `KeyMap_ScreenShot` | スクリーンショット (Switch キャプチャ) |
| bit 23 | `0x0080_0000` | 8,388,608 | `KeyMap_Turbo_3` | Turbo 3段階 / 特殊 Turbo |
| bit 24 | `0x0100_0000` | 16,777,216 | `KeyMap_Turbo_Auto` | 自動連射 (Hold/Auto Turbo) |
| bit 25 | `0x0200_0000` | 33,554,432 | `KeyMap_P1` | P1 (背面パドル左) |
| bit 26 | `0x0400_0000` | 67,108,864 | `KeyMap_P2` | P2 (背面パドル右) |
| bit 27 | `0x0800_0000` | 134,217,728 | `KeyMap_Swap` | プロファイル/ボタン Swap |
| bit 30 | `0x4000_0000` | 1,073,741,824 | `KeyMap_P4` | P4 (追加バンパー R4) |

#### 無効化 / マクロ・システム識別用 非ビットマスク定数

| 値 (`u32`) | 16進値 | 定数名 | 用途 |
| ---: | :---: | --- | --- |
| `0` | `0x0000_0000` | `KeyMap_N` | **無効化 / 未割当 (None / Unmapped)** |
| `119` | `0x0000_0077` | `KeyMap_M1` | マクロ 0 識別値（インデックス 0） |
| `120` | `0x0000_0078` | `KeyMap_M2` | マクロ 1 識別値（インデックス 1） |
| `121` | `0x0000_0079` | `KeyMap_M3` | マクロ 2 識別値（インデックス 2） |
| `122` | `0x0000_007A` | `KeyMap_M4` | マクロ 3 識別値（インデックス 3） |
| `123` | `0x0000_007B` | `KeyMap_Share` | Share ボタン識別（`getS_Mappings` の元キー側にも使用） |
| `124` | `0x0000_007C` | `KeyMap_Logo` / `KeyMap_View` | Logo / View ボタン識別（同一値 `124`） |
| `227` | `0x0000_00E3` | `KeyMap_My_NUll` | 無効化/未定義プレースホルダ（UI内部表現） |

> **マクロとの関連**: マクロをボタンに割り当てる場合、実際の設定ではボタン側のリマップエントリを無効 (`KeyMap_N`) にするか、`MacroFunPro3Record.key_map` に起動トリガーキー（例: `KeyMap_P1`, `KeyMap_L2` 等の 32-bit `KeyMap` 値）を書き込む仕様となっている（UI では最大 4 つのマクロに対し `M1..M4` を割り当て管理する）。

---

## 6. 6軸ジャイロ / モーション (`SixAxis`, `+0x92C..+0x94F`, 3 × 12B)

`offset = 0x92C + slot * 0x0C`。

| 相対オフセット | サイズ | 型 | フィールド名 | 値域 / Enum / デフォルト |
| :---: | :---: | --- | --- | --- |
| `+0x00` | 4B | `u32` | `flag` | `1` = 有効 (`flag_enable`), `0` = 無効 |
| `+0x04` | 4B | `u32` | `key_map` | ジャイロ有効化ボタン（32-bit `KeyMap` ビットマスク。`0` = 常時または未指定） |
| `+0x08` | 1B | `u8` | `trigger_mode` | トリガー方式:<br>`0` = **Hold** (押している間有効)<br>`1` = **Toggle** (1度押しでON、もう一度押しでOFF) |
| `+0x09` | 1B | `u8` | `sensitivity` | 感度レベル（UI 基準 `1..=15` 系、デフォルト `5`） |
| `+0x0A` | 1B | `u8` | `dead_compensate` | デッドゾーン補償量 (`0..=255`, `Ultimate2AdvanceUI.resetFile` デフォルト `40` [`0x28`]) |
| `+0x0B` | 1B | `u8` | `mapping_type` | マッピング先 (`MotionContentFragment.RefreshView` 準拠):<br>`0` = **Left Stick** (左スティック)<br>`1` = **Right Stick** (右スティック)<br>`2` = **Mouse** (マウスエミュレーション) |

> **書込時の注意**: Android `libadvancelib.so` の `getBT2_WriteSixAxis` バグ（`offset=0, total_len=0x0C`）ではなく、Windows 版 `8BitDoAdvance.dll` と同様 **`offset = 0x92C + slot * 0x0C`, `len = 0x0C`, `total_len = 0x0AD0`** で書き込むこと（§1.2 注1）。

---

## 7. RGB ライティング (`+0x950..+0xACF`)

### 7.1 色フォーマット

全テーマ構造体の色は **`u32` (Little-Endian) の ARGB 形式 (`0xAARRGGBB`)** で格納される（Alpha チャネルは通常 `0xFF` 固定。メモリ/ワイヤーバイト順は `[B, G, R, A]`）。

`Ultimate2AdvanceUI.resetFile` での実測デフォルト色:
- `Dynamic`: `color = 0xFF260032`, `bg_color = 0xFF084E50`
- `HotWheel`: `color = 0xFFCC6200` (オレンジ), `bg_color = 0xFFCC0000` (赤)
- `Single`: 全 24 LED が `0xFF7E3200`

### 7.2 `Dynamic_Theme` (`+0x950..+0x973`, 3 × 12B)

`offset = 0x950 + slot * 0x0C`。

| 相対オフセット | サイズ | 型 | フィールド名 | 説明 |
| :---: | :---: | --- | --- | --- |
| `+0x00` | 4B | `u32` | `flag` | 有効フラグ (`1` = 有効, `0` = 無効) |
| `+0x04` | 4B | `u32` | `color` | 前景色 (`0xAARRGGBB`) |
| `+0x08` | 4B | `u32` | `bg_color` | 背景色 (`0xAARRGGBB`) |

### 7.3 `HotWheel_Theme` (`+0x974..+0x9A3`, 3 × 16B)

`offset = 0x974 + slot * 0x10`。

| 相対オフセット | サイズ | 型 | フィールド名 | 説明 |
| :---: | :---: | --- | --- | --- |
| `+0x00` | 4B | `u32` | `flag` | 有効フラグ (`1` = 有効, `0` = 無効) |
| `+0x04` | 4B | `u32` | `color` | 前景色 (`0xAARRGGBB`) |
| `+0x08` | 4B | `u32` | `bg_color` | 背景色 (`0xAARRGGBB`) |
| `+0x0C` | 1B | `u8` | `speed` | エフェクト速度 (`0..=15`, デフォルト `7`) |
| `+0x0D..+0x0F` | 3B | `[u8; 3]` | `reserved` | アライメント用パディング (`0x00`) |

### 7.4 `Single_Theme` (`+0x9A4..+0xACF`, 3 × 100B)

`offset = 0x9A4 + slot * 0x64`（`100` バイト）。

| 相対オフセット | サイズ | 型 | フィールド名 | 説明 |
| :---: | :---: | --- | --- | --- |
| `+0x00` | 4B | `u32` | `flag` | 有効フラグ (`1` = 有効, `0` = 無効) |
| `+0x04..+0x63` | 96B | `[u32; 24]` | `colors[24]` | 左右スティックリング周上の 24 LED 個別色 (`0xAARRGGBB` × 24) |

---

## 8. マクロ管理ヘッダ (`Macro_Pro3_Struct`, `+0x68C..+0x913`, 3 × 216B)

各スロット 216 バイト (`0xD8`)、`offset = 0x68C + slot * 0xD8`。

### 8.1 `Macro_Pro3_Struct` (216B / スロット)

| 相対オフセット | サイズ | 型 | フィールド名 | 説明 |
| :---: | :---: | --- | --- | --- |
| `+0x00` | 4B | `u32` | `flag` | マクロ機能有効フラグ (`1` = 有効, `0` = 無効) |
| `+0x04` | 1B | `u8` | `total_cnt` | 登録済みマクロ数 (`0..=4`) |
| `+0x05..+0x07` | 3B | `[u8; 3]` | `reserved` | 4B アライメントパディング (`0x00`) |
| `+0x08..+0xD7` | 208B | `[MacroFunPro3Record; 4]` | `records[4]` | 4 つのマクロ定義（各 52B = `0x34`） |

### 8.2 `MacroFunPro3Record` (52B = `0x34` / マクロ)

スロット `slot`・マクロ番号 `m` (`0..=3`) の開始位置は `0x68C + slot * 0xD8 + 0x08 + m * 0x34`（= `0x694 + slot * 0xD8 + m * 0x34`）。

| 相対オフセット | サイズ | 型 | フィールド名 | 説明 |
| :---: | :---: | --- | --- | --- |
| `+0x00..+0x1F` | 32B | `[u8; 32]` | `filename` | マクロ名（UTF-16BE、最大16文字、ゼロパディング） |
| `+0x20` | 1B | `u8` | `gamepad_mode` | 対象コントローラモード (`GAMEPAD_MODE_SWIFT`) |
| `+0x21` | 1B | `u8` | `special_flag` | 特殊フラグ（通常 `0`） |
| `+0x22..+0x23` | 2B | `u16` | `max_steps` | **実ステップ数** (`0..=200`)。Flash 読込・書込バイト数の算出基準 |
| `+0x24..+0x25` | 2B | `u16` | `offset` | Flash オフセット参照またはアプリ管理用インデックス値 |
| `+0x26..+0x27` | 2B | `[u8; 2]` | `reserved` | アライメントパディング (`0x00`) |
| `+0x28..+0x2B` | 4B | `u32` | `key_map` | マクロを起動するトリガーキーの 32-bit `KeyMap` 値 (`0` = 未割当) |
| `+0x2C..+0x2F` | 4B | `u32` | `cycles_num` | ループ回数 (`0` = 無限ループ/ホールド、`1` 以上 = 指定回数、UI上では繰り返し回数) |
| `+0x30..+0x33` | 4B | `u32` | `interval_ms` | ループ間の待機時間（ミリ秒 `ms`） |

`max_steps` の絶対 offset は `0x68C + slot * 0xD8 + 0x08 + macro_idx * 0x34 + 0x22`（= `0x6B6 + slot * 0xD8 + macro_idx * 0x34`、`u16` LE）。`+0x08` はスロット内 `records[4]` の先頭、`+0x22` は各 `MacroFunPro3Record` 内のフィールド位置。

---

## 9. マクロステップバイナリ (`MacroDatas` 10 バイト) と Flash 読み書き仕様

### 9.1 `MacroDatas` 10 バイト構造

`S_UltimateBT2Advance_setMacroStruct` および `getMacro_Struct` でのメモリレイアウト（1 ステップ = 10 バイト、`u16[5]` 相当の Little-Endian 配置）:

| オフセット | サイズ | 型 | フィールド名 | 値域・エンコーディング詳細 |
| :---: | :---: | --- | --- | --- |
| `+0x00..+0x01` | 2B | `u16` | `ms_times` | このステップの持続時間 / 次ステップまでの遅延（**1ms 単位**、例: `50` = 50ms） |
| `+0x02..+0x03` | 2B | `u16` | `keys` | 押下ボタンのビットマスク（`KeyMap` の下位 16bit: bit 0〜15 = Start..R2） |
| `+0x04` | 1B | `u8` | `rt_value` | 右トリガー R2/RT アナログ値 (`0x00` = 解放 .. `0xFF` = 全押し) ※`trigger_value` 下位バイト |
| `+0x05` | 1B | `u8` | `lt_value` | 左トリガー L2/LT アナログ値 (`0x00` = 解放 .. `0xFF` = 全押し) ※`trigger_value` 上位バイト |
| `+0x06` | 1B | `u8` | `left_x` | 左スティック X 軸 (`0x00` = 左端, `0x7F` = ニュートラル, `0xFF` = 右端) ※`left_joy` 下位バイト |
| `+0x07` | 1B | `u8` | `left_y` | 左スティック Y 軸 (`0x00` = 上端, `0x7F` = ニュートラル, `0xFF` = 下端) ※`left_joy` 上位バイト |
| `+0x08` | 1B | `u8` | `right_x` | 右スティック X 軸 (`0x00` = 左端, `0x7F` = ニュートラル, `0xFF` = 右端) ※`right_joy` 下位バイト |
| `+0x09` | 1B | `u8` | `right_y` | 右スティック Y 軸 (`0x00` = 上端, `0x7F` = ニュートラル, `0xFF` = 下端) ※`right_joy` 上位バイト |

> **Java `MacroDatas` (`u16` 結合値) との対応検証 (`BasicAdvanceUI.getMacroRights` / `addRightSticks` / `addTrigger`)**:
> - `trigger_value` (`u16` LE): L2+R2=`0xFFFF`, L2のみ=`0xFF00` (上位=LT), R2のみ=`0x00FF` (下位=RT), なし=`0x0000`。
> - `left_joy` / `right_joy` (`u16` LE): ニュートラル=`0x7F7F` (`X=0x7F, Y=0x7F`)、上=`0x007F` (`X=0x7F, Y=0x00`)、下=`0xFF7F` (`X=0x7F, Y=0xFF`)、左=`0x7F00` (`X=0x00, Y=0x7F`)、右=`0x7FFF` (`X=0xFF, Y=0x7F`)。
> - アナログ値自体は **フル 8-bit (`0..=255`)** であり、ハードウェア記録時は任意の中間値もそのまま再生可能。
> - **最大ステップ数**: `BasicAdvanceUI.getPro3MacrosStruct` は 200 要素 (`0xC8`) 配列を確保する（200 × 10B = **2000 バイト** = `0x7D0`）。
> - **ホスト側内部バッファ構造 (`macro_record_content_t_ini_btu2`)**:
>   - スロットあたり `0x1F70` (8048B) = 4 マクロ × `0x7DC` (2012B)。
>   - 各マクロ `0x7DC` バイトの内訳: `+0x00..+0x01` `count: u16`, `+0x02..+0x03` pad, `+0x04..+0x07` `cycles_num: u32`, `+0x08..+0x0B` `interval_ms: u32`, **`+0x0C..+0x7DB` `macro_datas[200]` (200 × 10B = 2000B)**。Flash へ転送されるのはこのうち `+0x0C` 以降の `MacroDatas` 配列部分のみである。

### 9.2 `total_len` 32B アライメント計算と Flash セクタ配置

> **重要（`PROTOCOL_SPEC.md` 突合・補正結果）**:
> `PROTOCOL_SPEC.md` に記載されていた `align32(steps * 2)` は誤記であり、実コード（`libadvancelib.so` `getBT2_WriteMacro` 1058行目〜）では **`max_steps * 10` を 32 バイト境界に切り上げる**。

- **Flash セクタアドレス (`offset`)**:
  `offset = (macro_idx as u32) << 12`
  - マクロ 0 → `0x0000`
  - マクロ 1 → `0x1000` (4096)
  - マクロ 2 → `0x2000` (8192)
  - マクロ 3 → `0x3000` (12288)
- **ヘッダ `subcmd` 計算**:
  `subcmd = ((gamepad_mode as u16) << 8) | (slot as u16)`
  - 例: `Switch (0)`, `slot = 0` → `subcmd = 0x0000`
  - 例: `XINPUT_M (3)`, `slot = 1` → `subcmd = 0x0301`
  - ※注意: Flash セクタ番号は `macro_idx << 12` であり、スロット番号は `offset` ではなく `subcmd` の下位バイトに載る（コントローラ側がセクタまたは `mode|slot` に応じてバンク管理する）。
- **バイト長 (`total_len`) 計算式**:
  - **Raw バイト長**: `raw_bytes = (max_steps as u32) * 10`
  - **Write (`cmd 0x0103`) の `total_len`**: **32 バイト切り上げ**
    ```rust
    let raw_bytes = (max_steps as u32) * 10;
    let aligned_len = (raw_bytes + 31) & !31;
    // libadvancelib.so の実装:
    // if (raw_bytes & 0x1e) == 0 { raw_bytes } else { (raw_bytes & !0x1f) + 0x20 }
    // (raw_bytes は 10 の倍数＝常に偶数なので bit0 は 0、& 0x1e == 0 は 32 の倍数判定と等価)
    ```
    - 例: `max_steps = 4` → `raw_bytes = 40` → `aligned_len = 64`（不足分 24 バイトはホスト側バッファ上のゼロ詰め領域が送られる）
    - 例: `max_steps = 16` → `raw_bytes = 160` → `aligned_len = 160`
  - **Read (`cmd 0x0102`) の `total_len`**: アライメントなしの `max_steps * 10` をそのまま指定。
  - **Erase (`cmd 0x0104`) の `total_len` / `chunk_len` / `crc16`**: `total_len` は `0x1000` (4096 バイト = 1 セクタ分)。ペイロードを持たないが、公式実装は `chunk_len` も `0x1000` に設定する（wire header `+0x04` はその値。実ペイロードは `len = 0`）。`gamepad_type == 1` の CRC 計算は空ペイロードに対して行われ、`crc16` は長さ `<= 0` で `0xFFFF` を返すため、wire header `+0x06` は `0xFFFF` となる。

### 9.3 Flash 読み書き完全シーケンス

#### マクロ書き込み手順（必ずこの順序を守ること）

1. **Step 1: マクロ管理ヘッダ更新 (`Header`)**
   - `cmd = 0x0001`
   - `subcmd = 0x0000`
   - `offset = 0x68C + slot * 0xD8`
   - **`total_len = 0xD8` (216)**（公式 `getBT2_WriteMacroi` / `writeBT2Macro`。`0x0AD0` ではない）
   - 216B を `chunk_len = min(142, mtu - 3 - 17)` で分割（MTU 200 なら 142 + 74）。`offset` は実チャンク長だけ進む。
   - `ANSI_CRC_16_sh` は 2768B 全体で再計算するが、216B の wire には `+0x00C` を含めない。
2. **Step 2: ヘッダ Save (`Save`)**
   - **BLE**: `cmd = 0x0006, subcmd = 0x0067, offset = 0, len = 0, total_len = 0`
   - 公式 `writeBT2Macro_onfinished` はヘッダ書込完了時に Save を送る。
   - Header またはこの Save が失敗した場合、Erase は送らない。
3. **Step 3: Flash セクタ消去 (`Erase`)**
   - `cmd = 0x0104`
   - `subcmd = (gamepad_mode << 8) | slot`
   - `offset = macro_idx << 12`
   - `total_len = 0x1000`（消去サイズ。ペイロード長ではない）
   - `chunk_len = 0x1000`（wire header `+0x04`。ペイロードなし、`len = 0`）
   - `crc16 = 0xFFFF`
   - ACK 後 **100ms** 待ってから Write を送る（`MACRO_ERASE_SETTLE_DELAY`、暫定値）。
4. **Step 4: マクロステップデータ書込 (`Write`)**
   - `cmd = 0x0103`
   - `subcmd = (gamepad_mode << 8) | slot`
   - `offset = (macro_idx << 12) + chunk_index * chunk_size`
   - `total_len = align32(max_steps * 10)`
   - `chunk_size` は MTU 連動（64 / 32）、`--chunk-size 32` で **32B 均一**（460B → 480B = 15 × 32B、500B → 512B = 16 × 32B）。32/64 以外は `UnsupportedChunkSize`。
   - ペイロード: `max_steps * 10` バイト＋32B アライメント用ゼロパディング。各チャンクに CRC-16/MODBUS。
   - **最終チャンクの ACK 後 50ms** 待ってから次の Save を送る（`MACRO_WRITE_SETTLE_DELAY`、暫定値）。
5. **Step 5: 最終 Save (`Save`)**
   - **BLE**: `cmd = 0x0006, subcmd = 0x0067, offset = 0, len = 0, total_len = 0`
   - 公式 `EraseMacroDataDone` / `WriteMacroDataDone` の連鎖後に Save を送る。
6. **Step 6: 読み戻し検証 (`Read`)**
   - 最終 Save ACK 後 **300ms** 待ってから `cmd = 0x0002` で 2768B config を読み、対象 slot の **216B ヘッダ**が送信した `header_bytes` と一致することを確認する（`MACRO_VERIFY_DELAY`、暫定値）。
   - 続けて `cmd = 0x0102` で `(slot, macro_idx)` を読み、`max_steps * 10` バイトが書き込んだ `MacroDatas` と一致することを確認する。

> ACK は各コマンド送信後の transport `wait_ack` で待つ。マクロの `Erase` / `Write` / ヘッダは 2 秒（実装定数 `0x44fa0000`）。マクロ `Save` は 3 秒で **リトライしない**（USB HID の仕様記載。BLE は未文書化のため暫定で 3 秒を使う）。OTA の 10 秒 erase / 1 秒 chunk / 64B 固定はマクロに適用しない。

> **実装ノート（Mock 検証済み・実機書き込み未検証）**:
> - **実機観測（2026-11-22）**: 旧順序（Erase → Write → Header(total_len=0x0AD0) → Save）で macro 0（500B）は成功し、macro 1（460B）だけが全 ACK 後に `0x0102` zeros で Verify 失敗した。216B ヘッダは一致していた。公式 RE の **Header(total_len=0xD8) → Save → Erase/Write → Save** との差分を原因候補とし、本実装を公式順序に変更した。最終 32B チャンク仮説に対しては `--chunk-size 32` も利用できる。RE 根拠: `getBT2_WriteMacroi` / `writeCustomConfig_onfinished` / `writeBT2Macro_onfinished` / `EraseMacroDataDone` / `WriteMacroDataDone`。
> - **Fail-closed**: Header または最初の Save が失敗したら Erase を送らない。Erase / Write の失敗時はヘッダが既に Save 済みなので `saved = true` とし、バックアップから復元する。最終 Save / Verify の失敗も `saved = true` で、再書き込みしない。
> - **空きマクロ拒否**: `max_steps == 0` / `0xFFFF` への `macro write` は `Error::EmptyMacroWrite` で拒否する。step data の clear が未実装で、`macro write` では新規作成できない。`macro restore` はバックアップが非空なら現在のマクロが空でも書ける（pre-restore backup と 216B ヘッダ更新を使う）。
> - **ヘッダ値**: `max_steps` は新しいステップ数。`key_map` / `cycles_num` / `interval_ms` / `flag` / `total_cnt` は指定がなければ既存値を保持する。既存の予約バイト（slot `+0x05..+0x07`、record `+0x26..+0x27`）も保持する。
> - **CRC**: 216B の Header 部分書込には config CRC (`+0x00C`) を含めない（公式の部分書込と同じ）。Save 時に device が CRC を再計算するかは実機未確認。
> - Header / Write のチャンク長は **MTU 連動**。`max_payload = mtu - 3 - 17` とし、Header は `min(142, max_payload)`、Write は `max_payload >= 64` → 64B、`>= 32` → 32B、それ未満は書き込み開始前に `Error::MtuTooSmall`。MTU 200 は 142 + 74 / 64B、MTU 64 は 44×4 + 40 / 32B、MTU 23 は Header 3×72 だが Write 不可。Mock で全フレームが `mtu - 3` 以下であることを固定済み。実機は MTU 200 のみを想定検証する。
> - **USB**: USB 32B 書き込みは未実装。`SaveTransport::Usb` は `Error::Unsupported` で拒否し、CLI は BLE のみを使う。

#### マクロ読み出し手順

1. 先に 2768B コンフィグ（または `Macro_Pro3_Struct`）を読み出し、対象 `(slot, macro_idx)` の `max_steps` を取得する。
2. `max_steps > 0` の場合、Flash 読み出しを発行する：
   - `cmd = 0x0102`
   - `subcmd = (gamepad_mode << 8) | slot`
   - `offset = (macro_idx << 12) + chunk_offset`
   - `total_len = chunk_len = max_steps * 10`（全長のまま）
   - 応答ペイロードを 10 バイト単位で `MacroDatas` としてデコードする。
3. **1 要求 = 1 チャンク**（実機 2026-11-22 観測: 142B）。`chunk_offset += payload.len()` で `offset` を進めて再要求する。実機では応答 `offset` が要求した絶対 offset をエコーした（macro 0 / 500B: `[0, 142, 284, 426]`、macro 1 / 460B: `[4096, 4238, 4380, 4522]`）。`macro backup` は互換のため 0（逐次）とセクタ相対 offset も受け入れるが、`response_offsets` に実際の値を記録する。最終チャンクは残り長に切り詰める。

---

## 10. 参考: Ultimate 2 Wireless (`0x6012`, 1592B `0x638`) との主な差分メモ

Windows 版 `8BitDoAdvance.dll` の `_writeUltimate2*@1596` 群の逆アセンブル結果との比較により、**Ultimate 2 Wireless (`0x6012`, 1592 バイト = `0x638`)** とのレイアウト差分は以下の通り完全に特定されている。

| セクション | Ultimate 2 Bluetooth (`0x600F`, `0xAD0` = 2768B) | Ultimate 2 Wireless (`0x6012`, `0x638` = 1592B) | 差分 |
| --- | --- | --- | --- |
| ヘッダ (`flag[3]`, `crc`, `mode`, `cur_slot`) | `0x000`〜`0x013` (20B) | `0x000`〜`0x013` (20B) | 同一 |
| プロファイル名 (`fileName[3]`) | `0x014`〜`0x073` (96B) | `0x014`〜`0x073` (96B) | 同一 |
| 振動 (`Rumble[3]`) | `0x074`〜`0x097` (36B) | `0x074`〜`0x097` (36B) | 同一 |
| スティック (`Joy[3]`) | `0x098`〜`0x0AF` (24B) | `0x098`〜`0x0AF` (24B) | 同一 |
| トリガー (`Trigger[3]`) | `0x0B0`〜`0x0C7` (24B) | `0x0B0`〜`0x0C7` (24B) | 同一 |
| 特殊機能 (`Special[3]`) | `0x0C8`〜`0x0DF` (24B) | `0x0C8`〜`0x0DF` (24B) | 同一 |
| リマップ (`MapKey[3]`) | `0x0E0`〜`0x1F3` (276B) | `0x0E0`〜`0x1F3` (276B) | 同一 |
| **レガシー領域 A (`3 × 0x188`)** | **`0x1F4`〜`0x68B` (1176B)** | **存在しない (0B)** | **`-0x498` (-1176B)** |
| マクロヘッダ (`Macro_Pro3_Struct[3]`) | `0x68C`〜`0x913` (648B) | `0x1F4`〜`0x47B` (648B) | オフセット `-0x498` |
| レガシー領域 B (`3 × 0x08`) | `0x914`〜`0x92B` (24B) | `0x47C`〜`0x493` (24B) | オフセット `-0x498` |
| 6軸ジャイロ (`SixAxis[3]`) | `0x92C`〜`0x94F` (36B) | `0x494`〜`0x4B7` (36B) | オフセット `-0x498` |
| RGB Dynamic (`Dynamic[3]`) | `0x950`〜`0x973` (36B) | `0x4B8`〜`0x4DB` (36B) | オフセット `-0x498` |
| RGB HotWheel (`HotWheel[3]`) | `0x974`〜`0x9A3` (48B) | `0x4DC`〜`0x50B` (48B) | オフセット `-0x498` |
| RGB Single (`Single[3]`) | `0x9A4`〜`0xACF` (300B) | `0x50C`〜`0x637` (300B) | オフセット `-0x498` |
| **USB Save サブコマンド** | `cmd 0x0006, subcmd 0x0123` | `cmd 0x0006, subcmd 0x0064` | サブコマンド差異 |
| **合計サイズ** | **`0x0AD0` (2768B)** | **`0x0638` (1592B)** | `1592 + 1176 = 2768` |
