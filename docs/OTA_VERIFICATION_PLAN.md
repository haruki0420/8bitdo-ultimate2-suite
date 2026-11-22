# BLE OTA 実機検証準備（btsnoop 採取・判定手順）

> **位置づけ**: 本書は **実機・BLE に触れない準備物**である。btsnoop の採取・実機書込は、
> 本書の手順と安全条件を確認したうえで、別途ユーザーの承認を得て実施する。
> 本書の作成 PR ではスクリプトを実行せず、実機にも接続しない。
>
> **対象**: 8BitDo Ultimate 2 Bluetooth（`0x600F`）の BLE OTA。App Mode `"80ND"`
> （service `FF10`）から Bootloader `"8bt"`（service `FF20` / characteristic `FF21`・`FF23`）
> へ遷移する経路のみ。USB OTA は **対象外**（TX フレーミング未文書化。§8.4 / AGENTS.md §4.4）。
>
> **関連**: [PROTOCOL_SPEC.md](PROTOCOL_SPEC.md) §8 /
> [OTA_BLE_EVIDENCE.md](OTA_BLE_EVIDENCE.md) /
> [SESSION_OPERATIONS.md](SESSION_OPERATIONS.md) /
> [crates/eightbitdo-core/src/ota.rs](../crates/eightbitdo-core/src/ota.rs) /
> [scripts/ota/capture-btsnoop.sh](../scripts/ota/capture-btsnoop.sh)

---

## 1. 目的

[PROTOCOL_SPEC.md](PROTOCOL_SPEC.md) §8.5「未検証事項（実機 btsnoop 待ち）」のうち、
**wire で決着できる 1〜13** を実機 btsnoop で確定する。14（trust boundary）・15（バッテリー /
取得検証）は本書の対象外。

| 決めたいこと | 現在の状態 | 本書での扱い |
| --- | --- | --- |
| Bootloader RX フレーミング | 仮定（prefix なし） | 採取項目 1（**Phase A**） |
| `readPID` / `readVersionInBoot` 応答 | 仮定（envelope または直エコー） | 採取項目 2・3（**Phase A**） |
| ready ACK / 完了 ACK の区別 | 未確認（実装は既定 off のオプトイン） | 採取項目 4・5（**Phase B**） |
| `gamepad_type` の由来 | プロジェクト規約（PID ホワイトリスト） | 採取項目 14（**Phase A**） |
| Erase / Write のアドレス規約 | spec と証跡が矛盾 | 採取項目 7・8（**Phase B**） |
| `WriteVersion` payload | spec と証跡が矛盾 | 採取項目 11（**Phase B**） |
| `EraseFlash` の `chunk_len` | spec と実装が不一致 | 採取項目 9（**Phase B**） |
| `WriteFirmware` のフレーム長 | spec と証跡が矛盾 | 採取項目 10（**Phase B**） |
| `enterBoot` の成立 | 未確認 | 採取項目 12（**Phase A**） |
| タイムアウト差 | spec と証跡が矛盾 | 採取項目 15（read は Phase A、書込は Phase B） |
| Bootloader MTU | 未確認 | 採取項目 13（**Phase A**） |
| Jump と App Mode 復帰 | 未確認 | 採取項目 16（**Phase B**） |
| マルチセクション `.dat` | blocker | `.dat` の静的確認（btsnoop 不要） |
| 遅延 / 重複 ACK の誤帰属 | 未確認 | 採取項目 3〜6・15 の順序と `offset`/`total_len`（**Phase B**） |

> **Phase A と Phase B の非対称性**: Erase / Write の ACK・アドレス・`chunk_len`・
> `WriteFirmware` / `WriteVersion` のフレーム長・Jump は **書込を送らないと観測できない**。
> Phase A で確定できるのは RX フレーミング・read 応答・MTU・characteristic・enterBoot までで、
> これらが揃っても「書込 wire は確定」ではない（§2.3）。

判定結果は [OTA_BLE_EVIDENCE.md](OTA_BLE_EVIDENCE.md) に追記し、確定した項目だけ
[PROTOCOL_SPEC.md](PROTOCOL_SPEC.md) §8.5 を更新する（§7）。

---

## 2. 前提と現状

### 2.1 実装の現在地（`crates/eightbitdo-core/src/ota.rs`）

- BLE 専用の OTA 状態機械は実装済み。`Idle` → `EnteringBoot` → `AwaitingBootReconnect` →
  `PreparingBoot` → `Ready` → `Erasing` / `Writing` / `Finalizing` → `Complete`（失敗時 `Failed`）。
- `readPID`（`0x0008`）→ `gamepad_type` 判定 → 任意で `readVersionInBoot`（`0x0005`）は
  **フラッシュを送らない**。フラッシュは `write_firmware` のみ。
- `OtaConfig::allow_unverified_wire` は **既定 false**。false の間は `write_firmware` が
  フラッシュフレームを 1 つも送らず `InvalidRequest` で拒否する。
- `OtaConfig::require_completion_ack` も **既定 false**（ready/completion ACK の実形状が
  未確認のため）。MTU は `mtu - 3 >= 81`（`OTA_MIN_MTU = 84`）を Erase 前に検査する。
- リトライは無い。ACK / 応答タイムアウトで即 fail-closed。復旧は **最初から全消去・再書込**。
- **CLI / GUI / UniFFI への配線は無い**（PROTOCOL_SPEC §8.5 のとおり配線は保留）。したがって現時点の
  リポジトリには「実機 OTA を駆動するコマンド」が存在しない。

### 2.2 採取を駆動する手段（本書の範囲外）

btsnoop を取るには、実際に wire を流す駆動側が必要になる。次のいずれかを使う。

| 優先 | 手段 | 状態 | 注意 |
| --- | --- | --- | --- |
| 1 | **(d) 公式アプリ + HCI btsnoop** | 未試行 | **最初に試す**。Android 公式アプリで OTA を完遂し、HCI スヌープログ（§3.2、root 不要の bugreport 経路を含む）を取る。自前コードを書込経路に載せないため、最も安全な一次証跡になる |
| 2 | (a) 配線済みフロントエンド | 未実装（未マージ） | `OtaUpdate` を呼ぶだけの薄い CLI で足りる |
| 3 | (b) コミットしない一時ベンチハーネス | 未作成 | `OtaUpdate` / `BleTransport` / `BleBootTransport` を直接呼ぶ。**Phase B では `with_unverified_wire_allowed(true)` が必要** |
| — | (c) 公式 Windows ソフト | 挙動未検証 | バージョン・コマンド列・btsnoop 相当の取得可否が未確認。**証跡として採用しない** |

- **(d) を最初に試す**。公式アプリが boot 遷移・書込・Jump まで実行できるなら、Erase / Write /
  ACK / アドレス / Jump の実形状が自作コードなしで手に入る。EVIDENCE §4 の静的解析では
  Android APK の `enterBoot` / `readPID` / `jumpApp` に call が無い（dead code）ため
  **完遂できる保証はない**が、試行して「不可」と判定した場合は、その理由（どの段階で止まったか、
  どのコマンドが wire に出なかったか）を EVIDENCE に記録する。
- **(d) が不可である理由を記録できた場合に限り** Phase B（§8.3）へ進む。理由の記録が無いまま
  Phase B を実施しない。
- 本 plan の採取・判定手順（§3〜§7）は、(d) 公式アプリでも (a) / (b) でも使える。駆動手段の
  実装判断は本書の範囲外とする（残課題 §9-2）。
- **(b) 一時ハーネスの必須仕様**:
  - `erase` / `write` / `finalize` 区間を **非キャンセル**にする（タイムアウトや UI からの
    キャンセルで future を drop しない）。
  - **no retry**: `OtaUpdate` の「タイムアウトで即 fail-closed、同じコマンドを再送しない」
    挙動をラップして再試行しない。
  - 各ステップで `OtaWriteState` / `OtaPhase` / `packets_sent` / `sectors_erased` /
    `chunks_written` を**標準エラーまたはログに記録**する（失敗時にどの段階で止まったかを
    capture と突き合わせる）。
  - **通常使用していない個体**（復旧可能な専用コントローラ）と、可能なら検証専用ホストで
    実行する。通常使用のコントローラ・ホストでは行わない。
  - ハーネスはコミットしない。`with_unverified_wire_allowed(true)` はハーネス内で明示する。

### 2.3 段階（Phase A / Phase B）

| Phase | 内容 | フラッシュ | `allow_unverified_wire` |
| --- | --- | --- | --- |
| **A（読み取りのみ）** | enterBoot → `"8bt"` 再接続 → `readPID` → `readVersionInBoot` | 送らない | **false のままで可**（`prepare_boot` はフラッシュを送らない） |
| **B（初回書込）** | Erase / Write / WriteVersion / WriteFirmwareVersion / Jump | 送る | **true が必須**（一時ハーネスのみ） |

- **Phase A で確定できるのは次の範囲だけ**（フラッシュを送らないため）:
  1. Bootloader RX フレーミング（prefix の有無）
  2. `readPID` / `readVersionInBoot` の応答形状（envelope / 直エコー、payload 長）
  3. `"8bt"` の MTU
  4. 使用 characteristic（FF21 / FF23）と `gamepad_type` の手掛かり
  5. `enterBoot` の ACK / link close
  6. read のタイムアウト実測
- **Phase A では確定できない**: Erase / Write の ACK 形状、アドレス規約、Erase の `chunk_len`、
  `WriteFirmware` / `WriteVersion` のフレーム長、Jump。これらは **Phase B の書込でしか
  観測できない**（Phase A の結果だけでは「ACK / アドレス確定」とは言えない）。
- **Phase B は「未確定 wire での初回書込」**である。したがって:
  - (d) 公式アプリ capture を先に試し、不可の理由を記録済みであること（§2.2）
  - 復旧可能な専用個体・検証専用ホストで行うこと
  - **最初の Erase で ACK 不明 / タイムアウト / アドレス疑いが出たら、その場で中止**する
    （再試行しない。§8.4 (i)）
  - ユーザーの承認があること
- どちらの Phase も enterBoot 後は App Mode に自動復帰しない。終了時は電源再投入で復帰させる
  （§8.2）。

### 2.4 この準備 PR で触らないこと

- 実機・BLE・アダプタに一切触れない（`btmon` の起動、`eightbitdo` の実行、`bluetoothctl` を含む）。
- OTA の CLI / GUI / UniFFI 配線、`allow_unverified_wire` の既定値変更は行わない。

---

## 3. 採取環境

### 3.1 Linux BlueZ（第一候補）

| 要件 | 内容 |
| --- | --- |
| ツール | BlueZ の `btmon`（`command -v btmon` で存在確認。`btmon --help` でオプション確認） |
| 権限 | HCI monitor の購読に **root または `CAP_NET_ADMIN`** が必要。`sudo btmon ...` |
| アダプタ | コントローラを接続する BLE アダプタ 1 つ。他の BLE 用途と共有しない |
| 複数アダプタ | **`-i hciX` で対象を必ず指定**する（`btmon` は全アダプタを購読し、他アダプタの他機器の通信も混入する）。使用アダプタ名を操作ログと EVIDENCE に記録する |
| ホスト | AC 給電、スリープ / ハイバネート無効、Bluetooth の省電力設定を変更しない |
| 時刻 | ホストの時計を NTP 等で合わせる（btsnoop は絶対時刻を持つ） |

- **単一所有ロックとの関係**: `btmon` は受動モニタであり、`eightbitdo` の per-user
  ロック（`~/.cache/8bitdo/8bitdo-ble.lock`）を取得しない。ロックと競合しないが、
  `eightbitdo session` が動いている間は OTA 駆動側が接続できないため、事前に `quit` で
  停止する（[SESSION_OPERATIONS.md](SESSION_OPERATIONS.md) §3）。
- 採取中に `bluetoothctl scan` / `connect` などの**接続操作を併走させない**（`In Progress`
  とロック競合の原因）。`btmon` 以外のモニタを同時に起動しない（`pgrep -a btmon` で確認）。
- `btmon` は全アダプタを購読する。対象を絞る場合は `-i hciX` を使う
  （[scripts/ota/capture-btsnoop.sh](../scripts/ota/capture-btsnoop.sh) は `-i` を透過する）。

### 3.2 Android btsnoop（(d) の主経路。root 不要の bugreport を含む）

| 手順 | 内容 |
| --- | --- |
| 有効化 | 開発者向けオプション → 「Bluetooth HCI スヌープログ」を有効化 → Bluetooth を OFF/ON |
| 取得（root） | `adb root` → `adb pull /data/misc/bluetooth/logs/btsnoop_hci.log` |
| 取得（非 root） | `adb bugreport ota-bugreport.zip` → zip 内 `FS/data/misc/bluetooth/logs/btsnoop_hci.log` を探す |
| 取得（旧 Android） | `adb pull /sdcard/btsnoop_hci.log` |
| 解析 | そのまま Wireshark で開ける（btsnoop 形式） |

- **現状の制約（静的解析）**: 公式 Android APK v2.01 は `enterBoot` / `readPID` /
  `readVersionInBoot` / `jumpApp` を送出しない（[OTA_BLE_EVIDENCE.md](OTA_BLE_EVIDENCE.md)
  §4 の dead code 判定）。**ただしこれは静的解析の結論であり、実機の公式アプリ OTA が
  boot 遷移を行うかは未確認**なので、まず (d)（§2.2）として実機で試す。不可と確認できた
  場合に限り「Android 単独では駆動できない」を確定させ、理由を EVIDENCE に記録する。
  将来モバイルに OTA を配線した後の採取経路にもなる。
- ログのバッファは有限でローテート / 切り詰めがある。採取直後に取得し、可能なら取得前に
  ログを新規化する。Android のバージョンによってパスが異なるため、bugreport 内を検索する。

### 3.3 macOS / Windows

本 plan の対象外。Windows 公式ツールでの OTA は挙動が未検証であり、証跡にしない。
必要になった時点で別途 plan を作る。

---

## 4. 採取タイミング（enterBoot 前〜Jump 後）

### 4.1 タイムライン

**開始は enterBoot より前**（可能なら BLE 接続前）。**停止は Jump 直後ではなく、App Mode
`"80ND"` への復帰とバージョン読み出しを確認した後**。接続前から取ることで、サービス探索
（handle → UUID）と MTU 交換も同じファイルに残り、Wireshark が characteristic を解決できる。

| # | 操作 | 採取側で見るもの | 備考 |
| --- | --- | --- | --- |
| 0 | ホスト時刻を記録 | `date -u +%Y-%m-%dT%H:%M:%SZ` | 操作ログと btsnoop の突き合わせ用 |
| 1 | **btmon 開始** | — | 接続前から開始（探索・MTU を含める） |
| 2 | scan → `"80ND"` 接続 | 広告名、MTU 交換、サービス探索、FF10/FF11・FF13 系の handle | App Mode |
| 3 | **enterBoot `0x000C`** | App TX（prefix `0x04`）、ACK または link close | 採取の起点 |
| 4 | `"8bt"` 広告 → 再接続 | 広告名、MTU 交換、FF20 / FF21・FF23 のどれか | **characteristic の index が `gamepad_type` の手掛かり**（EVIDENCE §6） |
| 5 | `readPID` `0x0008`（5 s） | 応答形状と PID 値（`0x600F` 期待） | Phase A |
| 6 | `readVersionInBoot` `0x0005`（5 s、offset `0x01017040`） | 応答形状と version 値 | Phase A |
| 7 | （Phase B のみ）Erase / Write / WriteVersion / WriteFirmwareVersion / Jump | 各要求と ACK、payload、`offset` / `total_len` | フラッシュ形状 |
| 8 | Jump 後の link close → `"80ND"` 再広告 → 再接続 → `ReadVersion` | 復帰とバージョン | **Phase B の成功確認** |
| 9 | **btmon 停止** | — | 復帰確認後 5〜10 秒待って停止 |

- 各操作の壁時計時刻を 1 秒単位で記録する（`date -u +%Y-%m-%dT%H:%M:%SZ`）。
- 採取中は他の BLE 接続（Switch / 2.4G レシーバ等）を切っておくと切り分けやすい（必須ではない）。
- コントローラの自動電源断・スリープに注意し、採取中は操作を継続する。

### 4.2 採取コマンド

```sh
# 開始（root または CAP_NET_ADMIN が必要。対話端末で実行）
scripts/ota/capture-btsnoop.sh -o .tmp/ota-capture
#   → .tmp/ota-capture/btsnoop-YYYYmmddTHHMMSSZ.snoop
#   → 案内に従って OTA を実施し、App Mode 復帰後に Enter（または SIGINT）で停止
```

- **（d) の Android 採取ではこのスクリプトを使わない**。端末の HCI スヌープログを §3.2 の
  手順で取得し、§4.1 の採取ウィンドウ（enterBoot 前〜 App Mode 復帰後）に相当する範囲を
  切り出す。
- スクリプトは `btmon -w` を起動し、停止時に `SIGINT` → `SIGTERM` → `SIGKILL` の順で
  **確実に btmon を終了**させる。`umask 077` でファイルを作る（BD_ADDR を含むため）。
- **複数アダプタがある場合は `-i hciX` を必ず指定**する（スクリプトは複数検出時に `-i` 無しを
  拒否する）。使用アダプタ名を操作ログと EVIDENCE に記録する。`btmon` は全アダプタを購読する
  ため、指定しないと他アダプタの他機器の通信が混入する（§7.4）。
- 手動で行う場合は `sudo btmon -w capture.snoop`、停止は Ctrl-C（btmon が btsnoop を閉じる）。
- 出力先 `.tmp/` は `.gitignore` 済み。**capture ファイルはコミットしない**（§7.4）。

---

## 5. 確認項目リスト

### 5.1 項目表

| # | 確認項目 | btsnoop での見方 | 仕様 / 実装の期待 | 証跡（EVIDENCE）との差 | PROTOCOL_SPEC §8.5 |
| --- | --- | --- | --- | --- | --- |
| 1 | **Bootloader RX フレーミング** | `"8bt"` の Notification の先頭バイト | 16B ヘッダ + payload、**prefix なし**（App の実測と同じ）。`0x05` は TX のみ | 未確認 | 1 |
| 2 | **`readPID` 応答** | `0x0008` 送信後の Notification | `cmd=0x0004` envelope（`subcmd=0x0008`）または `cmd=0x0008` 直エコー。payload 2B、`total_len`/`offset`/`crc16` を記録 | 未確認（payload 長 2B は実装仮定） | 2 |
| 3 | **`readVersionInBoot` 応答** | `0x0005` 送信後（要求 `offset=0x01017040`） | 4B payload（LE `u32`）、envelope / 直エコー、ready ACK が先行するか | 未確認 | 2 |
| 4 | **ready / 完了 ACK（Erase）** | `0x0004` ごとの Notification 数と時刻 | 1 個（最初の一致 ACK で完了）か 2 個（payload-less ready → 完了）か。payload-less が先行するか | 未確認。実装は既定で 1 個扱い | 3 |
| 5 | **ready / 完了 ACK（Write）** | 各 `0x0003` ごとの Notification 数・順序・遅延 | 1 個か 2 個か。次チャンク送信前に完了 ACK が来るか | 未確認 | 3 |
| 6 | **ACK の `offset` / `total_len`** | ACK ヘッダの `+0x0C` / `+0x08` | 要求と同値か、0 か、別値か（ACK にシーケンス番号は無い） | 未確認 | 13 |
| 7 | **Erase アドレス規約** | `0x0004` の `offset` | spec / 実装は `load_base + sector_off`。EVIDENCE §3 はイメージ相対 `sector_off` | **矛盾** | 5 |
| 8 | **Write アドレス規約** | `0x0003` の `offset` | 同上。`.dat +0x04` の `load_base` と比較する | **矛盾** | 5 |
| 9 | **Erase の `chunk_len`** | `0x0004` の `+0x04` とフレーム長 | 実装は `chunk_len = 0` / 17B。EVIDENCE §3 は宣言 `0x1000` | **不一致** | 7 |
| 10 | **WriteFirmware のフレーム長と payload 元** | `0x0003` の ATT write 長、`chunk_len`、先頭 64B | 実装は 81B（prefix + 16B + 64B）、`chunk_len=0x40`、payload は `.dat[0x1C..]`。EVIDENCE §7.1 は 16B 送信・`+0x1C` を足さない | **矛盾** | 8 |
| 11 | **`WriteVersion` payload** | `0x00C4` のフレーム長、`chunk_len`、crc | 実装は 41B（24B payload、`chunk_len=0x18`、`crc16=0`）。EVIDENCE §2 は 16B（`chunk_len=0`、crc `0xFFFF`） | **不一致** | 6 |
| 12 | **`enterBoot` `0x000C`** | App TX とその後 | ACK が来るか、link close だけか。送信直後の切断時刻 | 未確認（APK では dead code） | 9 |
| 13 | **MTU** | `Exchange MTU Request` / `Response`、最大 ATT write 長 | 81B フレームが 1 回の ATT write で通るか。App の実測は 92 / 200 | Boot は未確認 | 11 |
| 14 | **characteristic / `gamepad_type`** | `"8bt"` で使われた handle が FF21 か FF23 か | FF21（index 0）= type 0、FF23（index 1）= type 1。PID `0x600F` との整合 | 未確認 | 4 |
| 15 | **タイムアウト実測** | 要求送信 → ACK / 応答の経過時間 | Erase 10 s / Write 1 s / WriteVersion 3 s / read 5 s に対して十分余裕があるか | 読取は Android 2 s | 10 |
| 16 | **Jump / 復帰** | `0x0006`（offset `0x01018000`）→ link close → `"80ND"` | ACK か link close か。復帰までの時間、復帰後の version | 未確認 | 未検証事項外 |

補足:

- **Phase A で確定できるのは #1〜#3・#13・#14（と #15 の read 部分）だけ**である。
  **#4〜#12・#16 は Phase B（初回書込）でしか観測できない**。Phase A の結果だけで
  「ACK / アドレス / フレーム長が確定した」と扱わない。
- **#10 の payload 元**は、先頭 64B を `.dat[0x00..0x40]`（28B ヘッダ込み）と
  `.dat[0x1C..0x5C]`（イメージ先頭）のどちらと一致するかで判定する。EVIDENCE §7 は
  Android 実装が先頭に 28B ヘッダを混ぜる可能性を報告しており、実装差がそのまま出る。
- **マルチセクション `.dat`** は btsnoop ではなく `.dat` の静的確認で判定する。
  `file_size > 28 + header[8]` なら trailing data（2 セクション目）がある。現実装は
  `OtaInvalidFirmware` で拒否する（blocker）。
- ACK にシーケンス番号が無いため、#4〜#6 は「**要求と ACK の対応が一意に取れるか**」を
  タイムラインで確認する。遅延 ACK の誤帰属（PROTOCOL_SPEC §8.5 13）の判断材料になる。

### 5.2 比較用の期待フレーム（HEAD `4004757` の実装が送る形）

ヘッダは LE で `cmd(2) | subcmd(2) | chunk_len(2) | crc16(2) | total_len(4) | offset(4)`、
TX は `1B prefix + 16B header + payload`。RX は **prefix 無し**を仮定（App の実測と同じ）。
CRC は payload に対する CRC-16/MODBUS（`0xA001`）。

| コマンド | フレーム（prefix 込み） | 長さ |
| --- | --- | --- |
| `enterBoot`（App） | `04 0C 00 67 00 00 00 00 04 00 00 00 04 00 00 00` | 17B |
| `readPID`（Boot） | `05 08 00 67 00 00 00 00 02 00 00 00 00 00 00 00` | 17B |
| `readVersionInBoot`（Boot） | `05 05 00 67 00 00 00 00 04 00 00 00 40 70 01 01` | 17B |
| `EraseFlash`（Boot） | `05 04 00 00 00 00 00 00 00 10 00 00 <addr LE 4B>` | 17B |
| `WriteFirmware`（Boot） | `05 03 00 00 40 00 <crc LE 2B> <image_len LE 4B> <addr LE 4B> <64B>` | 81B |
| `WriteVersion`（Boot） | `05 C4 00 00 18 00 00 00 <image_len LE 4B> 00 00 00 00 <24B>` | 41B |
| `WriteFirmwareVersion`（Boot） | `05 02 00 00 04 00 <crc LE 2B> 04 00 00 00 40 70 01 01 <version LE 4B>` | 21B |
| `JumpApp`（Boot） | `05 06 00 00 00 00 00 00 00 00 00 00 00 80 01 01` | 17B |

- `EraseFlash` の `addr` は実装では `load_base + sector_index * 0x1000`。`chunk_len = 0`。
- `WriteFirmware` の `addr` は `load_base + chunk_offset`。`total_len = image_len`。
- `WriteVersion` の payload は `.dat[0x00..0x18]`（28B ヘッダの先頭 24B）。`crc16 = 0`。
- `WriteFirmwareVersion` の version は `.dat +0x00` の `u32`。`crc16` は 4B に対する値。
- `enterBoot` は App Mode なので prefix は `0x04`。それ以外は Boot の `0x05`。

### 5.3 判定の観点

各項目を次のいずれかに分類する。

| 判定 | 意味 | 次の動作 |
| --- | --- | --- |
| **仕様一致** | spec / 実装の期待どおり | PROTOCOL_SPEC §8.5 を「確定」に更新 |
| **証跡一致** | Android 実装の報告どおり（spec と違う） | どちらが実機の正かを記録。実装変更は別 PR（Tier 3） |
| **どちらとも違う** | 第 3 の形状 | **未確定のまま**。この項目に依存する実装・配線を止める |
| **応答なし / 切断** | コマンド不成立 | Phase B の**初回 Erase** なら §8.4 (i) で中止（再試行しない）。それ以外は成立条件をやり直す。フラッシュは触らない |

---

## 6. 採取後の解析手順

### 6.1 `btmon` で読む

```sh
# 全フレームを時刻付きで読む（-t: 時刻、-T: 日付+時刻、-P: pager 無効）
btmon -r .tmp/ota-capture/btsnoop-YYYYmmddTHHMMSSZ.snoop -T -P | less

# ATT だけを抜き出す（TX: Write Command、RX: Handle Value Notification）
btmon -r .tmp/ota-capture/btsnoop-YYYYmmddTHHMMSSZ.snoop -T -P \
  | grep -E 'ATT: (Write Command|Handle Value Notification|Exchange MTU)' \
  > .tmp/ota-capture/att.txt
```

- `btmon -r` の出力は `ATT: Write Command (0x52)` / `ATT: Handle Value Notification (0x1b)` /
  `ATT: Exchange MTU Request (0x02)` の形で出る（表記は btmon のバージョンで異なる場合がある）。
  `-t` は時刻のみ、`-T` は日付 + 時刻、`-P` は pager 無効。
- TX フレームの先頭が `05` なら Boot、`04` なら App。`05` の次 2 バイトが `cmd`。
- `-i hciX` で対象アダプタを絞れる（`btmon` は全アダプタを購読するため）。

### 6.2 Wireshark / `tshark` で読む

Wireshark で `.snoop` をそのまま開く。表示フィルタの例:

| 目的 | 表示フィルタ |
| --- | --- |
| 全 ATT | `btatt` |
| MTU 交換 | `btatt.opcode == 0x02 || btatt.opcode == 0x03` |
| 書き込み（TX） | `btatt.opcode == 0x52`（Write Command）/ `0x12`（Write Request） |
| 通知（RX） | `btatt.opcode == 0x1b`（Handle Value Notification） |
| Boot 系だけ | `(btatt.opcode == 0x52 || btatt.opcode == 0x1b) && btatt.value[0] == 0x05` |
| 対象 handle | `btatt.handle == 0x0013` など（下記で特定） |

- **handle の特定**: 接続前から採取しているので、サービス探索（`Read By Type Response` 等）が
  同じファイルに残る。Wireshark の ATT ツリーで `FF20` / `FF21` / `FF23` の handle を確認し、
  `btatt.handle` で絞る。**FF21 か FF23 かが `gamepad_type` の手掛かり**（EVIDENCE §6）。
- **値の確認**: パケット詳細の `Attribute Protocol → Value` を開く。列に
  `btatt.value` / `btatt.handle` / `btatt.opcode` / `frame.time_relative` を出すと追いやすい。
- フィールド名・byte-array のインデックス記法（`btatt.value[0]`）は Wireshark のバージョンで
  異なる場合がある。フィルタが効かない場合は「値」列を目視で確認する。
- `tshark` がある場合の抽出例:

```sh
tshark -r .tmp/ota-capture/btsnoop-YYYYmmddTHHMMSSZ.snoop \
  -Y 'btatt.opcode == 0x52 || btatt.opcode == 0x1b' \
  -T fields -e frame.number -e frame.time_relative -e btatt.opcode -e btatt.handle -e btatt.value \
  > .tmp/ota-capture/att.tsv
```

- 生バイトのエクスポート: Wireshark の File → Export Packet Dissections → As Plain Text
  （「Packet bytes」を含める）、または `tshark -x`。本書へ貼る抜粋は BD_ADDR を伏字にする。

### 6.3 集計する数値

| 指標 | 出し方 | 判定に使う項目 |
| --- | --- | --- |
| Erase 1 回あたりの Notification 数 | `05 04` 送信から次の `05 03`（最初の Write）または次の `05 04`（次セクタ）までを数える | #4 |
| Write 1 チャンクあたりの Notification 数・順序 | `05 03` と `0x1b` の対応を時刻順に並べる | #5, #6 |
| 要求→ACK の遅延 | `frame.time_relative` の差 | #5, #15 |
| Erase→完了 ACK の遅延 | `0x0004` 送信から完了 ACK まで | #4, #15 |
| `offset` / `total_len` の一致 | 要求ヘッダと ACK ヘッダを突き合わせる | #6, #7, #8 |
| ATT write 長の最大値 | `btatt.value` の長さ（+ ヘッダ分） | #10, #13 |
| 総所要時間 | 最初の `05` から Jump の link close まで | #15, #16 |

---

## 7. 判定結果の記録（EVIDENCE への追記）

### 7.1 追記先と構成

[OTA_BLE_EVIDENCE.md](OTA_BLE_EVIDENCE.md) の末尾に、次の構成で 1 節を追加する。

```text
## 13. 実機 btsnoop（YYYY-MM-DD）

### 13.1 環境
| 項目 | 値 |
| --- | --- |
| ホスト / kernel / BlueZ | ... |
| 使用アダプタ（`-i hciX`） | ... |
| コントローラ FW version（App Mode `0x000B`） | ... |
| `"8bt"` の characteristic | FF21 / FF23 |
| Bootloader MTU | ... |
| 採取開始 / 停止（UTC） | ... |
| 駆動手段 | (d) 公式アプリ / (a) 配線済み / (b) 一時ハーネス |

### 13.2 ファームウェアとキャプチャ
| 項目 | 値 |
| --- | --- |
| `.dat` ファイル名 | ... |
| `.dat` size / SHA-256 | ... |
| `.dat` version / `load_base_addr` / `image_len` / セクション数 | ... |
| capture size / SHA-256（**必須**） | ... |

### 13.3 確認項目の判定
| # | 項目 | 判定（確定 / 仕様一致 / 証跡一致 / 未確認） | 根拠（フレーム番号・時刻） |
| --- | --- | --- | --- |
| 1 | Bootloader RX フレーミング | ... | ... |
| ... | ... | ... | ... |

### 13.4 フレーム抜粋（BD_ADDR は伏字）
（送信 / 応答の 16 進とヘッダ分解。必要最小限）
```

### 7.2 追記時のルール

- 既存の判定ラベル（**確定** = 直接確認 / **推測** / **未確認**）を踏襲する。
- **capture の SHA-256 は必須**。size と SHA-256 を記録できない capture は証跡として採用しない
  （`btmon -w` が途中で切れた、ファイルが空、コピーが壊れた等はやり直す）。
- 確定した項目だけ [PROTOCOL_SPEC.md](PROTOCOL_SPEC.md) §8.5 を更新する。矛盾が残る項目は
  未検証のまま残し、EVIDENCE に両論を書く。
- 実装と食い違った場合、**その場でコードを直さない**。EVIDENCE に記録し、実装変更は
  別 PR（Tier 3）で行う（ACK 照合・アドレス規約・フレーミングは write-path）。
- `readPID` の PID が `0x600F` 以外だった場合、その個体の OTA は **停止**する（実装は
  `OtaUnsupportedPid` で停止する。推測で type 0 のアドレスを使わない）。

### 7.3 PROTOCOL_SPEC の更新

- PROTOCOL_SPEC §8.5 の該当項目を「確定」にし、根拠として EVIDENCE の節番号をリンクする。
- §8.2 / §8.3 のフレーム表（アドレス規約・`chunk_len`・payload 長）が実機と違えば、
  spec 側を実機に合わせて修正する。**実装を先に変えない**。
- `require_completion_ack` の既定値は、実 wire の ACK 形状が確定してから決める
  （PROTOCOL_SPEC §8.5「CLI / GUI / UniFFI 配線時の必須検証」4）。

### 7.4 capture ファイルの扱い

- btsnoop には **BD_ADDR（デバイスの MAC アドレス）が多数**含まれる。対象コントローラだけでなく、
  **周囲の他機器（スマホ・イヤホン・PC・テレビ等）の BD_ADDR・広告名・接続履歴**も同じ
  ファイルに入る。生ファイルは**コミットしない**。`.tmp/`（gitignore 済み、スクリプトは
  `umask 077`）に置き、共有・issue 添付・チャット貼り付けもしない。
- Android の `adb bugreport` は btsnoop 以外に**端末のアカウント・アプリ・設定などの広範な
  PII** を含む。bugreport zip 自体を共有・コミットせず、btsnoop だけを `.tmp/` に取り出して
  残りは破棄する。
- コミットするのは判定表・環境表・SHA-256・**全 BD_ADDR を伏字にした**フレーム抜粋のみ。
  伏字は対象コントローラだけでなく、capture 内の**すべての** BD_ADDR / 広告名に適用する。
- 抜粋からもシリアル / 個体識別子を除く（`WriteAPPID` は type 1 では送らないが、
  ペイロードに個体情報が混ざる可能性があるため値は最小限にする）。
- 共有が必要な場合は、生ファイルではなく「判定表 + SHA-256 + 伏字済み抜粋」を共有する。

---

## 8. 安全手順

### 8.1 不変条件

1. **`allow_unverified_wire = false`（既定）のままでは書込不可**。`write_firmware` は
   フラッシュフレームを 1 つも送らず `InvalidRequest` で拒否する。この既定を実機で
   破らない（破るのは Phase B の一時ハーネスのみ）。
2. **実機書込は「未確定 wire での初回書込」であることを認識して行う**。Phase A で確定できるのは
   RX フレーミング・read 応答・MTU・characteristic・enterBoot まで（§2.3）。Erase / Write の
   ACK・アドレス・`chunk_len`・フレーム長は Phase B でしか観測できないため、「ACK / アドレスが
   確定してから書込む」という前提は**循環する**。したがって Phase B は
   (d) 公式アプリ capture を先に試して不可の理由を記録し、復旧可能な専用個体で行い、
   **初回 Erase の異常で即中止する**という条件でのみ実施する（§8.3）。
3. **公式ファームウェアのみ**を使う。配信元から取得し、ファイルサイズと SHA-256 を記録する。
   マルチセクション `.dat` は現実装では拒否される（blocker）。書込前に
   `28 + header[8] == file_size` を確認する。
4. **バッテリー確認**。満充電に近い状態で行う。残量が確認できない、または 50% 未満なら
   実施しない。host は AC 給電、スリープ無効、コントローラを近接させる。
5. **書込区間はキャンセルしない**。erase / write / finalize の途中で SIGINT・UI
   キャンセル・プロセス終了をしない（`ota.rs` の cancellation contract）。
6. **事前バックアップ**。2768B config とマクロ 3 slot をバックアップし、SHA-256 を記録する
   （[SESSION_OPERATIONS.md](SESSION_OPERATIONS.md) と AGENTS.md §6.4 の運用）。
   更新後に読み戻して比較し、消えていれば `config write` / `macro restore` で復元する。
7. **単一所有**。`eightbitdo session` や他の BLE 接続ツールを停止してから開始する。
8. **初回 Erase の異常で即中止**。最初の `EraseFlash` の ACK が不明 / タイムアウト /
   アドレス規約に疑いがある場合は、再試行せず、全消去・再書込に進まず、ベンダ復旧
   （§8.4 (i)）へ移る。

### 8.2 Phase A（読み取りのみ）の安全

- フラッシュを送らない。`prepare_boot`（`readPID` / `readVersionInBoot`）だけを実行する。
- enterBoot 後は App Mode に自動復帰しない。採取終了時は
  1. `"8bt"` から切断する
  2. コントローラの電源を切って再投入する
  3. `eightbitdo scan --check` で `"80ND"` が戻ることを確認する
- `"8bt"` に接続したまま放置しない（バッテリー消費・誤操作防止）。
- `"80ND"` が戻らない場合は、まず scan と再接続を試す。物理操作（ボタン押下）は
  最後の手段とし、推測で手順を作らない。

### 8.3 Phase B（初回書込）の前提

Phase B は **未確定 wire に対する最初のフラッシュ書込**である。次をすべて満たす場合のみ実施する。

1. (d) 公式アプリ capture を先に試し、「不可」の理由（どの段階で止まったか、どのコマンドが
   wire に出なかったか）を [OTA_BLE_EVIDENCE.md](OTA_BLE_EVIDENCE.md) に記録済み（§2.2）。
2. Phase A が完了し、RX フレーミング・read 応答・`"8bt"` の MTU・characteristic が
   EVIDENCE に記録済み（§8.2）。**Phase A で書込 wire は確定しない**ことを前提に置く。
3. §8.1 の 1〜8 を満たす。
4. **復旧可能な専用個体**（通常使用していないコントローラ）と、可能なら検証専用ホストで行う。
   通常使用中の個体・ホストでは行わない。
5. 一時ハーネスは §2.2 の必須仕様（非キャンセル / no retry / `OtaWriteState` ログ）を満たし、
   `OtaConfig::with_unverified_wire_allowed(true)` を明示する。**この設定をコミットしない**
   （`allow_unverified_wire` は削除またはベンチ専用に限定するのが最終形）。
6. `require_completion_ack` は Phase A / (d) の結果で決める。決められない場合は既定（off）で
   進め、ACK 形状を採取結果から確定する（§7.3）。
7. 書込前に `expected_load_base` が `.dat +0x04` と一致することを確認する（不一致なら
   Erase 前に `OtaInvalidFirmware` で停止する）。
8. **最初の Erase で ACK 不明 / タイムアウト / アドレス疑いが出たら、即中止**する
   （再試行も全消去・再書込もしない。§8.4 (i)）。
9. ユーザーの承認がある。

### 8.4 失敗時の復旧（失敗段階ごとの分岐）

**共通**: 再試行しない。最後の段階（`OtaWriteState::Untouched` / `EraseStarted` /
`PartiallyWritten` / `Written`）と capture を保存し、btmon は復旧が終わるまで止めない。

**(i) 最初の Erase で異常（ACK 不明 / タイムアウト / アドレス疑い）**

- **その場で中止**する。同じ Erase も Write も再送しない。
- 全消去・再書込に進まない（wire 未確定のまま書込を重ねない）。
- **ベンダ復旧**へ移る: 公式アプリ / 公式サポートの復旧手順に従う。本リポジトリにはベンダ復旧手順が
  無いため、推測で代替しない。コントローラの状態（`"8bt"` / `"80ND"` のどちらが広告されるか）と
  capture、最後に送ったフレームを記録して報告する。

**(ii) wire 確定後の中断（ACK 形状・アドレスが確定済みで、途中で失敗 / 切断）**

- `"8bt"` に再接続 → `readPID`（`0x0008`）→ `.dat` と `expected_load_base` を再確認 →
  **最初のセクタから全消去 → 全チャンク書込 → `WriteVersion` → `WriteFirmwareVersion` →
  `JumpApp`** を、新しい `OtaUpdate`（`Idle`）で `prepare_boot` からやり直す。
- 「wire 確定後」とは、EVIDENCE に ACK 形状とアドレス規約が記録され、同じ設定で再現できる
  状態を指す。確定していない場合は **(i)** として扱う。

**(iii) App Mode が正常（`"80ND"` が広告され、`ReadVersion` が返る）**

- **原則そのまま**にする。バージョン・config・マクロを確認し、**更新が必要な場合だけ**再更新する。
- 更新前と同じバージョンなら書込は未確定なので、「必要なら」enterBoot からやり直す。
  不要なら触らない（中途半端な再書込を増やさない）。
- config / マクロを読み戻して SHA-256 を比較し、消えていればバックアップから復元する。

**(iv) どちらの広告も見えない**

- コントローラの電源を切って再投入 → scan。それでも見えない場合は、**推測で物理操作を
  繰り返さない**。capture と最後のステップを記録して停止・報告する（USB 復旧は未文書化の
  ため使わない）。

復旧中も BLE の単一所有を守る（他の BLE ツールを起動しない）。

### 8.5 実施前チェックリスト

- [ ] 対象個体の FW version / config / マクロをバックアップし SHA-256 を記録した
- [ ] `eightbitdo session` など BLE を使うプロセスを停止した
- [ ] コントローラを `"80ND"` で接続できることを `scan --check` で確認した
- [ ] ホストは AC 給電・スリープ無効・時刻合わせ済み
- [ ] 複数アダプタ環境では `-i hciX` を指定し、使用アダプタを操作ログに記録した
- [ ] `btmon` を起動し、capture ファイルが作られることを確認した（enterBoot 前）
- [ ] Phase B の場合: (d) 公式アプリ capture を試し、不可の理由を EVIDENCE に記録した
- [ ] Phase B の場合: Phase A の判定が EVIDENCE に記録済みで、ユーザーの承認がある
- [ ] Phase B の場合: 公式 `.dat` の size / SHA-256 / `load_base` / `image_len` /
      セクション数を確認した
- [ ] Phase B の場合: バッテリー残量を確認した（50% 未満なら中止）
- [ ] Phase B の場合: `allow_unverified_wire=true` は一時ハーネスのみで、コミットしない
- [ ] Phase B の場合: ハーネスが非キャンセル / no retry / `OtaWriteState` ログを満たす
- [ ] Phase B の場合: 最初の Erase の異常（ACK 不明 / タイムアウト / アドレス疑い）で
      即中止する手順を確認した（再試行しない）

### 8.6 実施後チェックリスト

- [ ] btmon を停止し、capture ファイルの size / **SHA-256（必須）** / 使用アダプタを記録した
- [ ] App Mode `"80ND"` へ復帰し、`ReadVersion` でバージョンを確認した
- [ ] config / マクロを読み戻し、バックアップと SHA-256 を比較した
- [ ] 判定表を [OTA_BLE_EVIDENCE.md](OTA_BLE_EVIDENCE.md) に追記した
- [ ] capture 生ファイルをコミットしていない（`.tmp/` のみ）
- [ ] 抜粋に含めた**すべての** BD_ADDR / 広告名（周囲の他機器を含む）を伏字にした

---

## 9. 残課題

1. **(d) 公式アプリ capture の可否判定（未試行）**: 公式アプリで OTA を完遂できるか、HCI
   スヌープログに boot 遷移から Jump まで残るかを最初に確認する。不可なら「どの段階で止まったか」
   を EVIDENCE に記録する（§2.2）。
2. **駆動手段が無い**: CLI / GUI / UniFFI に OTA が未配線。Phase A から一時ハーネスか
   配線済みフロントエンドが必要（§2.2）。ハーネスはコミットしない。
3. **公式 `.dat` の入手・検証**: 配信 API のレスポンス（`md5` のクセを含む）と、
   ダウンロードしたファイルの SHA-256 / セクション数の記録方法を決める。
4. **マルチセクション `.dat` の blocker**: 現実装は単一セクションのみ。公式 `.dat` が
   マルチセクションなら更新できない（PROTOCOL_SPEC §8.5 12）。実測して対応方針を決める。
5. **ACK の `offset` / `total_len` 照合**: 採取後に実装する（PROTOCOL_SPEC §8.5 13）。遅延 ACK の
   誤帰属を防ぐ必須項目。
6. **`load_base_addr` とフラッシュサイズの確定**: `UT2_TYPE1_APP_BASE_MIN` /
   `UT2_TYPE1_APP_FLASH_END` の暫定ウィンドウを実測値に置き換える。
7. **capture の保管・共有ポリシー**: BD_ADDR（対象外の他機器を含む）と Android bugreport の
   PII 範囲を踏まえ、共有範囲を決める（§7.4）。
8. **Android / Windows 経路**: 現状は採取も駆動もできない。配線後に別 plan で扱う。

---

## 10. 参照

- [PROTOCOL_SPEC.md](PROTOCOL_SPEC.md) §8（BLE OTA 仕様・§8.5 未検証事項）
- [OTA_BLE_EVIDENCE.md](OTA_BLE_EVIDENCE.md)（Android v2.01 の静的解析証跡）
- [SESSION_OPERATIONS.md](SESSION_OPERATIONS.md)（BLE 単一所有・押下手順）
- [crates/eightbitdo-core/src/ota.rs](../crates/eightbitdo-core/src/ota.rs)（状態機械と安全ガード）
- [crates/eightbitdo-protocol/src/commands.rs](../crates/eightbitdo-protocol/src/commands.rs)（`boot` builder）
- [scripts/ota/capture-btsnoop.sh](../scripts/ota/capture-btsnoop.sh)（`btmon` 採取ヘルパー）
- [AGENTS.md](../AGENTS.md) §4（OTA の安全性）・§6（現在の状態）
