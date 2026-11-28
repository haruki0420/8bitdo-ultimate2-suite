# 公式機能カバレッジ・マトリクス（8BitDo Ultimate 2 Bluetooth / `0x600F`）

- 基準: `main` **`ed61961`** 以降のドキュメント更新（**2026-11-29 Windows USB / 2.4G 実機採取反映**）。
- 対象: **Ultimate 2 Bluetooth Controller**（内部 PID `0x600F` / 2.4G レシーバー `0x6011` [`0x310B`] / USB `057E:2009`→`2DC8:600F`）。
- 比較対象: 公式 Windows 版 `8BitDo_Ultimate_Software_V2_Windows_V1.38`（`8BitDoAdvance.dll` + 実機接続時の EXE UI スクショ 18 枚 + USBPcap 7 本 + `v2.01` `.dat`）と公式 Android 版 `V2.01`（`classes.dex` + `libadvancelib.so`）の解析。
- **実機検証**: Padbench 自体の実機検証は **Linux（BlueZ）・BLE `"80ND"`・MTU 200・74B chunk のみ**。Windows 公式ソフト（V1.38）による USB 有線 / 2.4G 実機パケット・UI 挙動は 2026-11-29 の USBPcap で確認済み。macOS / Android / iOS 実機および Padbench 本体の USB/Windows 実行は未検証。
- この文書は、ローカル RE メモ `.tmp/features/FEATURE_MATRIX.md` / `.tmp/features/OFFICIAL_FEATURE_RE_EVIDENCE.md`（`.tmp/` は `.gitignore` 済みで未コミット）の 2026-11-22 時点の「リマップ未実装」等の記述を、現在の main の実装に合わせて更新したものである。証拠ラベルの定義はローカル RE メモを踏襲する。
- **RE 未確認のビット・レンジ・セマンティクスを「実装済み」「実機確認済み」と書かない**。実装されているのは「RE で確定した範囲」または「実機で観測した値集合」に限られる。

## 0. ラベル定義

### 0.1 RE 証拠ラベル（`.tmp/features/FEATURE_MATRIX.md` §0 を踏襲）

| ラベル | 意味 |
| --- | --- |
| **RE 確認済み** | 公式バイナリの UI 文言と（DLL export / JNI 関数 / 構造体フィールド）の**両方**を確認した |
| **推測** | 上記の**片方のみ**（UI 文言だけ、または関数・フィールドだけ） |
| **未確認** | 公式バイナリ由来の証拠が無い、または詳細 RE 待ち |
| **Web 情報のみ** | 公式製品ページ・レビュー記事のみで、バイナリ証拠が無い |

RE の一次証拠（DLL export の RVA、DEX の code_off、構造体オフセット等）は `.tmp/features/OFFICIAL_FEATURE_RE_EVIDENCE.md` に記録されている。本リポジトリには、コミット可能な形で [docs/PROTOCOL_SPEC.md](PROTOCOL_SPEC.md) と [docs/STRUCT_BT2_2768B.md](STRUCT_BT2_2768B.md) に要点を反映済み。

### 0.2 実装状況ラベル

| ラベル | 意味（main `c169392` 時点） |
| --- | --- |
| **実装済み（core + CLI + GUI）** | 型付き編集（`ConfigEdit`）と書き込み経路が core にあり、CLI と GUI の両方から実行できる |
| **実装済み（CLI のみ）** | core + CLI に経路があるが、GUI に露出していない |
| **実装済み（GUI のみ）** | core + GUI に経路があるが、CLI の型付きコマンドに無い（raw `config write` のバイト編集は別途可能） |
| **実装済み（core のみ）** | ライブラリ API はあるが、CLI / GUI に配線されていない（未配線） |
| **未実装** | コード経路が無い |
| **未確認（RE 証拠不足）** | 対象かどうか、またはプロトコル上の格納先が未確定で、実装判断ができない |

「実装済み」は**書き込みが安全機構（必須 pre-write backup、CRC 再計算、予約領域保持、Save、read-back 検証、TOCTOU 再読込）を通る**ことを含む。実装済みでも実機検証は別欄で判定する。

### 0.3 実機検証ラベル

| ラベル | 意味 |
| --- | --- |
| **実機確認済み（Linux BLE）** | Linux・BLE・MTU 200 で実行し、結果（read-back 一致等）を記録済み |
| **実機未検証** | コード経路はあるが、実機で実行していない |
| **実機評価不能** | 下位の RE が未確定で、実機に送る値を決められない |

---

## 1. サマリ

| 機能領域 | 公式ソフトでの提供 | RE 証拠 | 実装状況 | 実機（Linux BLE） | 主な残作業 |
| --- | --- | --- | --- | --- | --- |
| ボタンリマップ | Mapping タブ（Front / Back、22 物理ボタン） | **RE 確認済み** | **実装済み（core + CLI + GUI）** | **実機確認済み**（BLE `remap` 書込、`flag` と背面 `PL/PR/L4/R4` 入力ビットを USB で確定。Switch 実効は未） | 22 index の Switch 側実効観測 |
| スティック | Dead zone（0..100）、軸反転・入替（※カーブ・キャリブレーションなし） | **RE 確認済み** | 実装済み（core + CLI + GUI） | Y 軸符号（`0=上/255=下`）は USB 実機確定。デッドゾーン効果は未検証 | 実機でのデッドゾーン効果確認 |
| トリガー | L/R Trigger の範囲（1..100）、`Swap L2 and R2` | **RE 確認済み**（デジタル化専用フィールドは無し） | 実装済み（core + CLI + GUI） | `L2`/`R2` 入力ビットと `+0x09`/`+0x08` は USB 実機確定 | 実機での範囲効果確認、ヘアトリガーの扱い確定 |
| 振動 | L/R Vibration（0〜5 段階）、Hold L2/R2 テスト案内 | **RE 確認済み** | 実装済み（core + CLI + GUI）。振動テスト UI は未実装 | 実機未検証 | 実機での強度確認、テスト UI |
| ジャイロ / 6 軸 | **公式 Windows `V1.38` UI では `0x600F` 接続時非表示（N/A）** | **UI 非表示確定**（構造体と DLL export のみ存在） | 実装済み（core + CLI + GUI） | 実機未検証（UI 非露出） | Switch 実機で構造体設定が効くかの確認 |
| LED / RGB | `Default`（`Close`/`Light-tracing`/`Fire Ring` 等）と `Custom`（`Sync left/right RGB` + 24 LED）。**Brightness スライダなし** | **UI 実画面・DLL・USBPcap 確認済み**（`0x0040`/`0x0041` 確定。一部効果名は未操作） | 実装済み（core + CLI + GUI）だが 3 テーマ書込のみ（`0x0040`/`0x0041` 未実装） | `0x0040`/`0x0041` は Windows USB 実機確認済み | `0x0040`/`0x0041` 実装、他効果パラメータ対応、実機発光確認 |
| プロファイル | 3 スロット、Create / Move・Load 1..3 / Rename / Copy / Deactivate / Delete | **実機 USBPcap 確定**（`slot_flags`: `0x20200911` 有効 / `0` 削除 / `0xFFFFFFFF` 未初期化） | 名前・`gamepad_mode`・`cur_slot` は実装済み（core + CLI + GUI）。`0x0015` は **core + CLI + REPL 配線済み**（Save なし）。copy/delete UI は未 | `0x0015` BLE 1 回観測済み。Windows の Delete/Deactivate/Load は 2768B 全量書込+Save であることを USB 実機確認 | `0x0015` 永続性・実効、copy/delete UI |
| ターボ / 連射 | Turbo / AutoTurbo / Turbo_3 の割当（速度 UI なし確認） | 定数と書込経路は **RE 確認済み**。速度・段階のセマンティクスは **未確認** | リマップ先として実装済み（core + CLI + GUI）。専用 UI・レート設定は未実装 | 実機未検証 | レート / 3 段階の意味確認 |
| マクロ | 記録・編集・削除（Clear）・`Single`・`Use uniform interval time [30] ms` | **実機 USBPcap 確定**（Clear は `0x0104` なしで 52B ヘッダ 0 クリア+Save、USB 書込は 32B 分割） | 記録・編集・書込・復元を実装（core + CLI + GUI）。空マクロ作成・削除は未実装 | **実機確認済み**（BLE S1 M1/M2、Windows USB/2.4G で S1 M3・S2 M1 書込と Clear を観測） | M4・S3、Switch 発動確認、Clear 実装 |
| ファームウェア更新 | バージョン表示、**USB 直結必須の OTA**（2.4G は USB 要求モーダル） | **USB Boot TX/RX と 2 セクション `.dat` を USBPcap 確定**。BLE OTA は Android 非対応・未確認 | バージョン読取は実装済み。BLE OTA は **core のみ**。USB OTA は未実装 | **Windows USB OTA（`v2.00→v2.01`, `[0x81,0x05]`）実機確認済み**。BLE OTA は未確認 | 2 セクション `.dat` 対応 + USB OTA 実装（Phase 4） |
| backup / restore | 明示的な UI は未確認。相当は Profile Copy / Delete | Copy / Delete は **RE 確認済み** | pre-write backup・config / macro backup・config / macro restore は実装済み（CLI）。GUI は backup のみ | backup は実機確認済み | GUI restore、JSON/TOML 入出力 |
| USB / 2.4G | 公式 Windows の主経路（有線 `0x600F` / 2.4G `0x310B`） | **App Mode・2.4G・USB Boot TX すべて実機 USBPcap 確定** | **未実装（保留解除・設計可能）** | 公式ソフトの USB / 2.4G 動作を USBPcap 確認済み（Padbench 未実装） | `hidapi` による Phase 2e / 4 実装 |
| バッテリー / ポーリング / モード / キャリブレーション | 2.4G/USB の接続アイコン区別（USB 有線時はバッテリー表示なし） | バッテリー生値 `+0x0A` は USB で `94..96` 観測。Polling は `0x600F` 非対応（確定）。**キャリブレーション UI なし確認（N/A）** | バッテリー生値表示のみ。他は未実装 | USB 有線生値 `94..96` を観測（UI 非表示のためスケール未確定） | ワイヤレス時の残量スケール確定、モード書換実機確認 |
| キーボードキー割当 | 他デバイス向け UI 文字列のみ | **未確認**（`0x600F` 実機キャプチャにも登場せず） | 未実装 | 実機評価不能 | 必要なら対象外（N/A）としてクローズ |

---

## 2. ボタンリマップ

| 項目 | 公式ソフトでの内容 | RE 証拠 | 実装状況 | 実機（Linux BLE） | 残作業 |
| --- | --- | --- | --- | --- | --- |
| 22 ボタンのリマップ | `Mapping` タブ、`Front` / `Back` / `Top` ビュー。割当先を選択（`N` = 無効） | **RE 確認済み**: `MapKeyRecord.key[22]`（`+0x0E4 + slot*0x5C + idx*4`、各 `u32`）、Windows `_writeUltimateBT2MapKey`、Android `getBT2_WriteMapKey`、DEX の 22 index 順 | **実装済み（core + CLI + GUI）**。`Session::write_full_config` 経由で 2768B を書き、`ConfigEdit::ButtonRemap` が該当 4B だけを変更する。CLI `config remap` / `config map show`、GUI 設定タブの 22 ボタン | **実機確認済み（書込と read-back のみ）**: `config remap A B` で `key[0]` が `A→B`、続けて `B→A` に復帰。**押したボタンの出力が実際に変わるか（実効）は未確認** | 実機の押下→入力レポート / Switch 側挙動の観測、`flag` の意味確定 |
| L4 / R4 / PL / PR | 物理ボタン。`Mapping` に `Front` / `Back` タブあり | **RE・USBPcap 確定**: index 18 = P1 = **PL** (`0x0200_0000`), 19 = P2 = **PR** (`0x0400_0000`), 20 = P3 = **L4** (`0x0020_0000`), 21 = P4 = **R4** (`0x4000_0000`) | 22 index の一部として実装済み（CLI は `P1`/`PL`、`P2`/`PR`、`P3`/`L4`、`P4`/`R4` の別名を受理） | 実機ダンプ割当＋**2026-11-29 USB 実機単独押下で全 4 ビット一致を確認** | 前面 UI の割当先一覧に出すかの仕様固定 |
| 無効化（`N` / Null） | `Null` 選択で無効化 | **RE 確認済み**: 無効値は `KeyMap_N = 0`。`KeyMap_My_NUll = 227` は書込・表示経路から参照されず用途未確認 | 実装済み。CLI `null` / `disable`、GUI のコンボボックスで選択可能。227 は拒否 | 実機未検証（remap 経路としては実機確認済み） | 227 の用途は追加 RE。書かない方針を維持 |
| 特殊機能（軸反転・スティック入替・L2/R2 入替・D-pad 入替等） | `Invert X/Y-Axis`、`Swap the left and right joysticks` 等 | **RE 確認済み**（UI 文言 + `_writeUltimateBT2Special` + `SpecialRecord.feature` ビット表）。一部ビット（joyShare 等）は **推測** | **実装済み（GUI のみ）**。GUI 設定タブが 13 ビットのチェックボックスを提供。**CLI の型付き `config set` には特殊機能が無い**（raw `config write` のバイト編集でのみ可能） | 実機未検証 | CLI への `special.*` 追加、未確定ビットの RE |
| マクロ起動キー（M1〜M4） | マクロに起動キーを割当 | **RE 確認済み**（`MacroFunPro3Record.key_map`） | 実装済み。CLI `macro write --trigger M1..M4/KeyMap`、GUI マクロエディタのトリガー選択 | 実機未検証（マクロ書込自体は M1/M2 で実機確認済み） | 実機でトリガー押下時のマクロ発動確認 |
| キーボードキー割当 | キーボード製品向けの UI 文字列（`Alphanumeric` 等）はある | **未確認**（`0x600F` の `key[22]` にキーボード値を入れる証拠なし） | 未実装。**対象かどうか自体が未確定** | 実機評価不能 | `0x600F` のリマップ値域にキーボードスキャンコードがあるか追加 RE。無ければ N/A と記録 |
| `flag`（リマップ有効フラグ） | 公式は単一エントリ書込で `flag` を変更しない。プロファイル Delete / Load 時の一括書込で切替 | **実機確定（2026-11-29）**: **`0x20200911`** = 有効、**`0x00000000`** = 削除/Deactivate、**`0xFFFFFFFF`** = 未初期化（工場出荷状態） | 実装済み: `ConfigEdit::MapKeyFlag`（typed は 0/1 のみ）。GUI は未変更の実機値を保持。raw 検証は観測値集合（`0/1/0x20200911/0xFFFFFFFF`）を受理（PR #17） | 実機意味確定（`04a_profile_ops.pcapng` で Delete→`0`、Activate→`0x20200911` を確認） | `ConfigEdit::MapKeyFlag` 等での `0x20200911` 対応整理 |

---

## 3. スティック

| 項目 | 公式ソフトでの内容 | RE 証拠 | 実装状況 | 実機（Linux BLE） | 残作業 |
| --- | --- | --- | --- | --- | --- |
| デッドゾーン（内側 / 外側） | `Stick` タブ、`L Stick` / `R Stick` (`0..100`) | **RE 確認済み**: `JoyRecord`（`+0x98 + slot*8`）、実機 Y 軸符号は **`0 = 上 / 255 = 下`** と確定（`03_usb_app_init_and_input.pcapng`） | **実装済み（core + CLI + GUI）**。CLI `config set joy.left_start` 等、GUI スライダー | Y 軸符号は USB 実機確定。デッドゾーン変化自体は未検証 | 実機でデッドゾーン変化を確認 |
| スティック感度 / カーブ | **UI なし確認（`0x600F` には存在しない）** | **N/A 確定**（スクショ `04a/04b`、`+0x1F4..+0x68C` はレガシー領域） | 対象外 | — | なし |
| スティック押込（L3/R3）の割当 | リマップ対象 | **RE 確認済み**（`KeyMap_L3` / `R3`、index 8/9） | 実装済み（リマップの一部） | remap の実機確認範囲に同じ | 押下ビット観測 |
| キャリブレーション | **UI なし確認（`0x600F` には存在しない）** | **N/A（他デバイス専用 UI）**: `Stick` タブに `Start Calibration` ボタンは存在しない（`04a/04b` 確認） | 対象外 | — | なし |

---

## 4. トリガー

| 項目 | 公式ソフトでの内容 | RE 証拠 | 実装状況 | 実機（Linux BLE） | 残作業 |
| --- | --- | --- | --- | --- | --- |
| トリガー範囲（L/R） | `Trigger` タブ、`L Trigger` / `R Trigger` | **RE 確認済み**: `TriggerRecord`（`+0xB0 + slot*8`、start / end 各 u8）、`_writeUltimateBT2Trigger`、`getBT2_WriteTrigger` | **実装済み（core + CLI + GUI）**。CLI `config set trigger.left_start` 等、GUI スライダー | 実機未検証 | 実機で範囲変化を確認 |
| ヘアトリガー | 専用文言は未発見。docs はモード時の開始値既定 `0x4D` を記録 | **推測**（構造体のデフォルト値記録のみ） | 特別扱いなし（範囲値としてのみ設定可能） | 実機評価不能 | ソフト設定か本体物理のみかを RE |
| アナログ / デジタル切替 | 本体背面のトリガーストップで物理切替 | **未確認**（プロトコル上の専用フラグは無い。デジタル化は `KeyMap` 再割当で表現） | 未実装（対象外の可能性） | 実機評価不能 | BLE から切替可能か RE。物理のみなら N/A |
| L2/R2 入れ替え | `Swap L2 and R2` / `Swap LT and RT` | **RE 確認済み**（`SpecialRecord.feature` bit7） | 実装済み（GUI の特殊機能チェックボックス）。CLI 型付きは未対応 | 実機未検証 | CLI 露出、実機確認 |

---

## 5. 振動

| 項目 | 公式ソフトでの内容 | RE 証拠 | 実装状況 | 実機（Linux BLE） | 残作業 |
| --- | --- | --- | --- | --- | --- |
| 左右モーター強度 | `L Vibration` / `R Vibration` の 0〜5 段階 | **RE 確認済み**: `RumbleRecord`（`+0x74 + slot*0x0C`、`f32` 左右、0.0〜1.0 を 0.2 刻み）、`_writeUltimateBT2Vibration`、`getBT2_WriteRumble` | **実装済み（core + CLI + GUI）**。CLI `config set rumble.level`、GUI スライダー | 実機未検証 | 実機で強度変化を確認 |
| 振動テスト | `Please Hold L2 / R2 to test the vibration` | **推測**（UI 文言のみ。専用コマンド無し） | 未実装（専用 UI 無し。`monitor` でトリガー入力値の確認は可能） | 実機評価不能 | テスト用 UI / 手順の追加 |
| 振動パターン（波形） | 無し（左右強度のみ） | **RE 確認済み**（構造体は左右 `f32` のみ） | 対象外 | — | — |
| 本体 / トリガー振動の有効化 | 専用 UI 文言は未発見 | **推測**（`SpecialRecord.feature` bit15 / bit14） | 実装済み（GUI 特殊機能。ビット名のみ RE） | 実機未検証 | UI 文言との対応 RE、実機確認 |

---

## 6. ジャイロ / 6 軸

| 項目 | 公式ソフトでの内容 | RE 証拠 | 実装状況 | 実機（Linux BLE） | 残作業 |
| --- | --- | --- | --- | --- | --- |
| 有効化方法と有効ボタン | **Windows `V1.38` UI に Motion タブなし**（`Mapping / Macros / Stick / Trigger / Vibration / RGB` の 6 タブのみ） | **UI 非表示確定**（`SixAxisRecord` `+0x92C` と `_writeUltimateBT2sixaxis` は存在） | **実装済み（core + CLI + GUI）**（CLI `gyro.*`, GUI 設定） | 実機未検証（UI 非露出） | Switch 実機で直接設定時に有効になるか確認 |
| 感度 / デッドゾーン補償 | 同上 | 同上 | 実装済み（CLI `gyro.sensitivity` / `gyro.dead_compensate`、GUI） | 実機未検証 | Switch 実機確認 |
| マッピング先 | 同上 | 同上 | 実装済み（CLI `gyro.mapping`、GUI） | 実機未検証 | Switch 実機確認 |
| Android の部分書込 JNI | — | **未確認**: Android は `writeBT2SixAxis` を呼ばず全体書込に同梱。Windows の `0x92C + slot*0x0C` が正規 | 本実装は Windows 準拠（`0x92C + slot*0x0C`、`total_len=0x0AD0`） | 実機未検証 | 実機確認 |
| 6 軸の利用制限 | — | **`0x600F` 公式 UI からは設定不可** | 実装上の制限なし | 実機評価不能 | 必要に応じて公式 UI 準拠の表示方針を検討 |

---

## 7. LED / RGB

| 項目 | 公式ソフトでの内容 | RE 証拠 | 実装状況 | 実機（Linux BLE） | 残作業 |
| --- | --- | --- | --- | --- | --- |
| Dynamic / HotWheel / Single の 3 テーマ＋即時切替 | `RGB` タブ: `Default`（`RGB Effect`: `Close` / `Light-tracing` / `Fire Ring` 等）＋ `Custom` | **実機 USBPcap・DLL・UI 確認済み**: テーマ確認 **`0x0041`**、即時切替 **`0x0040`**（`_read/writeUltimateBT2theme`、`subcmd = 0x02/0x04/0x05`）＋ 2768B 3 テーマ構造体 | 3 テーマ構造体書込のみ実装済み（core + CLI + GUI）。`0x0040`/`0x0041` は未実装 | `0x0040`/`0x0041` は Windows USB 実機確認済み | `0x0040`/`0x0041` 実装、実機発光確認 |
| 24 LED 個別色 | `Custom` タブ、`Sync left/right RGB`、24 LED 選択＋マルチ選択（Ctrl） | **RE 確認済み**（`Single_Theme.colors[24]`、`0xAARRGGBB` ＋ スクショ `07b`） | 実装済み（同色一括）。個別色 UI 未実装 | 実機未検証 | 24 LED 個別編集 UI、`Sync left/right RGB` の保存先確認 |
| エフェクト速度 | `Speed`（`HotWheel_Theme.speed`） | **RE 確認済み** | 実装済み（CLI `led.hotwheel.speed`、GUI） | 実機未検証 | 実機確認 |
| 明るさ | **UI なし確認（`0x600F` の `RGB` タブに独立スライダなし）** | **N/A（他デバイス用文言）** | 対象外（ARGB 色表現に含む） | — | なし |
| ライティング効果名の対応 | `Default` ドロップダウン: `Close` (`0x05` 確認)、`Light-tracing`、`Fire Ring` ほか（スクロールバーあり） | **一部確認**: `Close = 0x05` ほか `0x02`/`0x04` を USBPcap 観測 | 未実装（3 テーマ数値編集のみ） | `0x0040` で `0x02/0x04/0x05` を USB 実機観測 | ドロップダウン下部の効果名一覧と ID 対応表の補完 |
| `Sync left/right RGB` | `Custom` タブのチェックボックス | **RE 確認済み**（スクショ `07b`） | 未実装 | 実機未検証 | チェック状態が UI 補助のみか 2768B 保存かの確認 |
| Windows Dynamic Lighting 連携 / Lighting OFF | `Close` 選択で消灯。独立した Dynamic Lighting UI は `0x600F` に見当たらない | `Close` は `0x0040` / `flag=0` 相当 | テーマ `flag=0` で個別に無効化可能 | — | 対象外扱い整理 |

---

## 8. プロファイル

| 項目 | 公式ソフトでの内容 | RE 証拠 | 実装状況 | 実機（Linux BLE） | 残作業 |
| --- | --- | --- | --- | --- | --- |
| 3 スロットの名前変更 | `Profiles` 画面、Rename | **RE 確認済み**（`file_names` `+0x14 + slot*0x20`、UTF-16BE 32B） | **実装済み（core + CLI + GUI）** | 実機未検証 | 実機確認 |
| 有効プロファイル（`cur_slot`）と Deactivate / Load | `Current Profile` (Slot 1..3) と `Profile List`（`Move to Profile 1..3` / `Deactivate` / `Load to Profile 1..3`） | **実機 USBPcap 確定（`04a` + スクショ `10a..10c`）**: Deactivate/Load 時は **2768B 全量書込（`0x0001`）＋ Save（`0x0006/0x0123`）** を実行 | `ConfigEdit::CurSlot`（2768B 書換）＋ `0x0015` 配線済み | USB で全量書込フロー確定 | BLE での `cur_slot` 変更実機確認 |
| プロファイル切替コマンド `0x0015` | `Load to Profile 1..3` 等 | **RE 確認済み**（`cmd=0x0015, subcmd=slot`）。Windows USB の Load/Deactivate は 2768B 全量書込を使用 | **core + CLI + REPL 配線済み**（Save なし）。GUI 専用 UI は未 | BLE で ACK 1 回・`cur_slot` 更新確認。永続性・実効未確認 | 永続性・実効確認、GUI 切替 UI |
| Copy / Delete | `Copy`、`Delete`（メニュー確認 `10b/10c`） | **実機 USBPcap 確定（`04a`）**: Delete は **`0x0001` 全量書込＋Save** で対象スロット `slot_flags`・名前・全レコード `flag` を **`0x00000000`** クリア（領域 B も `00000000 01 64 01 64` へ更新） | **protocol ヘルパーのみ**（`Bt2Config::copy_slot` / `delete_slot`、未配線） | Windows USB で Delete/Activate の実バイト差分確定 | `ConfigEdit` 化と CLI / GUI 露出、領域 B の `[1, 100, 1, 100]` 整合確認 |
| `slot_flags` の意味 | プロファイル有効状態 | **実機確定（2026-11-29）**: **`0x20200911`** = 有効、**`0x00000000`** = 削除/無効、**`0xFFFFFFFF`** = 未初期化 | 読取・保持と raw 観測値集合チェック実装済み | **意味確定済み（`04a_profile_ops.pcapng`）** | `ConfigEdit` / raw での扱い整理 |
| 本体 Profile ボタン / 専用 UI | `Profiles` 管理画面 | **RE 確認済み** | 部分実装（名前・`cur_slot` のみ） | CLI `config profile` のみ BLE 確認 | Copy / Delete / Move / Load UI |

---

## 9. ターボ / 自動連射

| 項目 | 公式ソフトでの内容 | RE 証拠 | 実装状況 | 実機（Linux BLE） | 残作業 |
| --- | --- | --- | --- | --- | --- |
| ターボの割当 | マッピング先として `Turbo`、`AutoTurbo` 画像、Android `hold` / `burst` | **RE 確認済み**（`KeyMap_Turbo` = bit16、`Turbo_Auto` = bit24、`Turbo_3` = bit23。書込は通常のリマップと同じ 4B） | **実装済み（リマップ先として）**: CLI `config remap A turbo` 等、GUI のコンボボックスに `Turbo` / `Turbo_Auto` / `Turbo_3` を表示 | 実機未検証 | 実機で連射動作を確認 |
| 連射レート / 3 段階の意味 | `Turbo_3`（3 段階?）、`AutoTurbo` | **未確認**（構造体にレート・段階の専用フィールドが無い。FW 固定の可能性） | 未実装（割当のみ。レート設定 UI は無い） | 実機評価不能 | セマンティクスの追加 RE、専用 UI の要否判断 |
| ターボ専用 UI | 専用の設定画面 | **推測**（UI 文言・リソースのみ） | 未実装（リマップ UI 内の選択肢としてのみ提供） | 実機評価不能 | 専用 UI が必要かの判断（レートが RE できてから） |

---

## 10. マクロ

| 項目 | 公式ソフトでの内容 | RE 証拠 | 実装状況 | 実機（Linux BLE） | 残作業 |
| --- | --- | --- | --- | --- | --- |
| マクロの記録 / 編集 / 保存 | `Macros` タブ、`Single` ドロップダウン、`Use uniform interval time [30] ms`、`Start` / `Clear` / `Save` | **RE + USBPcap 確定**: Windows USB/2.4G でも `total_len = (macro_idx<<12) + align32(steps*10)`（S1 M3・S2 M1 実機観測）、`interval_ms = 30`、`0x0104` `tlen=0, clen=0x1000` | **実装済み（core + CLI + GUI）**。CLI `macro backup/write/restore`、GUI マクロエディタ | **実機確認済み**: BLE S1 M1/M2 ＋ **Windows USB/2.4G S1 M3・S2 M1**。Switch 側発動は未確認 | M4・S3、Switch 発動確認、GUI の JSON/TOML I/O |
| マクロの削除（Clear） / 新規作成 | `Clear` / 空マクロへの新規作成 | **実機 USBPcap 確定（`04a`/`04d`）**: **削除時は `0x0104` を発行せず**、対象 52B ヘッダを 0 クリアして 2768B 全量書込 (`0x0001`) ＋ Save (`0x0123`) のみ実行 | 空マクロ作成・削除は未実装（GUI / CLI とも空マクロ書込を拒否） | **Windows USB/2.4G で Clear 手順を実機観測** | `macro clear` / 空スロット作成手順の実装 |
| 実行モード | `Run Once`、`Hold to Run`、`Click Start/Stop`、`Click to Replay` 等 | **未確認**（`0x600F` UI には `Single` ドロップダウンと均一インターバル枠のみ表示） | 部分実装: `cycles_num` / `interval_ms` は数値として編集・書込可 | 実機未検証 | ドロップダウン選択肢と構造体の対応整理 |
| ループ / ラウンドモード | `Single` 等、`Use uniform interval time` | **一部確定**: `Use uniform interval time` が `+0x30` `interval_ms` であることを `04a/04d` で確認 | 部分実装（`interval_ms` / `cycles_num` のみ） | Windows 確認済み（`interval_ms = 30`） | `cycles_num` 変更時の全ドロップダウン比較 |
| マクロの復元 | — | **RE 確認済み** | 実装済み（CLI `macro restore`。GUI は復元 UI 無し） | 実機確認済み | GUI 復元 UI |
| ヘッダ `total_len` のプラットフォーム差 | — | **整理済み**: Windows は全量書込 (`0x0AD0`) でヘッダ更新、BLE 部分書込は `0xD8` | BLE 部分書込の `0xD8` を採用（BLE 実機で永続化確認） | BLE / USB 双方で永続化確認 | 完了 |

---

## 11. ファームウェア更新（OTA）

| 項目 | 公式ソフトでの内容 | RE 証拠 | 実装状況 | 実機（Linux BLE） | 残作業 |
| --- | --- | --- | --- | --- | --- |
| バージョン表示 | `Current Firmware Version` | **RE 確認済み**（BLE `0x000B`、USB `0x0100`） | 実装済み（BLE `0x000B`） | **実機確認済み**（BLE `0xC8`、USB `0xC8→0xC9`） | USB `0x0100` 配線（Phase 2e） |
| BLE OTA | Android APK はデッドコード（非対応） | コマンド列のみ残骸あり（`enterBoot` 等 0 呼び出し元）。**公式 2.4G UI も `"Please connect via USB"` を強制** | **core のみ実装・未配線**（`allow_unverified_wire = false`） | **実機評価不能**（BLE OTA 非対応の可能性が高い） | BLE OTA を廃止し USB OTA へ一本化するかの設計判断 |
| ファームウェア取得 API | `POST /firmware/select?firmware_type=..&beta=..&soft_type=5` | **RE 確認済み** | 未実装 | — | USB OTA 実装時に配線 |
| **USB OTA（`v2.00 → v2.01`）** | USB 直結専用の `Update`（`09b` スクショ） | **実機 USBPcap + `.dat` 完全確定（2026-11-29）**: `[0x81,0x05]`/`[0x02,0x05]`、最大 46B チャンク、2 セクション `.dat`（`0x6010` + `0x600F`、`0x01018000..0x01038C00`）、`0x00C3/0x0004/0x0003/0x00C1/0x00C4(8B)/0x0007(8B)` | **未実装（Phase 4 ブロッカー解消・設計可能）** | **Windows 公式 USB 更新（`v2.00→v2.01`）を USBPcap で全往復実測済み** | `hidapi` + 2 セクション `.dat` パーサ + Phase 4 実装 |
| OTA 画面（GUI） | バージョン・過去版選択・進捗・USB 接続案内 | スクショ `09a/09b` 確認済み | 未実装（Phase 3c 未着手） | — | Phase 4 USB OTA と合わせて Tauri GUI へ実装 |

---

## 12. backup / restore

| 項目 | 公式ソフトでの内容 | RE 証拠 | 実装状況 | 実機（Linux BLE） | 残作業 |
| --- | --- | --- | --- | --- | --- |
| 明示的な backup / restore UI | 公式ソフトに「backup / restore」という明示 UI は確認されていない。相当は Profile の Copy / Delete とマクロ管理 | **未確認**（公式 UI に backup 文言が無い）。Copy / Delete は **RE 確認済み** | 本リポジトリ独自の安全機能として実装済み（下記） | — | — |
| pre-write backup（書込前の自動バックアップ） | 公式相当なし（本リポジトリの安全機構） | — | **実装済み**: config / macro の全書込経路が、書込前に `config.bin` + `config.json`（+ マクロ）をディレクトリへ保存。TOCTOU 用 SHA-256 も記録 | 実機確認済み（backup ディレクトリ生成と SHA 記録） | — |
| config の backup / restore | — | — | 実装済み（CLI）: `backup --raw` / `config dump --raw`、復元は `config write --file`（`--accept-reserved-drift` / `--force-raw` は復旧用）。GUI は backup ボタンのみ | 実機確認済み（dump / full write の read-back 一致） | GUI からの config restore UI |
| マクロの backup / restore | マクロ管理（記録・保存） | **RE 確認済み** | 実装済み（CLI）: `macro backup` / `macro restore`（3 slot × 4 macro、JSON / TOML、pre-restore backup）。GUI はマクロ書込時の pre-write backup のみで、復元 UI は無い | 実機確認済み（復元後に config SHA が元に一致） | GUI 復元 UI |
| GUI の backup / restore | — | — | GUI は **backup のみ実装**（サイドバーの Backup ボタン）。restore は CLI を案内 | backup は実機未検証（GUI 書込全体が未検証） | GUI restore、JSON/TOML 入出力 |

---

## 13. その他（バッテリー / ポーリング / モード / キャリブレーション / 接続経路）

| 項目 | 公式ソフトでの内容 | RE 証拠 | 実装状況 | 実機（Linux BLE） | 残作業 |
| --- | --- | --- | --- | --- | --- |
| バッテリー表示 | Home 画面：USB/2.4G アイコン区別（USB 有線時はバッテリー表示なし） | 入力レポート `+0x0A`（USB 有線実機で `94..96` [`0x5E..0x60`] 観測、スケールは未確定） | 部分実装: CLI `monitor` と GUI 入力タブが生値を表示 | BLE / USB 双方で生値観測（スケール未確定） | ワイヤレス使用時の残量変化で `0..100` スケール確定 |
| ポーリングレート | `Polling Rate(Hz)` | **RE 確定**: `0x600F` 非対応 | 対象外（N/A） | — | なし（0x600F では提供しない） |
| モード切替（Switch / XInput 等） | `Home` に `Switch` / `Windows` カード（`SelectPlatform`） | `gamepad_mode` `+0x10` の読取は **RE 確認済み**。USB 列挙切替（`057E:2009` → `2DC8:600F`）は USBPcap 確定 | 実装済み: `ConfigEdit::GamepadMode`。書換で実機動作が切り替わるかは未検証 | 書換実効は未検証 | 書換の実機確認 |
| キャリブレーション | **UI なし確認（`0x600F` には存在しない）** | **N/A（`04a/04b` スクショでボタン非表示確認）** | 対象外 | — | なし |
| USB HID / 2.4G トランスポート | 公式 Windows の主経路（有線 `0x600F` / 2.4G `0x310B`） | **2026-11-29 USBPcap 確定**: USB App Mode（`057E:2009`→`2DC8:600F`、45B/32B チャンク）、2.4G（`0x310B` `EP 0x03/0x83`）、USB Boot TX（`[0x81,0x05]` 46B） | **未実装（Phase 2e / 4 着手可能）** | 公式 Windows 経路の実機 USBPcap 取得済み（Padbench 未実装） | `hidapi` による USB / 2.4G transport 実装 |
| Pairing tool / Shake-to-Wake | 別アプリ / FW 機能 | **Web 情報のみ** | 対象外 | — | スコープ外と判断（必要なら別ツール） |
| ソフト共通設定（言語・背景・更新確認等） | アプリ側設定 | — | GUI に日本語 / 英語切替のみ | — | 任意 |

---

## 14. コード上の根拠（分類の裏取り）

- **書き込み基盤**: `crates/eightbitdo-core/src/config_ops.rs`
  - `ConfigEdit` の型付き編集: `GamepadMode` / `CurSlot` / `ProfileName` / `Rumble` / `Joy` / `Trigger` / `Special` / `SixAxis` / `DynamicTheme` / `HotWheelTheme` / `SingleTheme` / `ButtonRemap` / `MapKeyFlag`
  - 全量 2768B を `min(MTU, 74)` バイトの `0x0001` で送り、BLE Save（`0x0006` / `subcmd 0x0067`）→ read-back 検証。予約領域（`0x1F4..0x68C`、`0x914..0x92C`）はバイト単位で保持。失敗は fail-closed で stage / Save 状態 / 復旧案内を返す
  - **部分書込（公式の 4B remap 等）は使わず、常に全量書込 + Save + read-back**。`ButtonRemap` は該当 4B だけを変える `ConfigEdit` として実装
- **CLI**: `crates/eightbitdo-cli/src/main.rs`
  - トップレベル: `scan` / `info` / `config dump|write|set|remap|map show` / `macro backup|write|restore` / `backup` / `monitor` / `keepalive` / `session`
  - `config set` の型付きフィールドは `cur_slot, gamepad_mode, profile_name, rumble.level, joy.*, trigger.*, gyro.*, led.*, map_key.flag`。**`special.*`（軸反転等）と profile copy/delete は CLI に無い。`0x0015` 切替は `config profile <N>` にある**
  - `session` REPL は `--allow-config-write` / `--allow-macro-write` + `--yes` または `yes` 入力が必須
- **GUI**: `crates/eightbitdo-gui/src/`（egui）
  - 6 タブ: Device / Input / Config（読み取り）/ Settings（書き込み）/ Macro / Log
  - Settings はプロファイル名・`gamepad_mode`・Active Profile・振動・スティック・トリガー・特殊機能 13 ビット・ジャイロ・3 テーマ・22 ボタンリマップを編集。確認ダイアログ、pre-write backup、read-back、TOCTOU
  - Macro は既存の非空マクロの読み出し・録音・ステップ編集・書込（空マクロ作成は拒否）。**OTA 画面・GUI restore は無い**
  - **GUI の書込は実機未検証（Tier 3）**
- **モバイル / UniFFI**: `crates/eightbitdo-uniffi/src/lib.rs`
  - 公開は **読み取り中心**（version / config / macro backup / input / `describe_config`）と、**書込のローカル検証**（`prepare_config_write` は CRC 再計算と差分返却のみで送信しない）。フラッシュ書込・OTA は未公開
- **OTA**: `crates/eightbitdo-core/src/ota.rs`
  - `Idle → EnteringBoot → AwaitingBootReconnect → PreparingBoot → Ready → Erasing / Writing / Finalizing → Complete / Failed`
  - `allow_unverified_wire` は既定 `false` で、未確認 wire ではフラッシュフレームを 1 つも送らない。`ota-bench`（既定 OFF）で checkpoint 停止が可能
  - 既知の blocker: ACK の `offset` / `total_len` 照合が未実装、マルチセクション `.dat` 非対応（2026-11-29 `.dat` 実測で `0x6010` + `0x600F` 2 セクション・`0x01018000..0x01038C00` を確認）、Bootloader MTU 未確認
- **raw flag の実機対応（PR #17）**: 各レコードの `flag` は実機観測値集合（`0` / `1` / `0x20200911` / `0xFFFFFFFF`、gyro は `0x20190000` も）のみ受理し、それ以外は `--force-raw`。`slot_flags` も同じ観測値集合。型付き `config set` の flag は 0/1 のみ。詳細は [DEVICE_CONFIG_EVIDENCE.md](DEVICE_CONFIG_EVIDENCE.md)

---

## 15. 残作業（優先順）

1. **GUI 書込（設定 / リマップ / マクロ）の実機確認**（Tier 3）。pre-write backup と read-back を前提に、Linux BLE・MTU 200 で確認する。
2. **USB HID トランスポート（Phase 2e）と USB OTA（Phase 4）の実装**: 2026-11-29 の USBPcap で USB App Mode（`057E:2009`→`2DC8:600F`, 45B/32B チャンク）・2.4G（`0x310B`）・USB Boot TX（`[0x81,0x05]`, 46B チャンク）・2 セクション `.dat`（`0x6010` + `0x600F`, `0x01018000..0x01038C00`）がすべて確定した。`eightbitdo-protocol` に 2 セクション `.dat` パーサと `0x00C1` / `0x0040` / `0x0041` / `0x0014` / `0x0100` を追加し、`hidapi` 経由で実装する（BLE OTA については公式非対応の可能性が高いため方針整理）。
3. **プロファイル管理とマクロ削除（Clear）の完成**: `slot_flags`（`0x20200911`/`0`/`0xFFFFFFFF`）が確定したため、`Copy / Delete / Deactivate / Load` とマクロ `Clear`（52B ヘッダ 0 クリア + Save）の `ConfigEdit` 化および CLI / GUI（Tauri 版）露出を行う。`0x0015` は再接続・電源断永続性と実効の確認が残る。
4. **RGB 即時切替（`0x0040` / `0x0041`）と Tauri GUI（P2 以降）の実装**: `0x600F` 公式 UI に準拠し（Motion / Calibration / Brightness なし）、`0x0040` / `0x0041` と Tauri BLE 接続・書込・USB OTA 画面を実装する。
5. **BLE 側での残実機確認**: `config set` の各フィールド、MTU 200 以外、REPL 書込、M3/M4・S2/S3、Switch 実機でのマクロ発動・リマップ実効、バッテリー残量の `0..100` スケール確認。
6. **モバイル & クロスプラットフォーム実機**: Android 実機 JNI ロード、iOS 実機、macOS / Windows 実機 BLE。
7. **対象外 / 保留の整理**: `0x600F` で公式 UI 非表示が確定した Motion / Calibration / スティック感度カーブ / Brightness（独立スライダ）および未確認のキーボードキー割当・ターボレート等のクローズ判定。

---

## 16. 情報源

- リポジトリ内: [PROTOCOL_SPEC.md](PROTOCOL_SPEC.md)、[STRUCT_BT2_2768B.md](STRUCT_BT2_2768B.md)、[DEVICE_CONFIG_EVIDENCE.md](DEVICE_CONFIG_EVIDENCE.md)、[IMPLEMENTATION_PLAN.md](IMPLEMENTATION_PLAN.md)、[OTA_BLE_EVIDENCE.md](OTA_BLE_EVIDENCE.md)、[OTA_VERIFICATION_PLAN.md](OTA_VERIFICATION_PLAN.md)、[SESSION_OPERATIONS.md](SESSION_OPERATIONS.md)、[CROSS_PLATFORM.md](CROSS_PLATFORM.md)
- コード: `crates/eightbitdo-protocol/src/`、`crates/eightbitdo-core/src/{config_ops,session,macro_ops,ota}.rs`、`crates/eightbitdo-cli/src/main.rs`、`crates/eightbitdo-gui/src/`
- 公式バイナリの RE 一次証跡: `.tmp/features/OFFICIAL_FEATURE_RE_EVIDENCE.md`（DLL export RVA、DEX code_off、構造体オフセット）、`.tmp/features/FEATURE_MATRIX.md`（2026-11-22 時点の旧マトリクス）。**`.tmp/` は `.gitignore` 済みで、公式バイナリはコミットしない**
- 公式製品ページ / ソフトページ / レビュー記事は旧マトリクス §6 を参照（Web 情報のみの項目は本文でその旨を明記）

## 17. 更新時の注意

- この表の「実装状況」は **main のコミット済みコード**を基準にする。作業ツリーの未コミット実装を「実装済み」と書かない。
- **RE 未確認のビット・レンジ・セマンティクスを「実装済み」「実機確認済み」と書かない**。実装は「RE 確定値」または「実機観測値集合」に限る。
- 実機検証の追記は、対象（Linux BLE / 他 OS・他経路）、MTU、chunk、read-back の有無、SHA-256 をセットで記録する。
- 公式ソフトは多数の 8BitDo 製品を 1 つの EXE で扱う。`0x600F` 向けでない UI 文字列（Polling Rate、Lift-off Distance、Volume 等）は「他デバイス向け」と明記する。
- 表を更新したら、[README.md](../README.md) のドキュメント一覧と、必要に応じて [IMPLEMENTATION_PLAN.md](IMPLEMENTATION_PLAN.md) §5 / [AGENTS.md](../AGENTS.md) §6 の状態記述との整合を確認する。
