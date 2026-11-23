# 実機 config ダンプ・プロファイル切替の証跡（2768B `1dca47fa…`）

BLE 接続した 8BitDo Ultimate 2（PID `0x600F`）から取得した 2768B 設定ブロックと `0x0015`（`WriteSLOT`）の実バイト証跡です。生ダンプ（`.tmp/restore-baseline/`）は gitignore 済みでリポジトリには含めません。

## 1. 基準ダンプの同一性（2026-11-22 22:20 JST、Linux BLE・MTU 200）

- ファイル: `.tmp/restore-baseline/config.bin`（**2768 バイト / `0x0AD0`**）
- SHA-256: `1dca47facfc1c0394a8893ac0a5b7ef80678c51081001886a4bf17ce2f59c600`
- ヘッダ値: `crc16`（stored, `+0x00C`）= `0x2C55` / `gamepad_mode`（`+0x010`）= `0` / `cur_slot`（`+0x012`）= `6`
- `slot_flags`（`+0x000..+0x00B`）: `[0x20200911, 0x20200911, 0xFFFFFFFF]`

## 2. 各レコードの `flag` と `map_key` 実測値

静的解析では `0/1` とされた各レコード先頭 `flag` は、実機では `0x20200911`・`0x20190000`・`0xFFFFFFFF` を取るため、**正確な意味は未確定（opaque）**です。

| レコード | `flag` オフセット | slot 0 | slot 1 | slot 2 |
| --- | --- | --- | --- | --- |
| `rumble` / `joy` / `trigger` / `special` / `map_key` | `0x074` / `0x098` / `0x0B0` / `0x0C8` / `0x0E0` | `0x20200911` | `0x20200911` | `0xFFFFFFFF` |
| `macro` ヘッダ / `dynamic` / `hotwheel` / `single` | `0x68C` / `0x950` / `0x974` / `0x9A4` | `0x20200911` | `0x20200911` | `0x20200911` |
| `gyro` | `0x92C + slot*0x0C` | **`0x20190000`** | `0x20200911` | `0x20200911` |

`map_key`（`0x0E0 + slot * 0x5C`）の `key[22]` 実測:
- slot 0 / 1 の index 0..17 は共通: `0x00002000 0x00001000 0x00000010 0x00000020 0x00000400 0x00000800 0x00004000 0x00008000 0x00000002 0x00000004 0x00000008 0x00000001 0x00010000 0x00020000 0x00000200 0x00000100 0x00000080 0x00000040`
- slot 0 の index 18..21（P1–P4）: `0x00002000 0x00001000 0x00000000 0x00000000` / slot 1: `0x00000000 × 4`
- slot 2: `0xFFFFFFFF × 22`（消去状態と考えられ、typed な `KeyMap` として解釈・書込してはならない）

## 3. `config remap A B` の実機差分（2026-11-23、MTU 200）

- before SHA-256: `1dca47facfc1c0394a8893ac0a5b7ef80678c51081001886a4bf17ce2f59c600`
- after / read-back SHA-256: `4c72a94220a52d849e3ffa807def7231634f17d296564c75a08bcec4c7c09be4`（一致）
- 変更領域: `0x000C..0x000E`（CRC: `sent=0x5138 stored=0x5138 in_place=0xB56C`）と `0x00E5..0x00E6`（`key[0]` `0x2000`→`0x1000`）。`map_key[0].flag` は `0x20200911` のまま不変。
- 送信: **38 write + 1 Save = 合計 39 packets**（74B chunk）。

## 4. raw 検証との不整合（PR #17 で修正済み）

修正前は全レコードの `flag` を `0/1` に限定していたため本ダンプの書き戻しが拒否され `--force-raw` が必要だった。修正後は実機観測値集合（`0` / `1` / `0x20200911` / `0xFFFFFFFF`、gyro は `0x20190000` も）と完全消去 slot（joy 全 255 + rumble NaN + map_key 全 22 `0xFFFFFFFF` + 各 flag `0xFFFFFFFF` が揃う場合のみ）を受理し、`--force-raw` なしで書き戻せる。partial state や範囲外値は引き続き拒否する。

## 5. `0x0015`（`WriteSLOT`）の実機証跡（旧 §7、2026-11-23 JST）

Linux BlueZ / btleplug・BLE App Mode `"80ND"`・MTU 200 で `eightbitdo config profile 1` を実行した **1 回の観測記録**（per-frame timeout 5 秒、体感 1 秒未満で応答）。

- **要求フレーム（17B）**: `04 15 00 01 00 00 00 00 00 00 00 00 00 00 00 00 00`（prefix `0x04`、`cmd=0x0015`、`subcmd=0x0001`、payload なし。RE 仕様と一致）
- **ACK フレーム（16B、prefix なし）**: `04 00 15 00 00 00 00 81 00 00 00 00 00 00 00 00`
  - `cmd=0x0004`（RESPONSE 封筒）/ `subcmd=0x0015` のみ観測（直接 `cmd=0x0015` ACK は未観測）。
  - `+0x06..+0x07` の `00 81`（LE `0x8100`）は payload なしのため CRC としての意味は未確定。
- **`cur_slot` の変化（Save なし）**: 直後の `config dump` で `+0x012` が **6 → 1** に更新（read-back SHA `7a9838e8…` → `e3a84da2…`）。ただし **Save 不要とは断定しない**。
- **復元**: `+0x012` を `6` に戻して `config write --yes`（39 packets）で書き戻し、`cur_slot=6` を確認（read-back SHA `af897618…`。切替前 SHA と不一致の理由は未確認）。
- **未確認事項**: 再接続・電源断を跨ぐ永続性、Switch 側でのプロファイル実効、`00 81` の意味、直接 ACK の条件、ACK 形状の再現性、`cur_slot=6` の意味。

## 6. 注意と関連

本記録は 1 個体・1 時点のスナップショットであり、別個体・別 FW で同一値の保証はない。関連: [PROTOCOL_SPEC.md](PROTOCOL_SPEC.md) §6.2 / [STRUCT_BT2_2768B.md](STRUCT_BT2_2768B.md) / [FEATURE_MATRIX.md](FEATURE_MATRIX.md) §8。
