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
- **(d) の成立条件と副作用**:
  - 更新前の FW version を App Mode の `ReadVersion` で記録し、公式アプリが「更新対象」と
    判断する状態（現行 < 最新）を用意する。公式アプリが「更新不要」と判断すると wire が出ない。
    その場合は別個体 / 旧 FW で試すか、不可理由として記録する。
  - 端末の HCI スヌープログを有効化してから OTA を実行し、**採取後に設定を元に戻す**。
  - **副次効果**: 公式 OTA は対象個体の FW を変更する（config / マクロが初期化される可能性、
    ダウングレードできない可能性がある）。**専用個体**で行い、事前バックアップと、更新後の
    version / config / マクロの確認を行う（§8.2・§8.4 (iii)）。
- **(d) が成功した場合、Phase B は実施しない**。(d) の capture で Erase / Write /
  WriteVersion / WriteFirmwareVersion / Jump の全 wire が揃えば、それが一次証跡であり、
  未確定 wire での自前書込（Phase B）を重ねる必要はない。EVIDENCE の判定と実装へ進む。
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
  - **Erase チェックポイント（`ota-bench` feature の段階実行 API）**: 最初の Erase ACK を
    受けた時点で Write へ進まず**停止**し、人間が btmon で `offset` / `total_len` / ACK 形状を
    確認してから次へ進む（timeout では誤受理 ACK / 誤アドレスを検知できないため）。
    **実 API は `impl-ble-ota`（head `8077287`、main 統合中）で実装済みだが、これは
    `8077287` 時点の暫定**であり、レビューで API 名・シグネチャが変わり得る。
    **main 統合後の reviewed SHA の rustdoc を正**とする（feature 名は **`ota-bench` のまま
    再定義しない**）。
    - feature: **`eightbitdo-core/ota-bench`**（**既定 OFF**。無効時は型・メソッドが存在しない。
      ハーネスは `#[cfg(feature = "ota-bench")]` で使う）
    - ハーネス側の例: `eightbitdo-core = { path = "...", features = ["ota-bench"] }`、
      または `cargo run -p <harness> --features eightbitdo-core/ota-bench`
    - `OtaUpdate::write_firmware_with_checkpoint(ready, image, progress, checkpoint)`
    - `OtaCheckpoint::{AfterFirstErase, AfterWrites, AfterWriteVersion}`
    - 戻り値 `OtaBenchOutcome { checkpoint, sectors_erased, chunks_written, bytes_written,
      packets_sent, jump }`（停止時 `checkpoint: Some(..)` / `jump: None`、完走時
      `checkpoint: None` / `jump: Some(..)`）
    - 停止時の phase は **`OtaPhase::BenchStopped`**。端末は bootloader に残り、**同じ
      `OtaUpdate` で再開できない**（続行・復旧は新しい `OtaUpdate` + 先頭からの全消去・再書込）
    - 安全ゲートは通常と同一: **`allow_unverified_wire = true` 必須**、PID → `gamepad_type 1`、
      expected base 一致、MTU 81B フレーム。feature 無効ビルドでは未定義
    - **この API はまだ main に統合されていない**（`impl-ble-ota`）。統合までは Phase B を
      実施しない。
  - **端末切断対策**: `tmux` / `setsid` などで端末から切り離して実行し、SIGINT / SIGHUP /
    端末クローズで書込途中にプロセスが落ちないようにする。
  - **完走するまで「wire 確定」と呼ばない**: Erase / Write / WriteVersion /
    WriteFirmwareVersion / Jump の**全 wire が記録できて初めて**確定（§8.4 (ii)）。
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
  - **最初の Erase ACK でいったん停止し、人間が `offset` / `total_len` / ACK 形状を
    確認してから Write へ進む**こと（timeout では誤受理 ACK・誤アドレスを検知できない）。
    停止は
    **`OtaUpdate::write_firmware_with_checkpoint(.., OtaCheckpoint::AfterFirstErase)`**
    で行う（§2.2）。**Phase B の解禁条件は §8.3 の 8 条件すべて**（「bench API +
    `allow_unverified_wire` + 人間チェックポイント」は最低限の要約にすぎない）。
  - **最初の Erase で ACK 不明 / タイムアウト / アドレス疑いが出たら、その場で中止**する
    （再試行しない。§8.4 (i)）
  - ユーザーの承認があること
- 「wire 確定」は、**全コマンド（Erase / Write / WriteVersion / WriteFirmwareVersion /
  Jump）の wire が記録できた状態**を指す。Erase だけで Write 以降が未記録の部分記録は
  確定ではない（§8.4 (i)(ii)）。
- **Phase A は Jump を送らないため App Mode に自動復帰しない**（終了時は `"8bt"` 切断 →
  電源再投入で復帰。§8.2）。Phase B は Jump 後に `"80ND"` へ戻る（§4.1）。

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

**開始は enterBoot より前**（可能なら BLE 接続前）。接続前から取ることで、サービス探索
（handle → UUID）と MTU 交換も同じファイルに残り、Wireshark が characteristic を解決できる。

**停止点は Phase で異なる**:

- **Phase A（読み取りのみ）**: `readVersionInBoot` の応答を確認してから **1〜2 秒待って停止**する。
  Phase A は Jump を送らないため **App Mode には復帰しない**。停止後に `"80ND"` の復帰を
  待つと永遠に待つことになる。復帰は §8.2 の手順（`"8bt"` 切断 → 電源再投入 → `scan --check`）
  で行う。
- **Phase B**: Jump 後の link close → `"80ND"` 再広告 → 再接続 → `ReadVersion` を確認し、
  **5〜10 秒待ってから停止**する。Jump 直後に止めない（再起動と再接続の観測に必要）。

| # | 操作 | 採取側で見るもの | 備考 |
| --- | --- | --- | --- |
| 0 | ホスト時刻を記録 | `date -u +%Y-%m-%dT%H:%M:%SZ` | 操作ログと btsnoop の突き合わせ用 |
| 1 | **btmon 開始** | — | 接続前から開始（探索・MTU を含める） |
| 2 | scan → `"80ND"` 接続 | 広告名、MTU 交換、サービス探索、FF10/FF11・FF13 系の handle | App Mode |
| 3 | **enterBoot `0x000C`** | App TX（prefix `0x04`）、ACK または link close | 採取の起点 |
| 4 | `"8bt"` 広告 → 再接続 | 広告名、MTU 交換、FF20 / FF21・FF23 のどれか | **characteristic の index が `gamepad_type` の手掛かり**（EVIDENCE §6） |
| 5 | `readPID` `0x0008`（5 s） | 応答形状と PID 値（`0x600F` 期待） | Phase A |
| 6 | `readVersionInBoot` `0x0005`（5 s、offset `0x01017040`） | 応答形状と version 値 | Phase A。応答後 1〜2 秒で停止 |
| 7 | （Phase B のみ）Erase / Write / WriteVersion / WriteFirmwareVersion / Jump | 各要求と ACK、payload、`offset` / `total_len` | **Phase A はここを送らない**。`OtaCheckpoint::AfterFirstErase` で Erase ACK 後に停止（§8.3） |
| 8 | （Phase B のみ）Jump 後の link close → `"80ND"` 再広告 → 再接続 → `ReadVersion` | 復帰とバージョン | Phase B の成功確認。Phase A は復帰しない |
| 9 | **btmon 停止** | — | **Phase A: 手順 6 の後 1〜2 秒 / Phase B: 手順 8 の後 5〜10 秒** |

- 各操作の壁時計時刻を 1 秒単位で記録する（`date -u +%Y-%m-%dT%H:%M:%SZ`）。
- 採取中は他の BLE 接続（Switch / 2.4G レシーバ等）を切っておくと切り分けやすい（必須ではない）。
- コントローラの自動電源断・スリープに注意し、採取中は操作を継続する。

### 4.2 採取コマンド

```sh
# 開始（root または CAP_NET_ADMIN が必要。対話端末で実行）
# Phase A（read-only）: readVersionInBoot 応答の 1〜2 秒後に Enter
scripts/ota/capture-btsnoop.sh -o .tmp/ota-capture -p a
# Phase B（flash）: Jump 後の "80ND" 復帰と ReadVersion の後に Enter
scripts/ota/capture-btsnoop.sh -o .tmp/ota-capture -p b
#   → .tmp/ota-capture/btsnoop-YYYYmmddTHHMMSSZ.snoop
#   → 案内に従って OTA を実施し、Phase に応じた停止点で Enter（または SIGINT）
#   → -p を省略すると両 Phase の案内を表示する
```

- **（d) の Android 採取ではこのスクリプトを使わない**。端末の HCI スヌープログを §3.2 の
  手順で取得し、§4.1 の採取ウィンドウ（Phase に応じた停止点まで）に相当する範囲を
  切り出す。
- スクリプトは `btmon -w` を起動し、停止時に `SIGINT` → `SIGTERM` → `SIGKILL` の順で
  **確実に btmon を終了**させる。`umask 077` でファイルを作る（BD_ADDR を含むため）。
- **複数アダプタがある場合は `-i hciX` を必ず指定**する（スクリプトは複数検出時に `-i` 無しを
  拒否する）。使用アダプタ名を操作ログと EVIDENCE に記録する。`btmon` は全アダプタを購読する
  ため、指定しないと他アダプタの他機器の通信が混入する（§7.4）。
- 手動で行う場合は `sudo btmon -w capture.snoop`、停止は Ctrl-C（btmon が btsnoop を閉じる）。
- スクリプトの終了コード: **0 = 正常 / 2 = 使用エラー・btmon 起動失敗・他の btmon が稼働中
  （`--allow-concurrent` 無し） / 3 = capture が INVALID（空・btsnoop magic 不正・SIGKILL
  停止）**。exit 3 の capture は証跡にしない（§6.4）。他の btmon が稼働中なら中止する。
  既存の出力先ディレクトリは所有者を確認し、mode を `700` に締める（symlink は拒否）。
- `od` が無い環境ではスクリプトは magic を検証できない（警告のみ）。終了後に手動で
  `od -An -tx1 -N8 <file>` が `62 74 73 6e 6f 6f 70 00` であることを確認し、確認できなければ
  証跡にしない。
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

### 6.4 capture の完全性検証

採取ファイルを証跡として使う前に、次を確認する。1 つでも欠ければ**やり直す**（部分的な
capture は証跡にしない）。

1. **正常終了**: スクリプトが `capture saved` を出し、btmon が SIGINT で正常終了している
   （SIGKILL フォールバック、`capture file is empty`、`btmon exited immediately` は不可）。
   **SIGKILL で止めた capture は btsnoop の最終フレームが欠ける可能性があり、完全性を
   保証しない**。スクリプトは空ファイル / magic 不正 / SIGKILL 停止を `INVALID` として
   非 0 で終了する。
2. **btsnoop 形式**: 先頭 8 バイトが `btsnoop\0`（スクリプトは終了時にこの magic を検査する。
   `od` が無い環境では未検証になるため、§4.2 の手動確認を行う）。
3. **最終フレーム**: Phase B は Jump の link close と `"80ND"` 再広告 / 再接続、
   Phase A は `readVersionInBoot` の応答が記録されている。
4. **期待フレーム数**（Phase B）: `image_len / 64` 個の `0x0003`、`ceil(image_len / 4096)` 個の
   `0x0004`、`0x00C4` / `0x0002` / `0x0006` が各 1 回以上、`0x0008` / `0x0005` も存在。
5. **切り出し範囲**: 抜粋に使ったフレーム番号範囲を記録する（§7.4）。
6. 数値は EVIDENCE の 13.5 に記録する。

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
| `ota-bench` API（main 統合 reviewed SHA） | ... |

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

### 13.4 フレーム抜粋（BD_ADDR は伏字、SMP / ペアリング / 暗号化は除外）
- 抽出条件: フレーム番号 X..Y、フィルタ `btl2cap.cid == 0x0004 && btatt.handle == 0x....`
- 対象: 対象 GATT の write / notification のみ（SMP / ペアリング / 暗号化フレームは含めない）
（送信 / 応答の 16 進とヘッダ分解。必要最小限）

### 13.5 capture の完全性
| 項目 | 値 |
| --- | --- |
| btmon の終了状態 | 正常終了 / 強制終了（SIGKILL） |
| btsnoop magic | OK / NG |
| 最終フレーム（Jump link close / 80ND 再広告 / readVersion 応答） | ... |
| Erase 数 / Write 数 / WriteVersion / WriteFirmwareVersion / Jump の有無 | ... |
| 期待値との一致（`image_len/64` 等） | ... |
| 切り出し範囲（フレーム番号） | ... |

### 13.6 チェックポイント判定
| セッション | checkpoint | ACK 形状（payload-less / envelope / 直エコー） | `offset` / `total_len` | 続行可否 | 判断者・根拠 |
| --- | --- | --- | --- | --- | --- |
| 1 | AfterFirstErase | ... | ... | 可 / 不可 | ... |
| 2 | full run の最初の Erase | ... | ... | 可 / 不可 | ... |

（セッション 2 の最初の Erase も未確定 wire なので判定対象に含める。）
```

### 7.2 追記時のルール

- 既存の判定ラベル（**確定** = 直接確認 / **推測** / **未確認**）を踏襲する。
- **capture の SHA-256 は必須**。size と SHA-256 を記録できない capture は証跡として採用しない
  （`btmon -w` が途中で切れた、ファイルが空、コピーが壊れた等はやり直す）。
- **capture の完全性を確認**してから判定する（§6.4）。途中で切れた capture、期待フレーム数が
  足りない capture は証跡にしない。
- **抜粋には抽出条件（フレーム番号範囲・表示フィルタ・tshark コマンド）を記録**する。
- **抜粋から SMP / ペアリング / 暗号化フレームを除外**する（§7.4）。ATT CID（`0x0004`）に
  限定すれば SMP（CID `0x0006`）は入らないが、抽出後に一度確認する。
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

### 7.4 capture ファイルの扱い（資格情報として扱う）

btsnoop は「通信ログ」ではなく **資格情報を含む秘密情報** として扱う。

**含まれ得る機密**:

- 対象コントローラと周囲の他機器（スマホ・イヤホン・PC・テレビ等）の BD_ADDR・広告名・
  接続履歴。
- **SMP / ペアリング / 暗号化の鍵素材**: LTK / IRK / CSRK、鍵配送、`LE Start Encryption`、
  `LE Long Term Key Request/Reply`、ボンディング情報、暗号化開始後の ATT ペイロード。
- Android の `adb bugreport` は btsnoop 以外に端末のアカウント・アプリ・設定などの広範な PII。

**保管**:

- 生ファイルは **コミットしない**。`.tmp/`（gitignore 済み、スクリプトは `umask 077`）に置き、
  共有・issue 添付・チャット貼り付けもしない。
- 解析が終わったら **削除する**。再解析のために保管が必要な場合だけ、**暗号化して保管**する
  （例: OS の暗号化ボリューム、`age` / `gpg` で暗号化して鍵を分離）。鍵素材を含むファイルを
  平文のままバックアップ・クラウド同期に置かない。
- Android: `adb bugreport` から btsnoop だけを `.tmp/` に取り出し、**bugreport zip と
  その他の展開物は破棄**する（btsnoop 以外は証跡に使わない）。

**共有用の抜粋（資格情報を含めない）**:

- 抜粋は **ATT の対象 characteristic 周辺だけ**にする。切り出し条件の例:
  - Wireshark: `btl2cap.cid == 0x0004 && btatt.handle == <target handle>`
    （ATT の CID は `0x0004`、SMP は `0x0006` なので SMP フレームは入らない）
  - `tshark -r cap.snoop -Y 'btl2cap.cid == 0x0004 && btatt.handle == <target handle>' -T fields ...`
- **SMP / ペアリング / 暗号化フレームは抜粋に含めない**。`Security Manager` / `SMP`、
  `LE Start Encryption`、`LE Long Term Key Request/Reply`、`LE Enable Encryption`、
  `Identity Information` / `Identity Resolving Key` 等の HCI イベント / コマンド、
  SMP の opcode を含むフレームは抽出対象から除外する。ATT CID 条件で原則除外できるが、
  抽出後に一度確認する。
- 抜粋に残すのは **対象 GATT の write / notification と、その前後の ATT のみ**。広告・
  スキャン応答・他デバイスとの接続・HCI コマンド全般は残さない。
- **共有前の自動チェック（必須）**: 抜粋を作ったあと、次を実行して確認する。

```sh
# 0) ATT の対象 handle だけで抜粋を作る（SMP は CID 0x0006 なので入らない）
tshark -r cap.snoop \
  -Y 'btl2cap.cid == 0x0004 && btatt.handle == <target handle>' \
  -w excerpt.pcapng

# 1) SMP / 非 ATT が混ざっていないこと（出力が空であること）
tshark -r excerpt.pcapng -Y 'btsmp || btl2cap.cid != 0x0004' \
  -T fields -e frame.number

# 2) 暗号化関連の HCI イベント / コマンドが無いこと（出力が空であること）
#    LE Encryption Change (0x08) / LE Enable Encryption (0x2019) /
#    LE Long Term Key Request (0x3e) / Key Request Reply (0x201a)
tshark -r excerpt.pcapng \
  -Y 'bthci_evt.code == 0x08 || bthci_evt.code == 0x3e || bthci_cmd.code == 0x2019 || bthci_cmd.code == 0x201a' \
  -T fields -e frame.number

# 3) 対象 characteristic の ATT が入っていること（0 件なら失敗）
tshark -r excerpt.pcapng -Y 'btatt.handle == <target handle>' \
  -T fields -e frame.number | head
```

- 1) / 2) が空でない場合、または 3) が空の場合は**共有しない**（抽出条件を見直す）。
  フィールド名は Wireshark のバージョンで異なる場合があるため、値列でも目視確認する。
- 共有・コミットするのは「判定表 + SHA-256 + 伏字済み・SMP 除外済みの抜粋 + 抽出条件
  （フレーム番号範囲とフィルタ）」のみ。
- 伏字は対象コントローラだけでなく、capture 内の **すべての** BD_ADDR / 広告名に適用する。
- シリアル / 個体識別子も除く（`WriteAPPID` は type 1 では送らないが、ペイロードに個体情報が
  混ざる可能性があるため値は最小限にする）。

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
   **Erase チェックポイント（`write_firmware_with_checkpoint` +
   `OtaCheckpoint::AfterFirstErase`）を置き、初回 Erase の異常で即中止する**という条件でのみ
   実施する（§8.3）。**解禁条件は §8.3 の 8 条件すべて**（bench API + `allow_unverified_wire` +
   人間チェックポイントは最低限の要約）。
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
- enterBoot 後は App Mode に自動復帰しない（**Phase A は Jump を送らない**）。`readVersionInBoot`
  の応答後 1〜2 秒で btmon を停止し、採取終了後に次の順で復帰させる:
  1. `"8bt"` から切断する
  2. コントローラの電源を切って再投入する
  3. `eightbitdo scan --check` で `"80ND"` が戻ることを確認する（exit 0 = あり / 3 = なし）
- `"80ND"` が戻らない場合は、まず scan と再接続を試す（[SESSION_OPERATIONS.md](SESSION_OPERATIONS.md)
  §2）。scan で `80ND` が見えない場合だけ、Profile を約 3 秒押して App Mode に入り直す。
  物理操作は最後の手段とし、推測で手順を作らない。
- `"8bt"` に接続したまま放置しない（バッテリー消費・誤操作防止）。

### 8.3 Phase B（初回書込）の前提と解禁条件

Phase B は **未確定 wire に対する最初のフラッシュ書込**である。次の **8 条件をすべて**満たし、
§8.1 の不変条件も満たす場合のみ実施する（「bench API + `allow_unverified_wire` + 人間
チェックポイント」は**最低限の要約**であり、これだけでは解禁しない）。

**解禁条件**

1. **`ota-bench` API が main に統合済みの reviewed SHA にある**。`impl-ble-ota` の
   `8077287` は**暫定**であり、レビューで API 名・シグネチャが変わり得る。**main 統合後の
   reviewed SHA の rustdoc を正**とし、`eightbitdo-core/ota-bench` feature を有効にした
   ハーネスで `OtaUpdate::write_firmware_with_checkpoint` / `OtaCheckpoint` /
   `OtaBenchOutcome` が存在することを確認する（**feature 名 `ota-bench` を再定義しない**）。
2. (d) 公式アプリ capture を先に試し、「不可」の理由（どの段階で止まったか、どのコマンドが
   wire に出なかったか）を [OTA_BLE_EVIDENCE.md](OTA_BLE_EVIDENCE.md) に記録済み（§2.2）。
3. Phase A が完了し、RX フレーミング・read 応答・`"8bt"` の MTU・characteristic が
   EVIDENCE に記録済み（§8.2）。**Phase A で書込 wire は確定しない**ことを前提に置く。
4. **復旧可能な専用個体**（通常使用していないコントローラ）と、可能なら検証専用ホストで
   行う。通常使用中の個体・ホストでは行わない。
5. **公式ファームウェアを検証済み**: 配信元・size・SHA-256・`load_base` / `image_len` /
   セクション数を確認し、書込前に `expected_load_base` が `.dat +0x04` と一致すること
   （不一致なら Erase 前に `OtaInvalidFirmware` で停止する）。
6. **`allow_unverified_wire = true` を一時ハーネスでのみ明示**する（非キャンセル / no retry /
   `OtaWriteState` ログ / `OtaBenchOutcome` ログ / `tmux`・`setsid` 等で端末から切り離す）。
   **この設定をコミットしない。**
7. **人間チェックポイント**: 最初の Erase ACK を受けた時点で停止し、人間が btmon で
   `offset` / `total_len` / ACK 形状を確認する。timeout では誤受理 ACK・誤アドレスを
   検知できないため省略しない。判定の合格条件は「実行手順」に列挙する。
8. **`require_completion_ack` の値を決めて EVIDENCE に記録する**。Phase A / (d) の結果から
   決め、決められない場合は既定（off）で進める（§7.3）。**セッション 2 の最初の Erase も
   未確定 wire であり、認証済みの ACK 形状ではない**ことに注意する。

上記 8 条件に加えて**ユーザーの承認**を要する。

**実行手順（セッション最小構成は 2 セッション）**

1. **セッション 1: `AfterFirstErase`**
   - `OtaUpdate::write_firmware_with_checkpoint(.., OtaCheckpoint::AfterFirstErase)`。
     **最初の 1 セクタの Erase のみ**を行い、ACK 後に `OtaPhase::BenchStopped` で停止する
     （**全消去ではない**）。
   - 人間が次を確認し、**続行可否を判断**する。初回 Phase B では「確定済み wire との一致」は
     成立しないため、次の**初回合格条件**で判定する:
     1. 応答の `cmd` / `subcmd` が期待どおり（`cmd=0x0004` envelope の `subcmd=0x0004`、
        または `cmd=0x0004` 直エコー）
     2. `0x000E` などのエラー通知がない
     3. **要求**の `offset` が `.dat +0x04` の `load_base` + `0*0x1000` と一致し、
        `total_len = 0x1000` である
     4. ACK 後に一定時間（例: 1〜2 秒）**次のフレームが来ない**（重複 ACK・追加フレームなし）
     5. リンクが維持されている（切断なし）
   - 判断は EVIDENCE 13.6 に記録する（ACK 形状 / `offset`・`total_len` / 続行可否 /
     判断者・根拠）。ACK 側の `offset` / `total_len` は要求と一致するか、0 か、未確定の値かを
     そのまま記録する。
   - **このチェックポイントは ACK 形状と要求値の確認であり、アドレス規約（絶対 vs イメージ
     相対）の正しさを保証しない**。誤アドレスを消去するリスクは残り、**専用個体とベンダ復旧で
     しか吸収できない**（§8.4 (i)）。
   - 停止時点で **先頭 1 セクタだけが消去された状態**になる。電源断・放置で App 領域が
     部分的に消えた状態が残るリスクを認識し、次セッションまで時間を置かない。
2. **セッション 2: 通常の full run**
   - 新しい `OtaUpdate`（`Idle`）で、**`"80ND"` 接続 → enterBoot `0x000C` → `"8bt"` 再接続 →
     `prepare_boot` → 通常の `write_firmware`** の順で実行し完走する。**先頭から全消去 →
     全チャンク書込 → `WriteVersion` → `WriteFirmwareVersion` → `JumpApp`**。これで残りの
     全 wire が記録でき、`"80ND"` 復帰と version で成功確認する。
   - **セッション 2 の最初の Erase も未確定 wire で実行される**（セッション 1 は形状確認だけ
     でアドレス規約を確定しない）。セッション 2 の Erase ACK も同様に監視し、異常なら即中止
     する（§8.4 (i)）。
   - `OtaCheckpoint::AfterWrites` / `AfterWriteVersion` は次の**追加条件**を満たす場合だけ
     挟む: `AfterWrites` は Write ACK の形状・数・フレーム長を切り分ける必要がある場合、
     `AfterWriteVersion` は WriteVersion ACK / payload を切り分ける必要がある場合。どちらも
     1 セッション = 全消去・再書込が増えるため、バッテリー・時間・書込回数のコストを許容できる
     場合に限る。
3. **各セッションの前に**バッテリー残量（**50% 未満なら中止**）と `"8bt"` の広告を再確認する。
   **セッション 1 以外は先頭から全消去・再書込**であり、**レジュームできない**（API doc
   どおり停止後は新しい `OtaUpdate` + full erase/rewrite）。

**2 セッションの capture 運用**

- セッション 1 は **`AfterFirstErase` の ACK を確認したら btmon を停止**する。ファイルは
  **形状確認専用**（完了証跡にしない）とし、btsnoop magic / enterBoot / readPID /
  readVersion / Erase 要求 1 件 + ACK / Write が無いこと / SHA-256 / フレーム番号を記録する。
- セッション 1 と 2 の間は**コントローラを電源再投入して App Mode に戻す**。`"80ND"` が
  広告されるまで `scan --check` で待つ（**最大 5 分**）。5 分待っても出ない場合は
  [SESSION_OPERATIONS.md](SESSION_OPERATIONS.md) §2 の手順（scan 確認 → Profile 約 3 秒押し）に
  従い、それでも戻らなければ中止する（推測で操作を繰り返さない）。
- **セッション 2 の btmon は enterBoot より前（可能なら App Mode 接続前）から開始**する。
- **完全性検証（§6.4）はセッション 2 の capture を対象**にする。セッション 1 は形状確認用
  として SHA-256 / フレーム番号のみを記録する。
- EVIDENCE 13.1 / 13.5 は **capture ごとに複数行**で記録する（session 1 / 2 を区別）。

**計画的停止と異常停止の分岐（§8.4 (i) との関係）**

- **計画的継続**: セッション 1 の Erase ACK が**上記の初回合格条件を満たし**（`cmd` /
  `subcmd` が期待どおり、エラーなし、要求 `offset` / `total_len` が一致、追加フレームなし、
  リンク維持）、人間が「続行可」と判断した場合。
  → セッション 2 の**新しい `OtaUpdate` による先頭からの全消去・再書込**へ進む。
  これは「部分記録の復旧」ではなく**計画されたベンチ継続**であり、§8.4 (i) の
  「自前で全消去・再書込しない」対象ではない。**ただし合格条件は ACK 形状と要求値の確認で
  あり、アドレス規約の正しさは保証しない**（残存リスクは専用個体＋ベンダ復旧で吸収）。
- **異常停止**: ACK 不明 / タイムアウト / 形状不一致 / アドレス疑い / 人間が「不可」と判断。
  → その場で中止し、**§8.4 (i) のベンダ復旧**へ。自前の全消去・再書込は行わない。
- 判断と根拠（フレーム番号・値・判断者）は EVIDENCE 13.6 に記録する。

**「wire 確定」の定義**: (d) 公式 capture または Phase B が**最後まで完走**し、Erase /
Write / WriteVersion / WriteFirmwareVersion / Jump の**全 wire が記録済み**になって初めて
確定。セッション 1 だけ、または Erase だけ成功して Write で失敗した状態は確定ではない
（§8.4 (ii)）。

### 8.4 失敗時の復旧（失敗段階ごとの分岐）

**共通**: 再試行しない。最後の段階（`OtaWriteState::Untouched` / `EraseStarted` /
`PartiallyWritten` / `Written`）と capture を保存し、btmon は復旧が終わるまで止めない。

**(i) 最初の Erase で異常、または wire が部分記録のまま中止**

- **計画的チェックポイント停止はここに含めない**。§8.3 のセッション 1 で Erase ACK が
  期待どおり（`offset` / `total_len` / 形状が一致し、アドレス疑いなし）で、人間が「続行可」と
  判断した場合は、**§8.3 のセッション 2（新しい `OtaUpdate` + 先頭からの全消去・再書込）へ
  進む**。これは計画されたベンチ継続であり、下記の「自前で全消去・再書込しない」対象では
  ない。
- **異常停止はその場で中止**する。同じ Erase も Write も再送しない。
- **異常な部分記録は確定ではない**。ACK 不明 / タイムアウト / 形状不一致 / アドレス疑い /
  人間が「不可」と判断 / 途中で切断 / capture が途中で切れた等の場合は、自前の全消去・
  再書込で復旧しない（wire 未確定のまま書込を重ねない）。
- **ベンダ復旧**へ移る: 公式アプリ / 公式サポートの復旧手順に従う。本リポジトリにはベンダ復旧手順が
  無いため、推測で代替しない。コントローラの状態（`"8bt"` / `"80ND"` のどちらが広告されるか）と
  capture、最後に送ったフレームを記録して報告する。**App Mode が正常に動く場合は (iii) を
  優先する**（動いている個体に無理な復旧をしない）。

**(ii) wire 完全確定後の中断（全コマンドが記録済みで、途中で失敗 / 切断）**

- **wire 完全確定の条件**: (d) 公式 capture または自前 Phase B が**最後まで完走**し、
  Erase / Write / WriteVersion / WriteFirmwareVersion / Jump の**全 wire が記録済み**であること。
  EVIDENCE に ACK 形状・アドレス規約・フレーム長が記録され、同じ設定で再現できる状態を指す。
  **1 つでも欠ける場合は確定ではない**（計画的なチェックポイント停止は §8.3 の継続手順、
  異常な部分記録は (i) として扱う）。
- `"8bt"` に再接続 → `readPID`（`0x0008`）→ `.dat` と `expected_load_base` を再確認 →
  **最初のセクタから全消去 → 全チャンク書込 → `WriteVersion` → `WriteFirmwareVersion` →
  `JumpApp`** を、新しい `OtaUpdate`（`Idle`）で `prepare_boot` からやり直す。

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
- [ ] Phase B の場合: `ota-bench` API が **main 統合済みの reviewed SHA** にあり、
      feature 有効で `write_firmware_with_checkpoint` / `OtaCheckpoint` / `OtaBenchOutcome` が
      存在する（既定ビルドに無いことも確認）
- [ ] Phase B の場合: (d) 公式アプリ capture を試し、不可の理由を EVIDENCE に記録した
- [ ] Phase B の場合: Phase A の判定が EVIDENCE に記録済み
- [ ] Phase B の場合: 復旧可能な専用個体・検証専用ホストで行う
- [ ] Phase B の場合: 公式 `.dat` の size / SHA-256 / `load_base` / `image_len` /
      セクション数を確認した
- [ ] Phase B の場合: `allow_unverified_wire=true` は一時ハーネスのみで、コミットしない
- [ ] Phase B の場合: `require_completion_ack` の値を Phase A / (d) の結果から決め、
      EVIDENCE に記録した
- [ ] Phase B の場合: ハーネスが非キャンセル / no retry / `OtaWriteState`・
      `OtaBenchOutcome` ログを満たす
- [ ] Phase B の場合: ハーネスを `tmux` / `setsid` 等で端末から切り離して実行する
- [ ] Phase B の場合: ユーザーの承認がある
- [ ] Phase B の場合: **各セッション前に**バッテリー残量（**50% 未満なら中止**）と
      `"8bt"` 広告を再確認した
- [ ] Phase B の場合: セッション 1 は `AfterFirstErase` で**最初の 1 セクタ Erase のみ**
      （全消去ではない）を行い、**初回合格条件**（cmd/subcmd・エラーなし・要求
      `offset`/`total_len`・追加フレームなし・リンク維持）で人間が**続行可否**を判断し、
      13.6 に記録した
- [ ] Phase B の場合: セッション 1 の btmon を停止し、capture は**形状確認専用**として
      SHA-256 / フレーム番号だけを記録した（完全性検証の対象にしない）
- [ ] Phase B の場合: セッション 1→2 の間に電源再投入し、`scan --check` で `"80ND"` 復帰を
      待った（**最大 5 分**。戻らなければ SESSION_OPERATIONS §2 → それでも不可なら中止）
- [ ] Phase B の場合: セッション 2 の btmon を **enterBoot 前（可能なら App 接続前）**から
      開始した
- [ ] Phase B の場合: セッション 2 は `"80ND"` 接続 → enterBoot → `"8bt"` 再接続 →
      `prepare_boot` → 通常の `write_firmware` を完走した（**2 セッション最小**。
      `AfterWrites` / `AfterWriteVersion` は切り分け条件を満たす場合のみ）
- [ ] Phase B の場合: セッション 2 の最初の Erase も**未確定 wire**として監視し、異常なら
      即中止した
- [ ] Phase B の場合: **完全性検証（§6.4）はセッション 2 の capture を対象**にした
- [ ] Phase B の場合: EVIDENCE 13.1 / 13.5 を capture ごと（session 1 / 2）に複数行で記録した
- [ ] Phase B の場合: 異常時（ACK 不明 / タイムアウト / 形状不一致 / アドレス疑い / 人間が
      不可）は中止してベンダ復旧へ移る（自前の全消去・再書込をしない）

### 8.6 実施後チェックリスト

**共通**:

- [ ] btmon を停止し、capture の size / **SHA-256（必須）** / 使用アダプタ / 切り出し範囲を
      記録した
- [ ] capture の完全性（正常終了 / btsnoop magic / 最終フレーム / 期待フレーム数）を
      確認した（§6.4）
- [ ] capture が **INVALID**（スクリプト exit 3・空・magic 不正・SIGKILL 停止）でないことを
      確認した
- [ ] 判定表を [OTA_BLE_EVIDENCE.md](OTA_BLE_EVIDENCE.md) に追記した
- [ ] capture 生ファイルをコミットしていない（`.tmp/` のみ）
- [ ] 抜粋から SMP / ペアリング / 暗号化フレームを除外し、抽出条件を記録した（§7.4）
- [ ] 共有前の自動チェック（`btl2cap.cid == 0x0004` 以外 0 件 / SMP 0 件 / 暗号化イベント
      0 件）を実行し、共有してよいことを確認した（§7.4）
- [ ] 抜粋に含めた**すべての** BD_ADDR / 広告名（周囲の他機器を含む）を伏字にした
- [ ] capture 生ファイルを削除した（保管する場合は暗号化した）

**Phase A**:

- [ ] `"8bt"` から切断した
- [ ] コントローラの電源を切って再投入し、`scan --check` で `"80ND"` 復帰を確認した
- [ ] 復帰しない場合は [SESSION_OPERATIONS.md](SESSION_OPERATIONS.md) §2 の手順
      （scan 確認 → Profile 約 3 秒押し）に従った

**Phase B**:

- [ ] App Mode `"80ND"` へ復帰し、`ReadVersion` でバージョンを確認した
- [ ] config / マクロを読み戻し、バックアップと SHA-256 を比較した

---

## 9. 残課題

1. **(d) 公式アプリ capture の可否判定（未試行）**: 公式アプリで OTA を完遂できるか、HCI
   スヌープログに boot 遷移から Jump まで残るかを最初に確認する。不可なら「どの段階で止まったか」
   を EVIDENCE に記録する（§2.2）。
2. **`ota-bench` 段階実行 API（`8077287` 時点の暫定・main 統合待ち）**: `impl-ble-ota`
   （head `8077287`）で `eightbitdo-core/ota-bench` feature、
   `OtaUpdate::write_firmware_with_checkpoint`、`OtaCheckpoint`、`OtaBenchOutcome`、
   `OtaPhase::BenchStopped` が実装された。plan は実 API 名に更新済みだが、**レビューで
   API が変わり得るため main 統合後の reviewed SHA の rustdoc を正**とする。**main へ
   統合されたら**、Phase B を §8.3 の 8 条件（＋ユーザー承認）で解禁する。**main 未統合の
   間は Phase B を実施しない。**
3. **駆動手段が無い**: CLI / GUI / UniFFI に OTA が未配線。Phase A から一時ハーネスか
   配線済みフロントエンドが必要（§2.2）。ハーネスはコミットしない。
4. **公式 `.dat` の入手・検証**: 配信 API のレスポンス（`md5` のクセを含む）と、
   ダウンロードしたファイルの SHA-256 / セクション数の記録方法を決める。
5. **マルチセクション `.dat` の blocker**: 現実装は単一セクションのみ。公式 `.dat` が
   マルチセクションなら更新できない（PROTOCOL_SPEC §8.5 12）。実測して対応方針を決める。
6. **ACK の `offset` / `total_len` 照合**: 採取後に実装する（PROTOCOL_SPEC §8.5 13）。遅延 ACK の
   誤帰属を防ぐ必須項目。
7. **`load_base_addr` とフラッシュサイズの確定**: `UT2_TYPE1_APP_BASE_MIN` /
   `UT2_TYPE1_APP_FLASH_END` の暫定ウィンドウを実測値に置き換える。
8. **capture の保管・共有ポリシー**: BD_ADDR（対象外の他機器を含む）と Android bugreport の
   PII 範囲を踏まえ、共有範囲を決める（§7.4）。
9. **Android / Windows 経路**: 現状は採取も駆動もできない。配線後に別 plan で扱う。

---

## 10. 参照

- [PROTOCOL_SPEC.md](PROTOCOL_SPEC.md) §8（BLE OTA 仕様・§8.5 未検証事項）
- [OTA_BLE_EVIDENCE.md](OTA_BLE_EVIDENCE.md)（Android v2.01 の静的解析証跡）
- [SESSION_OPERATIONS.md](SESSION_OPERATIONS.md)（BLE 単一所有・押下手順）
- [crates/eightbitdo-core/src/ota.rs](../crates/eightbitdo-core/src/ota.rs)（状態機械と安全ガード）
- [crates/eightbitdo-protocol/src/commands.rs](../crates/eightbitdo-protocol/src/commands.rs)（`boot` builder）
- [scripts/ota/capture-btsnoop.sh](../scripts/ota/capture-btsnoop.sh)（`btmon` 採取ヘルパー）
- [AGENTS.md](../AGENTS.md) §4（OTA の安全性）・§6（現在の状態）
