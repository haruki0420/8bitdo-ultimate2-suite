# 公式機能カバレッジ・マトリクス（8BitDo Ultimate 2 Bluetooth / `0x600F`）

- 基準: `main` **`c169392`**（2026-11-23、PR #17 反映後）。本ドキュメントはブランチ `docs/feature-matrix` で作成。
- 対象: **Ultimate 2 Bluetooth Controller**（内部 PID `0x600F` / 2.4G レシーバー `0x6011`）。
- 比較対象: 公式 Windows 版 `8BitDo_Ultimate_Software_V2_Windows_V1.38`（`8BitDoAdvance.dll` + EXE の UI 文言）と公式 Android 版 `V2.01`（`classes.dex` + `libadvancelib.so`）の静的解析（RE）。
- **実機検証は Linux（BlueZ）・BLE `"80ND"`・MTU 200・74B chunk のみ**。macOS / Windows / Android / iOS の実機、USB / 2.4G 経路の実機は未検証。
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
| ボタンリマップ | Mapping タブ（Front / Back / Top）。22 物理ボタン | **RE 確認済み** | **実装済み（core + CLI + GUI）** | **実機確認済み**（`config remap A B` の書込・read-back のみ。押下時の動作変化は未確認） | 22 index の実押下ビット観測、`flag` の意味確定 |
| スティック | Dead zone（内側 / 外側） | **RE 確認済み** | 実装済み（core + CLI + GUI） | 実機未検証 | 実機でのデッドゾーン効果確認 |
| トリガー | L/R Trigger の範囲 | **RE 確認済み**（デジタル化専用フィールドは無し） | 実装済み（core + CLI + GUI） | 実機未検証 | 実機での範囲効果確認、ヘアトリガーの扱い確定 |
| 振動 | L/R Vibration（0〜5 段階）、振動テスト | **RE 確認済み** | 実装済み（core + CLI + GUI）。振動テスト UI は未実装 | 実機未検証 | 実機での強度確認、テスト UI |
| ジャイロ / 6 軸 | Motion（Hold/Toggle、有効ボタン、感度、補償、マップ先） | **RE 確認済み**（Windows 経路）。Android は部分書込 JNI 未使用 | 実装済み（core + CLI + GUI） | 実機未検証 | Windows UI が `0x600F` で有効かの確定、実機確認 |
| LED / RGB | Brightness、RGB Effect、24 LED、Speed、Dynamic Lighting | 3 テーマ・24 LED・Speed は **RE 確認済み**。Brightness・効果名は **未確認 / 推測** | 実装済み（core + CLI + GUI）だが 3 テーマのみ | 実機未検証 | Brightness と効果名の格納先 RE、実機確認 |
| プロファイル | 3 スロット、名前、切替、Copy / Delete | 名前・`cur_slot`・`0x0015`・copy/delete は **RE 確認済み**。`slot_flags` は **未確認** | 名前・`gamepad_mode`・`cur_slot` は実装済み（core + CLI + GUI）。`0x0015` は **core + CLI に配線済み**（Save 要否・ACK 形状は未確認）。copy/delete は **protocol のみ・未配線** | `0x0015` は実機未検証（Save 要否・永続性は未確認）。GUI の切替 UI は未実装 | `0x0015` の実機確認、copy/delete UI、`slot_flags` 確定 |
| ターボ / 連射 | Turbo / AutoTurbo / Turbo_3 の割当 | 定数と書込経路は **RE 確認済み**。速度・段階のセマンティクスは **未確認** | リマップ先として実装済み（core + CLI + GUI）。専用 UI・レート設定は未実装 | 実機未検証 | レート / 3 段階の RE、専用 UI |
| マクロ | 記録・編集・実行モード・ループ | ヘッダ / ステップは **RE 確認済み**。実行モード / ラウンドモードは **未確認** | 記録・編集・書込・復元を実装（core + CLI + GUI）。空マクロ作成は未実装 | **実機確認済み**（slot S1 の M1/M2 の書込・復元。Switch 側の発動は未確認） | M3/M4・S2/S3、実行モード RE、空マクロ作成 |
| ファームウェア更新 | バージョン表示、OTA、ベータ版 | コマンドと UI は **RE 確認済み**。wire 詳細は **未確認** | バージョン読取は実装済み。OTA は **core のみ・未配線** | バージョン読取は実機確認済み。OTA は実機評価不能 | 実機 btsnoop で wire 確定、マルチセクション `.dat`、配線 |
| backup / restore | 明示的な UI は未確認。相当は Profile Copy / Delete | Copy / Delete は **RE 確認済み** | pre-write backup・config / macro backup・config / macro restore は実装済み（CLI）。GUI は backup のみ | backup は実機確認済み | GUI restore、JSON/TOML 入出力 |
| USB / 2.4G | 公式 Windows の主経路 | USB App Mode は **推測**、USB Boot TX は **未確認** | **未実装（保留）** | 実機評価不能 | USB App Mode の実機証跡、USB Boot TX の RE |
| バッテリー / ポーリング / モード / キャリブレーション | バッテリー画像、Polling Rate、モード、Calibration | バッテリー生値は RE 確認済み（スケール未確認）。Polling は **0x600F 非対応（確定）**。モード書換・キャリブレーションは **未確認 / 推測** | バッテリー生値表示のみ。他は未実装 | バッテリー生値表示は実機確認済み（スケール未確認） | スケール / 充電状態の RE、モード書換可否、キャリブレーション RE |
| キーボードキー割当 | キーボード製品向けの UI 文字列はある | **未確認**（`0x600F` 向けの証拠なし） | 未実装（対象か未確定） | 実機評価不能 | `0x600F` の `key[22]` にキーボード値が入るか RE |

---

## 2. ボタンリマップ

| 項目 | 公式ソフトでの内容 | RE 証拠 | 実装状況 | 実機（Linux BLE） | 残作業 |
| --- | --- | --- | --- | --- | --- |
| 22 ボタンのリマップ | `Mapping` タブ、`Front` / `Back` / `Top` ビュー。割当先を選択（`N` = 無効） | **RE 確認済み**: `MapKeyRecord.key[22]`（`+0x0E4 + slot*0x5C + idx*4`、各 `u32`）、Windows `_writeUltimateBT2MapKey`、Android `getBT2_WriteMapKey`、DEX の 22 index 順 | **実装済み（core + CLI + GUI）**。`Session::write_full_config` 経由で 2768B を書き、`ConfigEdit::ButtonRemap` が該当 4B だけを変更する。CLI `config remap` / `config map show`、GUI 設定タブの 22 ボタン | **実機確認済み（書込と read-back のみ）**: `config remap A B` で `key[0]` が `A→B`、続けて `B→A` に復帰。**押したボタンの出力が実際に変わるか（実効）は未確認** | 実機の押下→入力レポート / Switch 側挙動の観測、`flag` の意味確定 |
| L4 / R4 / PL / PR | 物理ボタン。Windows には `settingmapping_L4/R4/PL/PR` の画像名あり | **RE 確定**: index 18 = P1 = **PL**、19 = P2 = **PR**、20 = P3 = **L4**、21 = P4 = **R4**（DEX `MappingImageEngineer.getMotionKeyString`）。Android の前面割当先一覧には出さない | 22 index の一部として実装済み（CLI は `P1`/`PL`、`P2`/`PR`、`P3`/`L4`、`P4`/`R4` の別名を受理） | 実機ダンプで slot 0 の `key[18]/key[19]`（P1/P2）に A/B の factory 割当を確認済み（[DEVICE_CONFIG_EVIDENCE.md](DEVICE_CONFIG_EVIDENCE.md) §3）。**押下→入力ビットの観測記録は無い** | 押下ビット観測、前面 UI の割当先一覧に出すか（Android 準拠で出さない）の仕様固定 |
| 無効化（`N` / Null） | `Null` 選択で無効化 | **RE 確認済み**: 無効値は `KeyMap_N = 0`。`KeyMap_My_NUll = 227` は書込・表示経路から参照されず用途未確認 | 実装済み。CLI `null` / `disable`、GUI のコンボボックスで選択可能。227 は拒否 | 実機未検証（remap 経路としては実機確認済み） | 227 の用途は追加 RE。書かない方針を維持 |
| 特殊機能（軸反転・スティック入替・L2/R2 入替・D-pad 入替等） | `Invert X/Y-Axis`、`Swap the left and right joysticks` 等 | **RE 確認済み**（UI 文言 + `_writeUltimateBT2Special` + `SpecialRecord.feature` ビット表）。一部ビット（joyShare 等）は **推測** | **実装済み（GUI のみ）**。GUI 設定タブが 13 ビットのチェックボックスを提供。**CLI の型付き `config set` には特殊機能が無い**（raw `config write` のバイト編集でのみ可能） | 実機未検証 | CLI への `special.*` 追加、未確定ビットの RE |
| マクロ起動キー（M1〜M4） | マクロに起動キーを割当 | **RE 確認済み**（`MacroFunPro3Record.key_map`） | 実装済み。CLI `macro write --trigger M1..M4/KeyMap`、GUI マクロエディタのトリガー選択 | 実機未検証（マクロ書込自体は M1/M2 で実機確認済み） | 実機でトリガー押下時のマクロ発動確認 |
| キーボードキー割当 | キーボード製品向けの UI 文字列（`Alphanumeric` 等）はある | **未確認**（`0x600F` の `key[22]` にキーボード値を入れる証拠なし） | 未実装。**対象かどうか自体が未確定** | 実機評価不能 | `0x600F` のリマップ値域にキーボードスキャンコードがあるか追加 RE。無ければ N/A と記録 |
| `flag`（リマップ有効フラグ） | 公式は単一エントリ書込で `flag` を変更しない。全 22 + flag の一括コピーで設定 | **未確認**（静的解析は 0/1。実機は slot 0/1 = `0x20200911`、slot 2 = `0xFFFFFFFF` で意味不明） | 実装済み: `ConfigEdit::MapKeyFlag`（typed は 0/1 のみ）。GUI はユーザーが触った時だけ書き、未変更の実機値は保持。raw 検証は実機観測値集合を受理（PR #17） | 実機値の観測済み（意味は未確定） | 実機で `flag` とリマップ有効性の関係を確定 |

---

## 3. スティック

| 項目 | 公式ソフトでの内容 | RE 証拠 | 実装状況 | 実機（Linux BLE） | 残作業 |
| --- | --- | --- | --- | --- | --- |
| デッドゾーン（内側 / 外側） | `Stick` タブ、`Dead zone:`。左右それぞれ start / end | **RE 確認済み**: `JoyRecord`（`+0x98 + slot*8`、`left_start_value` 0..127 / `left_end_value` 0..128、右も同様）、`_writeUltimateBT2Sticks`、`getBT2_WriteJoyStick` | **実装済み（core + CLI + GUI）**。CLI `config set joy.left_start` 等（型付きは `flag=1` を同時に設定）、GUI スライダー | 実機未検証（`config set` 各フィールドは未検証） | 実機でデッドゾーン変化を確認 |
| スティック感度 / カーブ | 専用 UI 文言は未発見 | **未確認**（構造体に対応フィールドが無く、`+0x1F4..+0x68C` の用途不明領域の可能性） | 未実装 | 実機評価不能 | `+0x1F4..+0x68C` の追加 RE |
| スティック押込（L3/R3）の割当 | リマップ対象 | **RE 確認済み**（`KeyMap_L3` / `R3`、index 8/9） | 実装済み（リマップの一部） | remap の実機確認範囲に同じ | 押下ビット観測 |
| キャリブレーション | `Start Calibration` / `Recalibrate` 等 | **推測**（UI 文言のみ。専用フィールド・BT2 用 export 無し） | 未実装 | 実機評価不能 | 保存先・コマンドの追加 RE |

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
| 有効化方法と有効ボタン | `Motion`、`Enable method`（Hold / Toggle）、`Enable button` | **RE 確認済み**: `SixAxisRecord`（`+0x92C + slot*0x0C`、`trigger_mode` 0=Hold / 1=Toggle、`key_map`）、Windows `_writeUltimateBT2sixaxis` | **実装済み（core + CLI + GUI）**。CLI `config set gyro.trigger_mode` / `gyro.key`、GUI | 実機未検証 | 実機でジャイロ動作を確認 |
| 感度 / デッドゾーン補償 | `Sensivity`（原文綴り）、`Dead zone compensation` | **RE 確認済み**（`sensitivity` 1..15、`dead_compensate` 0..255） | 実装済み（CLI `gyro.sensitivity` / `gyro.dead_compensate`、GUI） | 実機未検証 | 実機確認 |
| マッピング先 | `Map motion sensing to:`（Left Stick / Right Stick / Mouse） | **RE 確認済み**（`mapping_type` 0/1/2） | 実装済み（CLI `gyro.mapping`、GUI） | 実機未検証 | 実機確認 |
| Android の部分書込 JNI | — | **未確認**: Android は `writeBT2SixAxis` を呼ばず、全体書込に同梱する可能性が残る。Windows の offset が正規 | 本実装は Windows 準拠（`0x92C + slot*0x0C`、`total_len=0x0AD0`）。Android の `offset=0` バグは踏襲しない | 実機未検証 | 実機 BLE で Windows 準拠 offset が通ることを確認 |
| 6 軸の利用制限 | `Motion control (for Switch only)` | **Web 情報のみ** | 実装上の制限なし（`gamepad_mode` 依存の挙動は未検証） | 実機評価不能 | モード別挙動の実機確認 |

---

## 7. LED / RGB

| 項目 | 公式ソフトでの内容 | RE 証拠 | 実装状況 | 実機（Linux BLE） | 残作業 |
| --- | --- | --- | --- | --- | --- |
| Dynamic / HotWheel / Single の 3 テーマ | `RGB Effect`、`Fire Ring`、`Color`、`Speed` 等 | **RE 確認済み**: `Dynamic_Theme`（`+0x950`）、`HotWheel_Theme`（`+0x974`、`speed` 0..15）、`Single_Theme`（`+0x9A4`、24×ARGB）、各 Windows export / Android JNI | **実装済み（core + CLI + GUI）**。CLI `config set led.dynamic.*` / `led.hotwheel.*` / `led.single.*`、GUI | 実機未検証 | 実機で発光変化を確認 |
| 24 LED 個別色 | `Select Custom Color`、パレット | **RE 確認済み**（`Single_Theme.colors[24]`、ARGB `0xAARRGGBB`） | 実装済み。CLI は `led.single.color` で 24 LED を同色に設定（個別色は raw 編集）。GUI も同色一括 | 実機未検証 | 24 LED 個別編集 UI、左右対応の実機確認 |
| エフェクト速度 | `Speed` | **RE 確認済み**（`HotWheel_Theme.speed` 0..15） | 実装済み（CLI `led.hotwheel.speed`、GUI） | 実機未検証 | 実機確認 |
| 明るさ | `Brightness` | **未確認**（2768B 内に独立フィールドが見当たらない。ARGB の Alpha 等で表現の可能性） | 未実装（明るさ専用の編集は無い） | 実機評価不能 | 格納先の追加 RE |
| ライティング効果名の対応 | `Star`、`Echo Press`、`Starlight`、`Cycle`、`Color Wave`、`Ripple`、`Breathing`、`Flow` 等 | **推測**（UI 文言は豊富だが、3 テーマ構造体との対応が不明） | 未実装（3 テーマの値編集のみ） | 実機評価不能 | 効果名 ↔ テーマ / パラメータの RE |
| `Sync left/right RGB` | 左右同期 | **未確認** | 未実装 | 実機評価不能 | 格納先の RE |
| Windows Dynamic Lighting 連携 | `Configure the lighting in Windows Dynamic Lighting` | **推測**（UI 文言のみ。HID LampArray は未文書化） | 未実装（対象外の可能性） | 実機評価不能 | HID LampArray 対応可否の RE |
| ライティング OFF | `Turn Off Lighting` | **未確認**（他デバイス向けの可能性） | 未実装（テーマ `flag=0` で個別に無効化は可能） | 実機評価不能 | 対象デバイスの確定 |

---

## 8. プロファイル

| 項目 | 公式ソフトでの内容 | RE 証拠 | 実装状況 | 実機（Linux BLE） | 残作業 |
| --- | --- | --- | --- | --- | --- |
| 3 スロットの名前変更 | `Profile name`、`Rename` | **RE 確認済み**（`file_names` `+0x14 + slot*0x20`、UTF-16BE 32B、`_WriteUltimateBT2Name`） | **実装済み（core + CLI + GUI）**。CLI `config set profile_name`、GUI テキスト入力 | 実機未検証 | 実機確認（名前の UTF-16BE 往復） |
| 有効プロファイル（`cur_slot`） | `Current Profile`、`Activate` | **RE 確認済み**（`cur_slot` `+0x12`）。実機値は 6 など未文書値も観測 | **実装済み（core + CLI + GUI）**: `ConfigEdit::CurSlot` で 2768B の `cur_slot` フィールドを書く。CLI `config set cur_slot`、GUI の Active Profile コンボ | 実機未検証 | 実機で「フィールド書換だけで切替が起きるか」を確認 |
| プロファイル切替コマンド `0x0015` | `Load to Profile 1..3` 等 | **RE 確認済み**（`cmd=0x0015, subcmd=slot, total_len=0, offset=0`。実装 `commands::switch_slot` と `PROTOCOL_SPEC` に一致）。Save 要否・ACK 形状は **未確認** | **core + CLI 配線済み**（`Session::switch_profile`、`config profile <N>`。Save は送らない）。GUI の専用 UI は未実装 | 実機未検証（ACK 形状・Save 要否・永続性） | 実機での ACK / 永続性確認、GUI 切替 UI |
| Copy / Delete | `Copy`、`Delete this profile ?` | **RE 確認済み**（Android JNI `copyConfig` / `deleteCurslot`、UI コンポーネント） | **protocol ヘルパーのみ**（`Bt2Config::copy_slot` / `delete_slot` は存在するが CLI / GUI 未露出。raw `config write` でバイト編集は可能） | 実機未検証 | `ConfigEdit` への追加と CLI / GUI 露出、実機確認 |
| `slot_flags` の意味 | プロファイルの有効状態に関係するとみられる | **未確認**（実機は `0x20200911` / `0xFFFFFFFF` で opaque） | 読取・保持のみ（書き換えは raw 経路のみ。観測値集合を検証） | 実機値の観測済み（意味は未確定） | 意味の確定（プロファイル作成・削除との対応） |
| 本体 Profile ボタンでの切替 | `Press the Profile button on the controller...` | **RE 確認済み**（案内文言）。本体 FW の挙動 | 対象外（ファームウェア機能） | 実機未検証 | ソフトからの切替可否（`0x0015`）を実機確認 |
| プロファイル切替の専用 UI | `Load / Move / Rename / Copy / Delete` | **RE 確認済み** | **部分実装**: 名前・`cur_slot` の編集はあるが、**Copy / Delete / 専用切替 UI は無い** | 実機未検証 | Copy / Delete / `0x0015` 切替 UI |

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
| マクロの記録 / 編集 / 保存 | `Macros` タブ、`Create a new macro`、`Click "Start" to begin recording` | **RE 確認済み**: `Macro_Pro3_Struct`（`+0x68C + slot*0xD8`、4×52B）、`MacroDatas` 10B/step（最大 200）、`0x0102/0x0103/0x0104` + Save | **実装済み（core + CLI + GUI）**。CLI `macro backup/write/restore`（JSON / TOML）、GUI マクロエディタ（20ms ライブ録音、ステップ削除・待ち時間挿入・値変更、名前・トリガー・`cycles_num` / `interval_ms` 編集、書込） | **実機確認済み**: slot S1 の M1/M2 で書込・read-back・復元（config SHA 一致）。**Switch 側での発動は未確認** | M3/M4・S2/S3、Switch 発動確認、GUI の JSON/TOML I/O |
| マクロの新規作成（空マクロ） | `Create a new macro` | **未確認**（空マクロ作成の手順が未文書化） | 未実装（GUI / CLI とも空マクロ作成は拒否。既存の非空マクロへの書込のみ） | 実機評価不能 | erase + step clear の手順確定と実装 |
| 実行モード | `Run Once`、`Hold to Run`、`Click Start/Stop`、`Click to Replay` | **未確認**（`cycles_num` / `interval_ms` / `special_flag` との対応が未確定） | 部分実装: `cycles_num` / `interval_ms` は数値として編集・書込可。4 モードの意味づけは無い | 実機未検証 | 4 モード ↔ フィールドの追加 RE、UI |
| ループ / ラウンドモード | `Loop Interval:`、`Round Mode:`（Single / Repeated / Multiple / Infinite） | **未確認**（ラウンドモードの格納先不明） | 部分実装（`interval_ms` / `cycles_num` のみ） | 実機未検証 | ラウンドモードの格納先 RE |
| マクロの復元 | — | **RE 確認済み**（読み書きコマンド） | 実装済み（CLI `macro restore`。GUI は復元 UI 無し） | 実機確認済み（復元後に config SHA が元に一致） | GUI 復元 UI |
| ヘッダ `total_len` のプラットフォーム差 | — | **未確認**: Windows は `total_len=0x0AD0`、Android は `0xD8`（BLE 実機では `0xD8` が通ることを確認済み） | 本実装は Android の `0xD8` を既定に採用（BLE 実機で永続化確認）。`0xAD0` は試していない | 実機確認済み（`0xD8` 経路） | Windows 値が BLE でも通るかの A/B（不要なら N/A と記録） |

---

## 11. ファームウェア更新（OTA）

| 項目 | 公式ソフトでの内容 | RE 証拠 | 実装状況 | 実機（Linux BLE） | 残作業 |
| --- | --- | --- | --- | --- | --- |
| バージョン表示 | `Current Firmware Version` | **RE 確認済み**（`ReadVersion` `0x000B`） | 実装済み（CLI `info`、GUI デバイスタブ、`backup`） | **実機確認済み**（ReadVersion `200 (0xC8)`） | なし |
| BLE OTA | `Update`、`Updating`、`Update Complete`、`Get beta Firmware` | コマンド列は **RE 確認済み**（`0x000C` EnterBoot → `"8bt"` → `0x0008/0x0004/0x0003/0x0005/0x00C4/0x0006`）。wire 詳細は **未確認**（[PROTOCOL_SPEC.md](PROTOCOL_SPEC.md) §8.5 の 1〜13） | **core のみ実装・未配線**（`crates/eightbitdo-core/src/ota.rs`。既定で未確認 wire は送信拒否するゲート付き。`ota-bench` で段階停止 API あり）。CLI / GUI / UniFFI 未配線 | **実機評価不能**（実機 btsnoop 未取得。フラッシュ書込は未実施） | 実機 btsnoop で ACK 形状・アドレス・MTU を確定、ACK の `offset` / `total_len` 照合、マルチセクション `.dat`、バッテリー / ダウングレード判定、配線 |
| ファームウェア取得 API | `POST /firmware/select?firmware_type=..&beta=..&soft_type=5` | **RE 確認済み**（API 仕様は [PROTOCOL_SPEC.md](PROTOCOL_SPEC.md) §8.1） | 未実装（ダウンロード・SHA-256 検証なし） | 実機評価不能 | API クライアントと整合性検証の実装（OTA 配線時） |
| USB OTA | 公式は USB 接続で更新 | **未確認**: USB Boot TX フレーミング未文書化 | 未実装（保留。`N/A + 理由（USB Boot TX 未文書化）`） | 実機評価不能 | USB Boot TX の RE 証跡が揃うまで着手しない |
| OTA 画面（GUI） | バージョン・進捗・復旧案内 | — | 未実装（Phase 3c 未着手） | 実機評価不能 | core の wire 確定後に実装 |

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
| バッテリー表示 | デバイス選択画面のバッテリー画像 | 入力レポート `0x0003` の `+0x0A`。**スケール（0-100 / 0-255）と充電状態は未確認** | 部分実装: CLI `monitor` と GUI 入力タブが生値を表示。% 表示・低残量警告は無い | 生値表示は実機確認済み（スケール未確認） | スケール / 充電ビットの RE、% 表示 |
| ポーリングレート | `Polling Rate(Hz)` | **RE 確定**: `0x600F` 非対応（UT3E の Tournament Mode 専用） | 対象外（N/A） | — | なし（0x600F では提供しない） |
| モード切替（Switch / XInput 等） | `SelectPlatform`、`platformview.xaml` | `gamepad_mode` `+0x10` の読取は **RE 確認済み**。書換の実機挙動は **未確認** | 実装済み: `ConfigEdit::GamepadMode` で 2768B のフィールドを書く（CLI `config set gamepad_mode`、GUI）。**フィールド書換でモードが実際に切り替わるかは未検証** | 実機未検証 | 書換の実機確認（BLE リンクが切れる可能性あり） |
| キャリブレーション | `Start Calibration` / `Calibrating` | **推測**（UI 文言のみ。2768B 外の別コマンドの可能性） | 未実装 | 実機評価不能 | 追加 RE |
| USB HID / 2.4G トランスポート | 公式 Windows の主経路（USB / 2.4G） | USB App Mode は **推測**、USB Boot TX は **未確認** | **未実装（保留）**。config / macro write は BLE のみ | 実機評価不能 | USB App Mode の実機証跡と transport 実装 |
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
  - 既知の blocker: ACK の `offset` / `total_len` 照合が未実装、マルチセクション `.dat` 非対応、Bootloader MTU 未確認
- **raw flag の実機対応（PR #17）**: 各レコードの `flag` は実機観測値集合（`0` / `1` / `0x20200911` / `0xFFFFFFFF`、gyro は `0x20190000` も）のみ受理し、それ以外は `--force-raw`。`slot_flags` も同じ観測値集合。型付き `config set` の flag は 0/1 のみ。詳細は [DEVICE_CONFIG_EVIDENCE.md](DEVICE_CONFIG_EVIDENCE.md)

---

## 15. 残作業（優先順）

1. **GUI 書込（設定 / リマップ / マクロ）の実機確認**（Tier 3）。pre-write backup と read-back を前提に、Linux BLE・MTU 200 で確認する。
2. **BLE OTA の wire 確定と配線**: 実機 btsnoop で [PROTOCOL_SPEC.md](PROTOCOL_SPEC.md) §8.5 の未検証事項（特に ACK 形状 / アドレス規約 / Bootloader MTU）を確定し、ACK の `offset` / `total_len` 照合とマルチセクション `.dat` 対応を入れてから CLI / GUI / UniFFI に配線する。
3. **プロファイル管理の完成**: `0x0015` 切替（core + CLI 配線済み）の ACK 形状・Save 要否・永続性の実機確認、GUI 切替 UI、Copy / Delete の `ConfigEdit` 化と CLI / GUI 露出、`slot_flags` の意味確定。
4. **実機確認の拡大**: `config set` の各フィールド（スティック / トリガー / 振動 / ジャイロ / LED / プロファイル名）、MTU 200 以外、REPL 経由の書込、M3/M4・S2/S3、Switch 側でのマクロ発動・リマップ実効。
5. **追加 RE が必要な項目**: ターボのレート / 3 段階、LED の Brightness と効果名、マクロ実行モード / ラウンドモード、空マクロ作成、キャリブレーション、バッテリースケール / 充電状態、キーボードキー割当の可否。
6. **GUI の残機能**: restore UI、JSON/TOML 入出力、OTA 画面（2 の後）。
7. **USB / 2.4G トランスポート**（Phase 2e / 4）: USB App Mode の実機証跡と USB Boot TX の RE が揃うまで実装しない。
8. **クロスプラットフォーム実機**: macOS / Windows / Android / iOS。現状の実機証跡は Linux BLE のみ（ビルド確認状況は [CROSS_PLATFORM.md](CROSS_PLATFORM.md)）。

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
