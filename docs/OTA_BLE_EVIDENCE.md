# BLE OTA 解析ノート（`"80ND"` / `"8bt"`）— 証跡

本ファイルは `docs/PROTOCOL_SPEC.md` §8 と `crates/eightbitdo-core/src/ota.rs` が参照する
OTA 関連の静的解析ノートである。公式バイナリ自体（`.apk` / `.so` / `.dex` / `.dll`）は
リポジトリに含めない。記載するのは関数アドレス・文字列・コード上の観測内容のみ。

- 対象: 8BitDo Ultimate Software V2 Android v2.01 の `classes.dex` / `classes2.dex` と
  `lib/x86_64/libadvancelib.so`（x86 / arm64 は裏取りのみ）
- 手法: `readelf` / `objdump -d -M intel` / `llvm-objdump`、androguard 4.1.4 による xref 走査
- 日付: 2026-11-22（実機・BLE には触れていない）
- 判定ラベル: **確定**=逆アセンブルで直接確認 / **推測**=コードから妥当に導けるが実機未確認 /
  **未確認**=この解析では決められない

> 本ノートは「Android アプリ v2.01 のコードが何をしているか」の証跡であり、実機の
> ファームウェア更新が成立することを保証しない。`PROTOCOL_SPEC` §8.5 の未検証事項は
> 実機 btsnoop が取れるまで未検証のまま扱う。

## 1. 確定したシーケンス（コード上の順序）

`Java_..._HIDBoot_startUpdate` **0x3d530** は `.dat` 先頭 28B をヘッダへコピーし、
`resetdata` **0x3b620** で状態変数を 0 にした後、`readSerial` **0x3bf00** を呼ぶ。

```
readSerial 0x00C3 (CRC 検証読み, 3000 ms)
  └ 一致セクタは Erase/Write をスキップ
  └ 不一致セクタのみ:
       EraseFlash 0x0004 (10000 ms) → EraseFlash_onfinished 0x3c920
         └ WriteFirmware 0x0003 (1000 ms) × セクタ内 64 回
              └ WriteFirmware_onfinished 0x3cad0 で writeoffset += 0x40
                 0x1000 を超えたら次のセクタへ readSerial
  └ 最終セクタ後 WriteVersion 0x00C4 (3000 ms) → writeversion_onfinished 0x3cdc0
       └ HIDBoot.writeFirmwareonfinished → getwritereset 0x2d030 → 0x0007 reset (3000 ms)
```

- 送信される cmd は **`0x00C3` / `0x0004` / `0x0003` / `0x00C4` / `0x0007` の 5 種のみ**。
- `0x0002`（WriteFirmwareVersion / WriteAPPID）と `0x0006`（JumpApp）は送られない（§4）。
- セクタ 4 KB、チャンク 64B、セクタあたり 64 チャンク（`WriteFirmware_onfinished` の
  `writeoffset += 0x40` と `> 0xFFF` 判定、0x3cbda–0x3cbe3）。
- セクタ数は `ceil(image_len / 0x1000)`（終了条件 `offset + index*0x1000 + 0x1000 >= header[8]`、
  0x3bbdd–0x3bbf4）。`image_len` が 4KB 倍数でない場合、最終セクタは部分書き込みになる。

## 2. `WriteVersion 0x00C4` の payload（確定）

`getWriteVersion` **0x2cfb0** は `chunk_len = 0x18` を宣言するが、`Generate_CustomDataCmd`
**0x27620** の分岐（0x27a84–0x27b9f）で payload が NULL のため `chunk_len = 0` に上書きされ、
CRC は空入力の `0xFFFF`、送信長は **16B（ヘッダのみ）** になる。

- wire は `[0x05][cmd=0x00C4][subcmd=0][chunk_len=0x0018][crc=0xFFFF][total_len=image_len][offset=0]`
  の 16B で、24B のヘッダデータは載らない（0x600F / BLE boot の汎用経路）。
- PID が `0x3019` / `0x9023` のときだけ payload 4B を載せる別分岐（0x27bb2）があるが、
  `0x600F` と BLE boot はこの分岐に入らない。
- 本リポジトリの `PROTOCOL_SPEC` §8.3 は 24B 送信と記載しており、**Android 実装とは不一致**（§11-3）。

## 3. Erase の実値（確定）

`getEraseFlashChannel(uint32_t sector_off)` **0x2d130**:

| フィールド | 値 | 根拠 |
| --- | --- | --- |
| `cmd` / `subcmd` | `0x0004` / `0x0000` | 0x2d1bd |
| wire `offset` | **セクタ相対 `sector_off`（0, 0x1000, …）**。`load_base` は加算されない（0x278e2 / 0x27949） | 0x2d1a8 付近 |
| `total_len` | `0x1000` | 0x2d1a8 |
| 宣言 `chunk_len` | `0x1000`（payload なし） | 0x278de |
| wire `crc16` | `0xFFFF`（空 payload の CRC。type 1 のみ書く） | 0x27b66 / 0x28630 |
| 実 payload / フレーム長 | 0B / 16B | 0x27b9f 分岐 |

タイムアウトは `EraseFlash` **0x3bfd0** の JNI で 10000 ms に上書き（0x3c01e）。
`PROTOCOL_SPEC` §8.3 は `offset = load_base_addr + sector_off` としており、**Android 実装とは
不一致**（§11-4）。

## 4. `0x0002` / `0x0006` 等のデッドコード判定（確定）

- `WriteAPPID` の唯一の呼び出し元は `WriteFirmwareVersion_onfinished` **0x3d0e0** だが、
  その `WriteFirmwareVersion` **0x3d200** 自身に `.so` 内 call が 0 件。
- `jumpApp` **0x3d340** / `readPID` **0x3d490** / `readVersionInBoot` **0x3b580** /
  `enterBoot` **0x357d0** も `.so` 内 call 0 件、`readelf -rW` の関数ポインタも無し。
- dex 側も `jumpApp` / `readPID` / `readVersionInBoot` の Java 呼び出し元は 0 件
  （`readVersionInBootInterfaceList` / `readPIDInBootInterfaceList` のリスナー登録のみ）。
- したがって Android APK は **App → Boot 遷移・PID 読み・JumpApp を実行しない**。
  実運用でどう Bootloader に入るかは端末操作 / 別ツールの可能性が高く、APK からは決められない。
- 本リポジトリの BLE OTA は `PROTOCOL_SPEC` §8 の完全シーケンスを実装するため、これらの
  コマンドを含む。実機での応答形状は未確認（§11）。

## 5. タイムアウト（確定）と仕様 5000ms との差

ネイティブ実値は `Channel+0x64` の float 即値。`Channel::start` **0x35f20** は 0 のとき
タイマを登録しない（=送信のみ）。

| cmd | 関数 | 実タイムアウト | `PROTOCOL_SPEC` §8.3 |
| --- | --- | --- | --- |
| `0x0003` WriteFirmware | `WriteFirmware` 0x3c9f0 / 0x3ca5a | **1000 ms** | 1000 ms（一致） |
| `0x0004` EraseFlash | `EraseFlash` 0x3bfd0 / 0x3c01e | **10000 ms** | 10000 ms（一致） |
| `0x00C3` 検証読み | `readSerial` 0x3bf00 / 0x3bf80 | **3000 ms** | 記載なし |
| `0x00C4` WriteVersion | `writeversion` 0x3c070 / 0x3c088 | **3000 ms** | 3000 ms（一致） |
| `0x0007` reset | `getreset` 0x3ce90 / 0x3cea8 | **3000 ms** | 記載なし |
| `0x0005` readVersionInBoot | 0x3b580 / 0x3b5ac | **2000 ms** | **5000 ms（差）** |
| `0x0008` readPID | 0x3d490 / 0x3d4bc | **2000 ms** | **5000 ms（差）** |
| `0x0002` WriteAPPID | 0x3d120 / 0x3d168 | **2000 ms** | **5000 ms（差）** |
| `0x0002` WriteFirmwareVersion | 0x3d200 / 0x3d22c | **0（タイマなし）** | 「即時（0ms設定）」（一致） |
| `0x0006` jumpApp | 0x3d340 / 0x3d36c | **0（タイマなし）** | 5000 ms（「送信設定 0」と注記） |

`readPID` / `readVersionInBoot` / WriteAPPID は Android では dead code のため、この差は
Android 実機の BLE OTA では観測されない。

## 6. `gamepad_type` の決定（確定）

`SHBLEUtils.setServiceUUIDAndCharacteristicUUID` はキャラクタリスティックの**リスト内 index**で
`gamepad_type` を決める（`getGamepadBootType` / `getGamepadAppType` は一致なしで 0）。

- App: `FF11`(index 0) → type 0、`FF13`(index 1) → type 1。
- Boot (`"8bt"`): `FF21`(index 0) → type 0、`FF23`(index 1) → type 1。
- `HIDChannel.setGamepadType` **0x29a50** は BSS **0x62a24** の `gamepad_type` に値を格納するだけ。
- type 1 のときだけ選ばれる値: `readVersionInBoot` **0x01017040**、`WriteFirmwareVersion`
  **0x01017040**、`jumpApp` **0x01018000**（`getVersionInBootChannel` 0x2cbe0 /
  `getJumpAppChannel` 0x2d8c0 の `cmove`）。type 0 は `0x08014034` / `0x08014000`。
- **`readPID` の PID 値と `gamepad_type` の対応は APK からは決められない**。本リポジトリは
  `0x600F` → type 1 のホワイトリストをプロジェクト規約として実装している（§11-3）。

## 7. Write の実値と 16B 問題（一部確定 / 一部未確認）

`getWriteFimwarefileChannel(fw, sect_off, writeoff)` **0x2d350**:

| フィールド | 値 | 根拠 |
| --- | --- | --- |
| `cmd` / `subcmd` | `0x0003` / `0x0000` | 0x2d3f0 |
| wire `offset` | `sect_off + writeoff`（イメージ相対。`load_base` を加算しない） | 0x2d3a1–0x2d3e0 |
| `total_len` | `header[8]`（image_len） | 0x2d3b9 |
| 宣言 `chunk_len` | `0x40` | 0x2d3e8 |
| payload コピー元 | `firmware_src + (sect_off + writeoff)`（**`+0x1C` を足さない**） | 0x27a37–0x27ac3 |
| `crc16` | `crc16(payload_64B, 0x40)`（type 1 のみ） | 0x27b66 / 0x27b72 |

### 7.1 16B 問題（コード事実は確定 / 実機 wire は未確認）

`Generate_CustomDataCmd` の送信長決定（x86_64 **0x27b91–0x27bad**）:

```
27b91: xor r14d,0x3            ; cmd
27b95: mov r13w,0x10
27b9a: or  r14w,[rbp-0x34]     ; subcmd
27b9f: je  27d57               ; (cmd==3 && subcmd==0) → 送信長 16B（ヘッダのみ）
27ba5: mov r13d,[rbx+0xc]      ; それ以外 → chunk_len + 16
```

同じ分岐が x86 32bit 0x22902–0x22919、arm64 0x29a9c–0x29ab4 にも存在（3 ABI 一致）。
つまり Android 実装は `cmd=0x0003, subcmd=0` のとき **64B payload を送信長に含めない**。
App Mode の `ReadKeysAndJoy`（`0x0003`/subcmd 0）を正しく送るための特例と推測されるが、
BLE Boot の `WriteFirmware` も同じ cmd/subcmd のため巻き込まれる。
実機で OTA が成立するか、端末がヘッダのみの `0x0003` をどう解釈するかは **未確認**（btsnoop 待ち）。

## 8. `.dat` ヘッダ（確定: 解釈 / 検証なし）

`startUpdate` 0x3d585–0x3d597 は先頭 **28B (0x1C)** をグローバルヘッダへコピーする。

| offset | 意味 | 使用箇所 |
| --- | --- | --- |
| `+0x00 u32` | `firmwareFile_Version` | `WriteFirmwareVersion`（dead） |
| `+0x04 u32` | `load_base_addr` | `+0x3c` に格納。汎用経路の wire `offset` には加算されない |
| `+0x08 u32` | `image_len` | `total_len`、ループ終了条件、マルチセクション位置計算 |
| `+0x0c..0x1b` | 付加メタデータ | Android 実装では読み出しに使わない |

- サイズ・マジック・署名・ハッシュ・CRC の検証は行わない（28B コピー後の分岐は
  PID==0x0553 のみ）。
- マルチセクション: `firmware_src_length > image_len + 0x1C` のとき
  `firmware_src + image_len + 0x1C` から 2 個目の 28B ヘッダを読む（0x3be6e / 0x3ccdb）。
- ダウンロード時の MD5 比較は `Const.getMD5(String.valueOf(contentLength))` であり、
  ファイル内容の MD5 ではない（`HttpsUtils.getDownloadFirwamre`）。

## 9. 失敗時の復旧（確定: アプリ側に実装なし）

- `EraseFlash_overtime` 0x3cac0 / `WriteFirmware_overtime` 0x3cdb0 / `readSerial_overtime`
  0x3c0f0 / `writeversion_overtime` 0x3cf10 / `getreset_overtime` 0x3cf60 はすべて
  `mov al,1; ret` で、再送・状態巻き戻し・切断・通知を行わない。
- Java のエラー通知 IF は無く、切断時の `HIDBoot.isUpdateing` 再スキャン分岐は
  `isUpdateing` が true にされないため dead。
- 再実行によるレジュームは「`0x00C3` で一致セクタをスキップする」ため成立する可能性がある
  （推測）。実機未確認。

## 10. 本実装との対応

| 本実装の判断 | 本ノートの根拠 | 状態 |
| --- | --- | --- |
| Erase/Write のアドレスを `load_base + sector_off` とする | §3 / §7 はイメージ相対と報告 | **spec と evidence が矛盾。実機未確認** |
| `WriteVersion` は 24B ヘッダを送る | §2 は 16B（payload 0）と報告 | **不一致。実機未確認** |
| `WriteFirmware` は 64B payload を送る | §7.1 は 16B 送信のコード事実を報告 | **不一致。実機未確認** |
| `EraseFlash` は `chunk_len = 0` で送る（既存 builder） | §3 は宣言 `0x1000` と報告 | **不一致。実機未確認** |
| `readPID` / `readVersionInBoot` は 5s | §5 は Android 2s と報告 | **仕様値。実機未確認** |
| `0x600F` のみ type 1 として続行 | §6 はキャラクタリスティック index 由来と報告 | **プロジェクト規約。実機未確認** |
| EnterBoot 後の `"8bt"` 再接続をフロントエンド責務とする | §4 は enterBoot 自体が dead と報告 | 実機未確認 |

## 11. 未確認事項（優先順）

1. `cmd=0x0003, subcmd=0` の実フレーム長（§7.1）。btsnoop 採取が最優先。
2. `0x00C3` 応答のレイアウト（端末側 4KB CRC が payload の `[2..3]` か）。
3. `0x00C4` の実 wire（16B か 24B か）。Windows 実機との差。
4. `"8bt"` 接続時の `gamepad_type` と `readPID` の対応（§6）。
5. `WriteFirmware` のコピー元 `+0x1C` 欠落（§7）と、実 wire で先頭 64B に 28B ヘッダが混ざるか。
6. Bootloader の MTU と、81B フレーム（`WriteFirmware`）が通るか。
7. マルチセクション `.dat` の実在と 2 セクション目の扱い。
8. Erase/Write の ready ACK と完了 ACK の区別（payload-less ACK が先行するか）。
9. タイムアウト時の端末状態と、`0x0007` reset 後の再列挙時間。

## 12. 再現手順（読み取りのみ）

```bash
# 1) APK から .so / dex を抽出（公式バイナリはリポジトリに置かない）
unzip -o 8BitDo_Ultimate_Software_V2_Android_V2.01.apk \
  'lib/x86_64/libadvancelib.so' 'classes*.dex' -d /tmp/ota-re

# 2) OTA JNI の逆アセンブル
objdump -d -M intel --no-show-raw-insn --start-address=0x3b620 --stop-address=0x3d6b0 \
  /tmp/ota-re/lib/x86_64/libadvancelib.so
# チャネル生成 / Generate_CustomDataCmd / Channel::start
objdump -d -M intel --no-show-raw-insn --start-address=0x2cbe0 --stop-address=0x2dd00 \
  /tmp/ota-re/lib/x86_64/libadvancelib.so
objdump -d -M intel --no-show-raw-insn --start-address=0x27620 --stop-address=0x27e30 \
  /tmp/ota-re/lib/x86_64/libadvancelib.so
objdump -d -M intel --no-show-raw-insn --start-address=0x35f20 --stop-address=0x36100 \
  /tmp/ota-re/lib/x86_64/libadvancelib.so

# 3) dex xref（androguard 4.1.4）
#    Analysis.create_xref() → get_xref_from() で jumpApp/readPID/enterBoot/... を全走査
```
