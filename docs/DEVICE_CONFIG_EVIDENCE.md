# 実機 config ダンプ証跡（2768B `1dca47fa…`）

BLE 接続した 8BitDo Ultimate 2 から取得した 2768 バイト設定ブロックの実バイト証跡です。`PROTOCOL_SPEC.md` / `STRUCT_BT2_2768B.md` の実機記述（特に `slot_flags` と `map_key.flag` の非 0/1 値）の根拠として参照します。

> **証跡ファイルの扱い**: 生の `config.bin`（および同じディレクトリの `config.json` / マクロダンプ）はローカルの `.tmp/restore-baseline/` にのみ置かれ、リポジトリにはコミットしていません（`.tmp/` は `.gitignore` 済み）。本ドキュメントは、値と SHA-256 を固定して再現可能にするための記録です。デバイスを特定できる情報は含みません。

---

## 1. 取得元と同一性

| 項目 | 値 |
| --- | --- |
| ファイル | `.tmp/restore-baseline/config.bin` |
| サイズ | **2768 バイト** (`0x0AD0`) |
| SHA-256 | `1dca47facfc1c0394a8893ac0a5b7ef80678c51081001886a4bf17ce2f59c600` |
| 取得 | `eightbitdo config dump --raw config.bin`（BLE・MTU 200）。2026-11-22 22:20 JST に保存 |
| 付随ファイル | `config.json`（同じ SHA / `slot_flags` / `cur_slot`）、`macros.json` / `macros.toml`、`slot0-macro0.bin` / `slot0-macro1.bin` |
| ヘッダ値 | `crc16` (stored, `+0x00C`) = `0x2C55` / `gamepad_mode` (`+0x010`) = `0` / `cur_slot` (`+0x012`) = `6` |

> `config.json` の `slot_flags` も `[538970385, 538970385, 4294967295]` = `[0x20200911, 0x20200911, 0xFFFFFFFF]` で一致する。

---

## 2. `slot_flags` (`+0x000..+0x00B`, `u32` × 3)

| オフセット | 生値 (`u32` LE) |
| --- | --- |
| `+0x000` | `0x20200911` (538970385) |
| `+0x004` | `0x20200911` (538970385) |
| `+0x008` | `0xFFFFFFFF` (4294967295) |

`0x20200911` は「非 0 が有効」とも解釈できるが、**正確な意味は未確定**である（bitfield の可能性を含む）。slot 2（`0xFFFFFFFF`）は未使用 / 消去済みの状態と考えられるが、断定はしない。

---

## 3. `map_key` (`MapKeyRecord`, `0x0E0 + slot * 0x5C`)

`flag` は各レコードの先頭 4B、`key[22]` は続く 88B（22 × `u32`）である。

| slot | `flag` オフセット | `flag` 生値 | `key[0]` (A) | `key[1]` (B) | `key[2]` (X) | `key[3]` (Y) |
| --- | --- | --- | --- | --- | --- | --- |
| 0 | `+0x0E0` | `0x20200911` | `0x00002000` | `0x00001000` | `0x00000010` | `0x00000020` |
| 1 | `+0x13C` | `0x20200911` | `0x00002000` | `0x00001000` | `0x00000010` | `0x00000020` |
| 2 | `+0x198` | `0xFFFFFFFF` | `0xFFFFFFFF` | `0xFFFFFFFF` | `0xFFFFFFFF` | `0xFFFFFFFF` |

slot 0 / 1 の `key[22]` 全量（index 0..21）:

```text
slot0 keys:
  0x00002000 0x00001000 0x00000010 0x00000020 0x00000400 0x00000800
  0x00004000 0x00008000 0x00000002 0x00000004 0x00000008 0x00000001
  0x00010000 0x00020000 0x00000200 0x00000100 0x00000080 0x00000040
  0x00002000 0x00001000 0x00000000 0x00000000
slot1 keys:
  0x00002000 0x00001000 0x00000010 0x00000020 0x00000400 0x00000800
  0x00004000 0x00008000 0x00000002 0x00000004 0x00000008 0x00000001
  0x00010000 0x00020000 0x00000200 0x00000100 0x00000080 0x00000040
  0x00000000 0x00000000 0x00000000 0x00000000
slot2 keys:
  0xFFFFFFFF × 22
```

- slot 0 の `key[18]` / `key[19]` は P1/P2 に A/B が割り当てられている。slot 1 は P1/P2 が未割当（`0`）。
- slot 2 は全 `key` が `0xFFFFFFFF` で、typed な `KeyMap` として解釈できない消去状態である。

### 3.1 `flag` の意味は未確定（opaque）

実機の `map_key.flag` は **`0x20200911`（slot 0/1）と `0xFFFFFFFF`（slot 2）** であり、静的解析が示す `0/1`（`1` = リマップ有効 / `0` = デフォルト）ではない。`0x20200911` が有効フラグの一種なのか、別のメタデータ（バージョン / タイムスタンプ等）を含むのかは**未確定**である。CLI の `config map show` が表示する「disabled/default」は現行実装の解釈であり、デバイスの確定仕様ではない。

---

## 4. 他のレコードの `flag` も 0/1 ではない

同じダンプの各レコード先頭 `flag` も `0/1` ではない（`STRUCT_BT2_2768B.md` のオフセット定義を使用）。

| レコード | `flag` オフセット | slot 0 | slot 1 | slot 2 |
| --- | --- | --- | --- | --- |
| `rumble` | `0x074 + slot*0x0C` | `0x20200911` | `0x20200911` | `0xFFFFFFFF` |
| `joy` | `0x098 + slot*0x08` | `0x20200911` | `0x20200911` | `0xFFFFFFFF` |
| `trigger` | `0x0B0 + slot*0x08` | `0x20200911` | `0x20200911` | `0xFFFFFFFF` |
| `special` | `0x0C8 + slot*0x08` | `0x20200911` | `0x20200911` | `0xFFFFFFFF` |
| `map_key` | `0x0E0 + slot*0x5C` | `0x20200911` | `0x20200911` | `0xFFFFFFFF` |
| `macro`（ヘッダ） | `0x68C + slot*0xD8` | `0x20200911` | `0x20200911` | `0x20200911` |
| `gyro` | `0x92C + slot*0x0C` | **`0x20190000`** | `0x20200911` | `0x20200911` |
| `dynamic` | `0x950 + slot*0x0C` | `0x20200911` | `0x20200911` | `0x20200911` |
| `hotwheel` | `0x974 + slot*0x10` | `0x20200911` | `0x20200911` | `0x20200911` |
| `single` | `0x9A4 + slot*0x64` | `0x20200911` | `0x20200911` | `0x20200911` |

`gyro` slot 0 だけ `0x20190000` (538509312) と異なる点も、`flag` が単純な有効 / 無効の 2 値ではないことを示す。

---

## 5. `config remap A B` の実機差分

同一接続・MTU 200 で `config remap A B` を実行した際の記録（`.tmp/sess.out`、2026-11-23）。

- コマンド: `eightbitdo config remap A B`（pre-write backup: `config-backup-1795363916072520571-2661740-0/`）
- before SHA-256: `1dca47facfc1c0394a8893ac0a5b7ef80678c51081001886a4bf17ce2f59c600`
- after SHA-256: `4c72a94220a52d849e3ffa807def7231634f17d296564c75a08bcec4c7c09be4`
- 変更領域:
  - `0x000C..0x000E` (2 bytes): config CRC (`ANSI_CRC_16_sh`)
  - `0x00E5..0x00E6` (1 byte): `map_key[0].key[0]` の `0x2000` → `0x1000`（A → B）
- `map_key[0].flag` (`+0x0E0`) は **`0x20200911` のまま不変**
- read-back SHA-256: `4c72a94220a52d849e3ffa807def7231634f17d296564c75a08bcec4c7c09be4`（送信内容と一致）
- CRC: `sent=0x5138 stored=0x5138 in_place=0xB56C`（device は送信された CRC を保持）
- 送信: **38 write + 1 Save = 合計 39 packets**、74B chunk

`0x00002000 → 0x00001000` の変更で実際に変わるのは LE の 2 バイト目だけなので、差分領域は `0x00E5` の 1 バイトになる。`flag` と `key[1]` 以降は変更されない。

---

## 6. raw 検証との不整合（`--force-raw` が必要）

この実機ダンプを `config write --file` でそのまま書き戻そうとすると、現行の raw 検証（各レコードの `flag` は `0/1` のみ許可）が実機値と衝突して拒否される。`.tmp/sess.err` の実際のエラー:

```text
error: I/O error: configuration file has invalid known fields; refusing to write (use --force-raw only to restore an undocumented device backup):
  - joy[0].flag 538970385 is outside 0..=1
  - trigger[0].flag 538970385 is outside 0..=1
  - gyro[0].flag 538509312 is outside 0..=1
  - map_key[0].flag 538970385 is outside 0..=1
  ... (全レコードの flag が同様に列挙される)
  - map_key[2].flag 4294967295 is outside 0..=1
  - button mapping slot 2 index 0 (A): 0xFFFFFFFF is not a documented single KeyMap value
  ... (slot 2 の 22 key すべてが列挙される)
```

- 現状の回避策は **`config write --file <dump> --force-raw`**。`--force-raw` は whitelist / range / enum の検証を警告に降格するが、予約領域ガードは維持する。
- 恒久対応（`flag` の扱いと検証規則の見直し）は**別 PR で修正予定**。本ドキュメントの値が確定であり、docs 上の「実機値は 0x20200911 / 0xFFFFFFFF、意味未確定」という記述を優先する。

---

## 7. 再現コマンド

生ダンプは `.tmp/restore-baseline/config.bin` に置いた状態で実行する。以下のパスは**ローカル作業ツリーのリポジトリルート基準**（`.tmp/` は gitignore されたローカル作業ディレクトリ）。別の worktree から確認する場合は、親リポジトリの絶対パスに読み替える。

```bash
# 同一性の確認
sha256sum .tmp/restore-baseline/config.bin
# => 1dca47facfc1c0394a8893ac0a5b7ef80678c51081001886a4bf17ce2f59c600

# slot_flags と map_key を実バイトから読む
python3 - <<'PY'
import struct

b = open(".tmp/restore-baseline/config.bin", "rb").read()
assert len(b) == 2768, len(b)

def u32(off):
    return struct.unpack_from("<I", b, off)[0]

print("slot_flags:",
      [hex(u32(o)) for o in (0x000, 0x004, 0x008)])

for slot in range(3):
    base = 0x0E0 + slot * 0x5C
    flag = u32(base)
    keys = [u32(base + 4 + i * 4) for i in range(22)]
    print(f"slot{slot}: flag@{base:#05x}={flag:#010x}")
    print("  keys:", " ".join(f"{k:#010x}" for k in keys))

print("crc16 stored:", hex(struct.unpack_from('<H', b, 0x00C)[0]))
PY
```

期待される出力（抜粋）:

```text
slot_flags: ['0x20200911', '0x20200911', '0xffffffff']
slot0: flag@0x00e0=0x20200911
  keys: 0x00002000 0x00001000 0x00000010 0x00000020 ...
slot1: flag@0x013c=0x20200911
  keys: 0x00002000 0x00001000 0x00000010 0x00000020 ...
slot2: flag@0x0198=0xffffffff
  keys: 0xffffffff 0xffffffff ... (22 個)
crc16 stored: 0x2c55
```

`config remap A B` の再現（実機書込を伴うため、必ず pre-write backup と read-back を確認する）:

```bash
eightbitdo config dump --raw before.bin
eightbitdo config remap A B          # pre-write backup と --yes の確認を含む
eightbitdo config map show           # key[0] が B (0x1000)、flag は 0x20200911 のまま
eightbitdo config dump --raw after.bin
sha256sum before.bin after.bin
```

---

## 8. 注意

- `flag` の意味（有効 / 無効、bitfield の各ビット）は未確定である。本ドキュメントの値を「有効フラグ」と断定しない。
- slot 2 の `0xFFFFFFFF` は消去状態と考えられるが、typed な `KeyMap` として解釈・書込してはならない。
- 本ドキュメントの値は 1 個体・1 時点のスナップショットである。ファームウェア更新や別個体で同じ値になる保証はない。
- 生ダンプ（`.tmp/restore-baseline/`）はリポジトリにコミットしない。

---

## 9. 関連ドキュメント

- [PROTOCOL_SPEC.md](PROTOCOL_SPEC.md) §6.2（mapKey 書込・raw 検証・実機 flag の注記）
- [STRUCT_BT2_2768B.md](STRUCT_BT2_2768B.md) §1.2 / §5.1（レコード配置・mapKey 実機確認）
- [IMPLEMENTATION_PLAN.md](IMPLEMENTATION_PLAN.md) §5 Phase 2d（実機確認範囲と既知の不整合）
