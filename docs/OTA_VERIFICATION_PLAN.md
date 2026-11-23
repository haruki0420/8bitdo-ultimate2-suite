# BLE OTA 実機検証準備（btsnoop 採取・判定手順）

- 対象: 8BitDo Ultimate 2 Bluetooth（`0x600F`）の BLE OTA（App Mode `"80ND"` `FF10` → Bootloader `"8bt"` `FF20` / `FF21`・`FF23`）。USB OTA は対象外（TX フレーミング未文書化）。
- 目的: [PROTOCOL_SPEC.md](PROTOCOL_SPEC.md) §8.5 の未検証事項 1〜13（wire で決着できる項目）を実機 btsnoop で確定し、[OTA_BLE_EVIDENCE.md](OTA_BLE_EVIDENCE.md) と [PROTOCOL_SPEC.md](PROTOCOL_SPEC.md) §8.5 を更新する。
- 関連: [SESSION_OPERATIONS.md](SESSION_OPERATIONS.md) / [crates/eightbitdo-core/src/ota.rs](../crates/eightbitdo-core/src/ota.rs) / [scripts/ota/capture-btsnoop.sh](../scripts/ota/capture-btsnoop.sh)。

> 実機採取・書込は、本書の安全条件を満たしユーザーの承認を得てから実施する。

## 1. 現状と駆動手段

- **実装状態（`ota.rs`）**: BLE OTA 状態機械は実装済みだが **CLI / GUI / UniFFI 未配線**。`OtaConfig::allow_unverified_wire` と `require_completion_ack` は既定 `false`、`OTA_MIN_MTU = 84`（`mtu - 3 >= 81`）、リトライなし（失敗時は先頭から全消去・再書込）。`readPID`（`0x0008`）と `readVersionInBoot`（`0x0005`）はフラッシュを送らない。
- **段階実行 API（`eightbitdo-core/ota-bench`、既定 OFF）**: `OtaUpdate::write_firmware_with_checkpoint`、`OtaCheckpoint::{AfterFirstErase, AfterWrites, AfterWriteVersion}`、`OtaBenchOutcome`、`OtaPhase::BenchStopped`。停止後は同じ `OtaUpdate` で再開できず、新規 `OtaUpdate` で全消去・再書込する（reviewed SHA の rustdoc を正とする）。
- **駆動手段の優先順**:
  1. **(d) 公式 Android アプリ + HCI btsnoop（最優先）**: 自前コードを書込経路に載せずに一次証跡を得る。現行 FW < 最新 FW の状態と専用個体を用意し、採取後は設定を戻す。**(d) で全 wire が取れた場合は Phase B を実施しない**。不可の場合のみ「どの段階で止まったか」を EVIDENCE に記録して Phase B へ進む。
  2. **(a) 配線済みフロントエンド（未実装）** または **(b) コミットしない一時ベンチハーネス**: Phase B では `with_unverified_wire_allowed(true)`、非キャンセル、no retry、`OtaWriteState` / `OtaBenchOutcome` ログ、`tmux` / `setsid` による端末切り離し、`AfterFirstErase` 停止を必須とする。
  - 公式 Windows ソフトは挙動未検証のため証跡に採用しない。

## 2. Phase A と Phase B の役割

| Phase | 実行範囲 | フラッシュ | `allow_unverified_wire` | 確定できる範囲 |
| --- | --- | --- | --- | --- |
| **A（読取のみ）** | `enterBoot` → `"8bt"` 再接続 → `readPID` → `readVersionInBoot` | 送らない | `false` のまま | RX フレーミング、read 応答、`"8bt"` MTU、使用 characteristic（`FF21`/`FF23`）、`enterBoot` 成立、read タイムアウト |
| **B（初回書込）** | `EraseFlash` / `WriteFirmware` / `WriteVersion` / `WriteFirmwareVersion` / `JumpApp` | 送る | `true` 必須（一時ハーネスのみ） | Erase / Write ACK、アドレス規約、Erase `chunk_len`、Write / WriteVersion フレーム長、Jump 復帰 |

- Phase A は `JumpApp` を送らないため App Mode に自動復帰しない（`"8bt"` 切断 → 電源再投入 → `scan --check` で復帰）。
- **「wire 確定」**は全コマンド（Erase / Write / WriteVersion / WriteFirmwareVersion / Jump）の wire が完走記録された状態のみを指す。部分記録は確定ではない。

## 3. 採取環境とタイミング

### 3.1 環境

- **Linux BlueZ（主経路）**: `sudo btmon`（root または `CAP_NET_ADMIN`）。複数アダプタ環境では `-i hciX` を必須指定し、アダプタ名を記録する。AC 給電・スリープ無効・NTP 時刻同期、`eightbitdo session` や他の BLE ツール・他 `btmon` を停止する。
- **Android（(d) 用）**: 開発者向けオプションの「Bluetooth HCI スヌープログ」を有効化 → Bluetooth OFF/ON → 採取後 `adb root && adb pull /data/misc/bluetooth/logs/btsnoop_hci.log`（非 root は `adb bugreport` の zip 内から抽出、旧端末は `/sdcard/btsnoop_hci.log`）。

### 3.2 採取コマンドとタイムライン

**開始は必ず最初の BLE 操作より前**（サービス探索の handle → UUID と MTU 交換を含めるため）。

```sh
scripts/ota/capture-btsnoop.sh -o .tmp/ota-capture -p a   # Phase A: readVersionInBoot 後 1〜2 秒で停止
scripts/ota/capture-btsnoop.sh -o .tmp/ota-capture -p b   # Phase B: セッションごとに 1 回ずつ計 2 回実行
```

- スクリプトは `umask 077` で `.tmp/`（gitignore 済み）に保存し、停止は `SIGINT` → `SIGTERM` → `SIGKILL`。終了コード: **0 = 正常 / 2 = 使用エラー・他 btmon 稼働 / 3 = INVALID（空・magic 不正・SIGKILL 停止）**。exit 3 は証跡にしない。`od` 不在時は手動で先頭 8B が `62 74 73 6e 6f 6f 70 00` であることを確認する。
- **Phase A 停止点**: `readVersionInBoot` 応答後 1〜2 秒。
- **Phase B 停止点**: セッション 1 は最初の Erase ACK 直後、セッション 2 は Jump 後の link close → `"80ND"` 再広告・再接続 → `ReadVersion` 確認後 5〜10 秒。

## 4. 確認項目と期待フレーム

### 4.1 確認項目（16 項目）

| # | 項目 | Phase | btsnoop の確認ポイント | 仕様 / 実装の期待と EVIDENCE との差 |
| --- | --- | --- | --- | --- |
| 1 | Bootloader RX フレーミング | A | `"8bt"` Notification 先頭バイト | 16B ヘッダ + payload、prefix なし（`0x05` は TX のみ） |
| 2 | `readPID` 応答 | A | `0x0008` 後の Notification | `cmd=0x0004/subcmd=0x0008` envelope または直エコー、payload 2B |
| 3 | `readVersionInBoot` 応答 | A | `0x0005`（`offset=0x01017040`）後 | 4B payload（LE `u32`）、ready ACK 先行有無 |
| 4 | Erase ready / 完了 ACK | B | `0x0004` ごとの Notification 数 | 1 個か 2 個か（実装は既定 1 個扱い） |
| 5 | Write ready / 完了 ACK | B | `0x0003` ごとの Notification 数・順序 | 1 個か 2 個か、次チャンク前完了か |
| 6 | ACK の `offset`/`total_len` | B | ACK `+0x0C` / `+0x08` | 要求と同値か 0 か（遅延 ACK 誤帰属の判断材料） |
| 7 | Erase アドレス規約 | B | `0x0004` の `offset` | 実装 `load_base + sector_off` vs EVIDENCE 相対（**矛盾**） |
| 8 | Write アドレス規約 | B | `0x0003` の `offset` | 同上（**矛盾**） |
| 9 | Erase `chunk_len` | B | `0x0004` の `+0x04` と長 | 実装 `0`/17B vs EVIDENCE `0x1000`（**不一致**） |
| 10 | WriteFirmware 長と元データ | B | `0x0003` 長・先頭 64B | 実装 81B・`.dat[0x1C..]` vs EVIDENCE 16B・`+0x1C` なし（**矛盾**） |
| 11 | `WriteVersion` payload | B | `0x00C4` 長・`chunk_len`・crc | 実装 41B（24B, `crc16=0`）vs EVIDENCE 16B（`0`, `0xFFFF`）（**不一致**） |
| 12 | `enterBoot` `0x000C` | A | App TX（prefix `0x04`）後 | ACK か link close のみか |
| 13 | Bootloader MTU | A | Exchange MTU / 最大 ATT write | 81B フレームが 1 回で通るか |
| 14 | characteristic / `gamepad_type` | A | 使用 handle（`FF21` vs `FF23`） | `FF21`=type 0 / `FF23`=type 1 と PID `0x600F` の整合 |
| 15 | タイムアウト実測 | A / B | 要求 → ACK 経過時間 | Erase 10s / Write 1s / WriteVersion 3s / read 5s（Android 2s） |
| 16 | Jump / 復帰 | B | `0x0006`（`0x01018000`）後 | ACK / link close → `"80ND"` 復帰時間と version |

- **マルチセクション `.dat`（blocker）**: btsnoop 不要。`file_size > 28 + header[8]` ならマルチセクションであり、現実装は `OtaInvalidFirmware` で拒否する。
- 判定区分: **仕様一致** / **証跡一致**（実装変更は別 PR・Tier 3）/ **どちらとも違う**（未確定のまま実装停止）/ **応答なし・切断**（初回 Erase なら即中止）。

### 4.2 実装が送る期待フレーム（TX = 1B prefix + 16B LE ヘッダ + payload、CRC-16/MODBUS `0xA001`）

| コマンド | フレーム（16 進） | 長さ |
| --- | --- | --- |
| `enterBoot`（App） | `04 0C 00 67 00 00 00 00 04 00 00 00 04 00 00 00` | 17B |
| `readPID`（Boot） | `05 08 00 67 00 00 00 00 02 00 00 00 00 00 00 00` | 17B |
| `readVersionInBoot`（Boot） | `05 05 00 67 00 00 00 00 04 00 00 00 40 70 01 01` | 17B |
| `EraseFlash`（Boot） | `05 04 00 00 00 00 00 00 00 10 00 00 <addr LE 4B>` | 17B |
| `WriteFirmware`（Boot） | `05 03 00 00 40 00 <crc 2B> <image_len 4B> <addr 4B> <64B>` | 81B |
| `WriteVersion`（Boot） | `05 C4 00 00 18 00 00 00 <image_len 4B> 00 00 00 00 <24B>` | 41B |
| `WriteFirmwareVersion`（Boot） | `05 02 00 00 04 00 <crc 2B> 04 00 00 00 40 70 01 01 <ver 4B>` | 21B |
| `JumpApp`（Boot） | `05 06 00 00 00 00 00 00 00 00 00 00 00 80 01 01` | 17B |

## 5. 解析・完全性検証・機密管理

- **解析**: `btmon -r <file> -T -P`、または Wireshark / `tshark` で `btatt.opcode == 0x52 || btatt.opcode == 0x1b`（MTU は `0x02 || 0x03`）を確認し、サービス探索から `FF21` / `FF23` の handle を特定する。
- **完全性条件（1 つでも欠ければやり直し）**: (1) btmon 正常終了（exit 0、SIGKILL 不可）、(2) 先頭 8B `btsnoop\0`、(3) 最終フレームあり（Phase A: `readVersionInBoot` 応答、Phase B session 2: Jump link close + `"80ND"` 再広告）、(4) 期待フレーム数一致（`image_len / 64` の `0x0003`、`ceil(image_len / 4096)` の `0x0004`、`0x00C4`/`0x0002`/`0x0006` 各 1 回以上）、(5) size と SHA-256 を記録。
- **機密管理（資格情報として扱う）**:
  - btsnoop には周囲機器を含む BD_ADDR・広告名・SMP 鍵素材（LTK/IRK/CSRK）・暗号化ペイロードが含まれ得る。Android `adb bugreport` には広範な PII が含まれるため、btsnoop 抽出後に zip を破棄する。
  - 生ファイルはコミット・共有・チャット貼付せず、解析後は削除（保管する場合は暗号化）。
  - 共有用抜粋は対象 handle の ATT（`btl2cap.cid == 0x0004 && btatt.handle == <handle>`）のみ抽出し、共有前に `tshark` で **SMP / 非 ATT（`btsmp || btl2cap.cid != 0x0004`）が 0 件**、**暗号化 HCI イベント/コマンド（`0x08`/`0x3e`/`0x2019`/`0x201a`）が 0 件**、対象 handle が 1 件以上であることを検証する。すべての BD_ADDR・広告名・シリアルは伏字にする。
- **EVIDENCE 追記**: [OTA_BLE_EVIDENCE.md](OTA_BLE_EVIDENCE.md) 末尾に「§13 実機 btsnoop（環境・`.dat` / capture SHA-256・16 項目判定・伏字済み抜粋と抽出条件・完全性・チェックポイント判定）」を追記し、確定項目のみ [PROTOCOL_SPEC.md](PROTOCOL_SPEC.md) §8.5 を更新する。実装と食い違ってもその場でコードを直さず別 PR（Tier 3）で行う。

## 6. 安全手順と失敗時復旧

### 6.1 共通不変条件

1. 公式 `.dat` のみ使用（`28 + header[8] == file_size` と SHA-256 を確認）。
2. バッテリー **50% 未満または確認不能なら中止**。AC 給電・スリープ無効・近接配置。
3. 事前に 2768B config とマクロ 3 slot をバックアップし SHA-256 を記録。
4. 書込区間は非キャンセル・no retry。BLE 単一所有を守る。

### 6.2 Phase B 解禁条件（8 条件すべて + ユーザー承認）

1. `eightbitdo-core/ota-bench` が main の reviewed SHA にある。
2. (d) 公式アプリ capture を試し、不可理由を EVIDENCE に記録済み。
3. Phase A 完了・記録済み。
4. 復旧可能な専用個体（＋可能なら検証専用ホスト）を使用。
5. 公式 `.dat` の size / SHA-256 / `load_base` / `image_len` / 単一セクションを確認済み。
6. `allow_unverified_wire = true` は非コミットの一時ハーネスのみ（非キャンセル / no retry / ログ / `tmux` 等）。
7. 人間チェックポイント（`AfterFirstErase`）を省略しない。
8. `require_completion_ack` の方針を決めて EVIDENCE に記録。

### 6.3 Phase B の 2 セッション実行手順

1. **セッション 1（形状確認専用、`AfterFirstErase`）**:
   - btmon 開始 → `"80ND"` → `enterBoot` → `"8bt"` → `prepare_boot` → `write_firmware_with_checkpoint(..., AfterFirstErase)`（先頭 1 セクタのみ Erase）。
   - **初回合格条件**: (a) ACK が `cmd=0x0004/subcmd=0x0004` envelope または `cmd=0x0004/subcmd=0` direct、(b) `0x000E` 等エラーなし、(c) 要求 `offset == load_base` かつ `total_len == 0x1000`、(d) 1〜2 秒追加フレームなし、(e) リンク維持。判定を EVIDENCE 13.6 に記録（アドレス規約の正しさ自体は保証しないため残存リスクは専用個体＋ベンダ復旧で吸収）。
   - btmon 停止 → boot transport を明示 `disconnect()` / drop → BLE flock 解放 → `eightbitdo scan`（`--check` は `80ND` 専用なので不可）で `"8bt"` 広告を確認（1〜2 分出なければ電源再投入し最大 2 分待機、出なければ下記 (i) へ）。
2. **セッション 2（full run、完全性検証の対象）**:
   - 先頭セクタ消去後は App 領域が無効で `"8bt"` のままになるため、`enterBoot` は送らない。
   - 別ファイルで btmon 開始 → 新規 `OtaUpdate` で `"8bt"` へ直接接続 → `prepare_boot` → 通常 `write_firmware`（全消去 → 全 Write → `WriteVersion` → `WriteFirmwareVersion` → `JumpApp`）→ `"80ND"` 復帰・`ReadVersion` → btmon 停止。
   - セッション 2 は開始後に途中中止できない。途中に判断点が必要なら `AfterWrites` / `AfterWriteVersion` を挟む 3 セッション構成にする。

### 6.4 失敗時の復旧分岐

- **(i) 初回 Erase 異常・異常な部分記録・セッション 2 途中の切断/電源断**: 再試行も自前の全消去・再書込もせず即中止し、公式アプリ / サポートによる**ベンダ復旧**へ移る（計画的なセッション 1 → 2 の継続はここに含めない）。
- **(ii) wire 完全確定後の中断**: `"8bt"` 再接続 → `readPID` → 新規 `OtaUpdate` で先頭から全消去・再書込。
- **(iii) App Mode が正常（`"80ND"` 復帰 + `ReadVersion` 応答）**: 原則そのまま。config / マクロを読み戻してバックアップと SHA-256 比較し、消えていれば復元する。
- **(iv) `"8bt"` も `"80ND"` も見えない**: 電源再投入 → scan。見えない場合は推測で物理操作を繰り返さず、capture と最終ステップを記録して停止・報告（USB 復旧は未文書化のため不可）。

## 7. 残課題

1. (d) 公式アプリ capture の可否確認（未試行）。
2. 駆動手段の用意（一時ハーネスまたは薄い CLI）。
3. 公式 `.dat` の入手・単一/マルチセクション確認（マルチセクションは現実装の blocker）。
4. 採取後の ACK `offset` / `total_len` 照合実装と、`load_base_addr` / フラッシュ範囲の確定。
