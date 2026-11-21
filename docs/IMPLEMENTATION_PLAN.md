# 実装計画書（8BitDo Ultimate 2 Cross-Platform Suite）

## 1. プロジェクト目標

**8BitDo Ultimate 2 Bluetooth Controller（内部 PID `0x600F` / 2.4G レシーバー `0x6011`）** を、Nintendo Switch へ接続した状態を維持しながら、**Linux / macOS / Windows / Android / iOS** の各環境から BLE GATT（または USB/2.4G HID）経由でフル設定・マクロ編集・ファームウェアアップデートできるオープンソース実装を構築する。

### 解決する課題
- 公式アプリ（8BitDo Ultimate Software V2）は Linux に非対応であり、マクロのインポート／エクスポートや細かい 1ms・8-bit アナログ値の編集が制限されている。
- Switch と接続したまま PC やスマホからリアルタイムにマクロやプロファイルを調整したいケースで、統一されたネイティブ環境がない。
- モバイル（Android / iOS）も含めて、BLE OTA ブートローダー経由のファームウェア更新や公式同等の全設定項目を一貫したプロトコルコアで扱いたい。

---

## 2. アーキテクチャ設計

プロトコル処理・2768 バイト設定構造体・マクロシリアライザ・CRC 計算・OTA ステートマシンを単一の Rust コアライブラリ（`eightbitdo-core`）に集約し、各プラットフォームのネイティブ UI から利用する構成とする。

```mermaid
graph TD
    subgraph Shared["共通コア (Rust Workspace)"]
        Proto["eightbitdo-protocol<br/>(no_std 互換・フレーミング・CRC16・2768B構造体・マクロ)"]
        Core["eightbitdo-core<br/>(セッション管理・OTAステートマシン・FW APIクライアント)"]
        FFI["eightbitdo-uniffi<br/>(UniFFI Swift / Kotlin バインディング)"]
        Proto --> Core
        Core --> FFI
    end

    subgraph Desktop["デスクトップ (Pure Rust・単一バイナリ)"]
        CLI["eightbitdo-cli<br/>(CLI / スクリプト自動化 / JSON・TOML I/O)"]
        GUI["eightbitdo-gui<br/>(egui / iced ネイティブ GUI)"]
        BTDesk["btleplug (BlueZ / CoreBluetooth / WinRT)"]
        HIDDesk["hidapi (hidraw / IOKit / hid.dll)"]
        CLI --> Core
        GUI --> Core
        Core --> BTDesk
        Core --> HIDDesk
    end

    subgraph Mobile["モバイル (各OSネイティブ UI + UniFFI)"]
        iOS["apps/ios (SwiftUI + CoreBluetooth)"]
        Android["apps/android (Kotlin + Jetpack Compose + Android BLE / USB OTG)"]
        iOS --> FFI
        Android --> FFI
    end
```

### 技術選定理由
| レイヤー | 採用技術 | 採用理由 |
| --- | --- | --- |
| 共通プロトコル＆コア | **Rust** | メモリ安全性、厳密なバイト列・ビットマスク処理、クロスプラットフォーム、OSS コミュニティの厚さ、`UniFFI` による iOS/Android ネイティブ連携の容易さ |
| デスクトップ BLE | `btleplug` | Linux (BlueZ D-Bus), macOS (CoreBluetooth), Windows (WinRT) を単一 API で扱える |
| デスクトップ USB/2.4G HID | `hidapi` | Linux (`hidraw`/`libusb`), macOS (IOKit), Windows (`hid.dll`) に対応 |
| デスクトップ GUI | **`egui` (`eframe`)** （代替候補: `iced`） | Pure Rust・Node.js/WebView 不要・軽量な単一バイナリ配布が可能・コントローラ入力のリアルタイム描画（60fps スティック/ボタンモニター・マクロタイムライン）と相性が良い |
| デスクトップ CLI | `clap` + `serde` (`serde_json` / `toml`) | マクロやプロファイルのバージョン管理・バッチ適用・CI テストに必須 |
| iOS アプリ | **Swift + SwiftUI + CoreBluetooth** | iOS での BLE バックグラウンド権限・GATT MTU 制御・OTA 再接続を最も安定して実装できるネイティブ構成。ロジックは `eightbitdo-uniffi` 経由 |
| Android アプリ | **Kotlin + Jetpack Compose + Android BLE / USB Host API** | BLE GATT と USB OTG HID の両方をネイティブ API で確実に制御できる。ロジックは `eightbitdo-uniffi` 経由 |

---

## 3. リポジトリ構成（目標レイアウト）

```text
8bitdo/
├── README.md
├── AGENTS.md                   # AI エージェント向け引き継ぎガイド
├── CLAUDE.md                   # AGENTS.md への参照
├── docs/
│   ├── PROTOCOL_SPEC.md        # BLE GATT / USB HID / CRC16 / OTA / FW API 完全仕様
│   ├── STRUCT_BT2_2768B.md     # 2768B 設定構造体 & 10B マクロステップ完全仕様
│   └── IMPLEMENTATION_PLAN.md  # 本実装計画書
├── Cargo.toml                  # Rust Workspace ルート
├── crates/
│   ├── eightbitdo-protocol/        # 純粋バイト列処理（I/O 非依存・単体テスト網羅）
│   │   ├── src/
│   │   │   ├── crc.rs          # ANSI_CRC_16_sh (0x8408) & Packet CRC16 (0xA001)
│   │   │   ├── header.rs       # 16B cumstom_header_t・BLE/HID フレーミング
│   │   │   ├── commands.rs     # App Mode & Bootloader コマンドビルダー/パーサー
│   │   │   ├── config_bt2.rs   # 2768B (0xAD0) record_ultimatebt2 構造体
│   │   │   ├── keymap.rs       # 32-bit KeyMap ビットマスク & 22キーインデックス
│   │   │   ├── macro_data.rs   # 10B MacroDatas・32Bアライン・JSON/TOML 変換
│   │   │   └── input_report.rs # cmd 0x0003 リアルタイム入力レポートパーサー
│   ├── eightbitdo-core/            # 非同期トランスポート抽象 + セッション + OTA + FW API
│   │   ├── src/
│   │   │   ├── transport.rs    # Transport trait (BLE / USB HID / Mock)
│   │   │   ├── ble.rs          # btleplug 実装 (desktop feature)
│   │   │   ├── hid.rs          # hidapi 実装 (desktop feature)
│   │   │   ├── session.rs      # 接続・初期化・設定/マクロ読み書き・リアルタイム録音
│   │   │   ├── firmware_api.rs # 8BitDo ファームウェア配信 API クライアント
│   │   │   └── ota.rs          # Bootloader 遷移 & Flash 書き込みステートマシン
│   ├── eightbitdo-cli/             # CLI ツール（実機検証・自動化・I/O）
│   ├── eightbitdo-gui/             # デスクトップ GUI (egui / eframe)
│   └── eightbitdo-uniffi/          # Swift / Kotlin 向け UniFFI 公開レイヤー
├── apps/
│   ├── ios/                    # SwiftUI アプリ (CoreBluetooth Transport 実装)
│   └── android/                # Jetpack Compose アプリ (BLE / USB OTG Transport 実装)
└── packaging/
    └── linux/
        └── 99-8bitdo-ultimate2.rules  # USB HID 用 udev ルール
```

> **クレート名について**: Cargo のパッケージ名は数字で開始できないため、正式名は `eightbitdo-*` とする（`8bitdo-*` は使用不可）。ディレクトリ名もパッケージ名に合わせる。

---

## 4. 機能スコープ

### 4.1 マクロ機能（最優先）
1. **リアルタイム入力録音**: `cmd 0x0007` でレポート有効化 → `cmd 0x0003` の通知からボタン・左/右アナログスティック・L2/R2 トリガー・経過ミリ秒を自動キャプチャし、ステップ列に変換。
2. **高精度 GUI ステップエディタ**:
   - ステップごとの **1ms 単位**の持続時間 (`ms_times`: `u16`) 編集
   - 16bit ボタン同時押しマスク編集
   - 左右スティックの **8-bit 精密座標** (`0x00..0xFF`, ニュートラル `0x7F`)・方向プリセット・角度/倒し量入力
   - L2/R2 アナログトリガー値 (`0x00..0xFF`) の個別設定
   - 実行モード (`once` / `hold` / `loop`)、繰り返し回数 (`cycles_num`)、間隔 (`interval_ms`)、トリガーボタン割り当て (`KeyMap`, P1/P2/L4/R4 等含む)
   - 最大 200 ステップ／マクロ、スロットあたり最大 4 マクロ × 3 スロット管理
3. **インポート／エクスポート**:
   - 人間が読める **JSON / TOML** 形式でのマクロ単体およびプロファイル一括の保存・読み込み・共有

### 4.2 コントローラ全設定（2768 バイト構造体パリティ）
- **プロファイル管理**: スロット 1〜3 の切り替え (`cmd 0x0015`)、プロファイル名（UTF-16BE 最大 32 バイト）、スロット初期化・コピー・バックアップ
- **ボタンリマッピング**: 22 物理ボタンの任意リマップ（Turbo / Swap / L4 / R4 / P1 / P2 / ScreenShot 等含む）
- **スティック＆トリガー**: デッドゾーン開始/終了値、特殊機能フラグ（反転・スワップ・ヘアトリガー等）、拡張カーブ領域の安全な保持および編集
- **振動（Rumble）**: 左右モーター強度 (`f32`)
- **6軸ジャイロ（SixAxis）**: トリガーボタン、発動モード、感度、デッドゾーン補正、マッピング先
- **RGB ライティング**: Dynamic / HotWheel (速度 `0..15`) / Single (24 LED カラー配列)

### 4.3 ファームウェアアップデート（PC / Android / iOS）
- 8BitDo 公式 API からの最新／ベータ版ファームウェア照会およびダウンロード
- ローカル `.dat` / `.bin` ファームウェアファイルの指定適用
- BLE Bootloader OTA（`cmd 0x000C` → `"8bt"` 再接続 → `0000FF20`/`0000FF21` → Flash 消去・書込・バージョン・APPID・JumpApp）
- USB HID 接続時のファームウェア更新（PC および Android USB OTG）
- **安全機構（ブリック防止）**: バッテリー残量確認警告、事前サイズ・CRC 検証、リトライ制御、書き込み中の電源断警告、中断時の `"8bt"` ブートローダー直接復旧モード

---

## 5. フェーズ別実装ロードマップ

> **2026-11-21 改訂**: ユーザー優先順位（**BLE マクロ書き込みが本命**、Linux でのファームウェア更新も必要）に合わせ、**BLE OTA を GUI より前に前倒し**した。USB OTA は USB Boot TX フレーミングが未文書化のため保留する。当初案の「Phase 3 = GUI / Phase 4 = OTA」から変更している。

### 5.1 トラック構成

```text
Track A: コア / CLI（Linux・デスクトップ）
  Phase 1  protocol                 完了
  Phase 2a read-only + session      ← いまここ
  Phase 2b macro write              本命
  Phase 2c BLE OTA (Linux)
  Phase 2d config apply
  Phase 2e USB / 2.4G transport     保留

Track B: GUI（egui / Pure Rust）
  Phase 3a read-only GUI            2a 後（前倒し可）
  Phase 3b macro editor             2b 後
  Phase 3c OTA screen               2c 後
  Phase 3d settings tabs            2d 後

Track C: モバイル
  Phase 5  UniFFI + iOS / Android   最後
```

### 5.2 現状（2026-11-21）

| 項目 | 状態 |
| --- | --- |
| Phase 1 | 完了。ただし実機で ReadVersion の 4B ペイロード、ReadConfig の `chunk_len = 0x0AD0`、1 要求 = 1 チャンク、最終 142B パディングを補正した |
| Phase 2a | 進行中。実機で ReadVersion **200 (0xC8)**、2768B dump（SHA-256 `1dca47fa...59c600`）、20ms keepalive **128.9 秒**を確認 |
| レビュー | Phase 2a は **Tier 3**（3 セッション）。`crates/eightbitdo-core/**` の session/backup と mock transport を含むため。Phase 2b 以降の書き込みも **Tier 3**（3 セッション） |

### Phase 1: `eightbitdo-protocol`（純粋プロトコル＆バイナリ層）
- **ゴール**: 実機なしで全パケット・CRC・2768B 構造体・10B マクロのエンコード／デコードを単体テストで検証する。
- **実装項目**:
  - `ANSI_CRC_16_sh` (poly `0x8408`) と Packet `crc16` (poly `0xA001`) の実装とテストベクタ検証
  - `cumstom_header_t` (16B LE) および BLE (`0x04`/`0x05`)・USB HID (`0x81 0x04` / `0x02 0x04`, 64B) フレーミング
  - `record_ultimatebt2` (2768B `0xAD0`) の双方向パース・シリアライズ（未知・予約バイトを一切破壊しないラウンドトリップ保証）
  - `MacroDatas` (10B) と 32B アラインされた Flash イメージ生成・パース、JSON/TOML スキーマ定義
  - `cmd 0x0003` リアルタイム入力レポートパーサー
- **完了条件**: `cargo test -p eightbitdo-protocol` がすべてパスすること。**実機なしの 100% 検証は達成できず**、Phase 2a の実機検証で読み取り経路の仕様補正を行った。

### Phase 2a: `eightbitdo-core` + `eightbitdo-cli` 読み取り専用 + `session`
- **ゴール**: 1 回の Profile 長押しで、Switch 接続を維持したまま version / 2768B config / backup / monitor を実行する。
- **実装項目**:
  - `Transport` トレイト（BLE / Mock、非同期 `send` / `recv` / `mtu` / `disconnect`）
  - BLE `"80ND"` スキャン、`FF10` + `FF11` 優先 / `FF13` フォールバック、`peripheral.mtu()` の読み取り（MTU ネゴシエーション自体は OS/BlueZ 任せ）
  - `Session`: operation lock（keepalive とコマンドの直列化）、ReadVersion（4B ダミーペイロード + `cmd=0x0004` エンベロープ）、ReadConfig（`chunk_len = total_len = 0x0AD0`、1 要求 = 1 チャンク、142B、最終チャンク切り詰め）、入力レポート
  - CLI: `scan` / `info` / `config dump` / `backup` / `monitor` / `keepalive` / **`session`**
  - `session`: 1 接続、`0x0007` enable + 20ms `0x0003` keepalive + 受信ドレイン、operation lock でコマンド中は keepalive を停止、`info` / `config dump` / `backup` / `monitor` / `monitor stop` / `status` / `help` / `quit`。`monitor` 自身も 20ms `0x0003` を送り、その間は keepalive が止まる（monitor が keepalive を兼ねる）。`monitor stop` 後は keepalive が自動再開。トップレベルの `monitor` も同じ 20ms `0x0003` ポーリングを共有し、終了時に `0x0007` disable → 切断する。なお `monitor --count N` は公式 `ReadKeysAndJoy` と同様に投げっぱなしで、応答が 1 件も来ない場合も 20ms 周期で `0x0003` を送り続け、`--count` に達するまで Ctrl-C まで回り続ける。`status` は `monitor sent` / `monitor received` を表示し、終了時のロック待ち・切断はタイムアウト付きで有界にする。連続失敗 3 回または `Disconnected` で degraded 停止し、自動再接続しない（`keepalive degraded ...` のメッセージで、設定モードが切れた可能性と Profile 長押しでの再入を案内する）。1 tick の受信ドレインは最大 4 レポート（`try_next_input` 自体も 1 回最大 16 フレーム + 1ms）に固定し、通知が途切れなくても operation lock を保持し続けない。設定の `--raw` / `--json` は同一ディレクトリの一時ファイルへ書いてから rename するアトミック書込で、同一ファイルを指す 2 つの出力パス（`.` / `..`、symlink、Unix の hard link）は端末にアクセスする前に拒否する。hard link の同一性検出は Unix のみ（device/inode）で、Windows は file identity 未実装のため hard link alias を事前拒否しない。ただし出力は一時ファイル + `rename` なので、JSON の `rename` が raw の内容を直接書き換えることはない。case-insensitive alias は raw 書き込み後の再検証で拾う。この再検証は偶発的な alias 用であり、出力ディレクトリに write 権限を持つ攻撃者による TOCTOU / 並行 rename は非保証である。REPL の 1 行は最大 4096 バイトで、超過分は読み捨てて次の行を正常に読む。`session` の Ctrl-C はコマンド実行中でも future を drop して REPL を抜け、セッションを終了する。行リーダーのキャンセル耐性は `monitor` / keepalive の終了通知による中断で読みかけの行を壊さないためのもので、Ctrl-C 後の継続を意味しない。
  - `session` は将来のデーモン化（常駐プロセス）への布石。1 回の Profile 長押しで複数操作を完結させる
  - docs: `FF13` 実機観測、`cmd=0x0004` エンベロープ、ReadConfig の `chunk_len`、keepalive 設計
- **完了条件**: 1 回の Profile で `session` 内の `info` → `backup` → `monitor` が通り、Tier 3 レビューと CI 4/4 を満たすこと。
- **状態**: 進行中。

### Phase 2b: BLE マクロ読み書き（本命）
- **ゴール**: 3 スロット（`slot 0..2`）× 4 マクロ（`macro_idx 0..3`）のバックアップと、Erase → Write → Header → Save の実機書き込み。
- **実装項目**:
  - `0x0102` ReadBT2Macro（`offset = macro_idx << 12`, `total_len = chunk_len = steps * 10`）
  - `0x0104` Erase（`total_len = chunk_len = 0x1000`, `crc16 = 0xFFFF`, ペイロードなし）
  - `0x0103` Write（BLE 64B / USB 32B、32B アライン、チャンクごと CRC）
  - `0x0001` Header（マクロステップ数更新）
  - `0x0006` Save（BLE `subcmd = 0x0067`）
  - 3×4 バックアップ（raw + JSON/TOML + SHA-256）
  - Mock でパケット列・CRC・チャンク境界・タイムアウト（Erase / Write / Header 2 秒、Save は transport 方針）を固定
  - fail-closed（失敗時に Save しない）、読み戻し検証
- **完了条件**: 実機で 1 マクロ書き込み → 読み戻し一致 → Switch 側で発動 → 元のマクロへ復元。
- **Tier**: **3**（write-path / Flash / CRC / マクロ）。
- **状態**: 未着手（Phase 2a 後）。

### Phase 2c: Linux BLE OTA
- **ゴール**: Linux から BLE Bootloader 経由でファームウェア更新・復旧を行う。
- **実装項目**:
  - 公式 API またはローカル `.dat` の取得・検証（PID / バージョン / サイズ / CRC）
  - `0x000C` EnterBoot（`subcmd=0x0067`）→ `"8bt"`（`FF20` / `FF21`）へ再接続
  - `0x0008` readPID（5s）、`0x0005` readVersionInBoot（type 1 は `offset=0x01017040`）
  - `0x0004` EraseFlash（4KB、10s）、`0x0003` WriteFirmware（64B、1s）、`0x00C4` WriteVersion（3s）、`0x0006` JumpApp
  - バッテリー警告、途中失敗時の `"8bt"` 復旧、全セクタ消去 → 再書き込み
  - Mock で正常系・タイムアウト・CRC 不一致・途中切断を固定
- **完了条件**: 同一バージョン再書き込み → 最新版更新 → 復旧確認。
- **Tier**: **3**（OTA / Flash / セキュリティ境界）。
- **状態**: 未着手（Phase 2b 後）。

### Phase 2d: 設定書き込み（config apply）
- **ゴール**: 2768B の部分書き込みと Save を CLI から安全に実行する。
- **実装項目**:
  - プロファイル名、リマップ、スティック / トリガー、振動、6 軸、RGB の部分書き込み
  - `ANSI_CRC_16_sh` 再計算、`total_len = 0x0AD0` を維持したチャンク分割
  - `0x0006` Save（BLE `0x0067`）
  - 書き込み前の 2768B バックアップと SHA-256 記録、読み戻し検証
- **完了条件**: 各セクションの書き込み → 読み戻し一致。
- **Tier**: **3**。
- **状態**: 未着手（Phase 2c 後）。

### Phase 2e: USB HID / 2.4G トランスポート（保留）
- **ゴール**: USB 有線 / 2.4G レシーバ経由の App Mode。
- **状態**: 保留。USB App Mode の実機証跡と、USB Boot の TX フレーミング（`EraseFlash` / `WriteFirmware` 等）が未文書化のため、BLE 経路を優先する。

### Phase 3: `eightbitdo-gui`（デスクトップ ネイティブ GUI：Linux / macOS / Windows）
- **技術**: `egui` (`eframe`)、Pure Rust。Node.js / WebView / Electron / Tauri は使わない。
- **Phase 3a（read-only GUI、Phase 2a 後）**: 接続パネル、デバイス情報、ライブ入力、設定ビューア、バックアップ。既定は **Tier 2**、ただし `crates/eightbitdo-core/**`（`session` / transport / mock を含む）の変更を伴う場合は **Tier 3**。GUI 側だけで完結する場合も、`session` の読み取り契約（ドレインの fail-closed、応答 `total_len` / `crc16` 照合、`Instant` 上限）を検証するテストを変えるなら Tier 3。
- **Phase 3b（マクロエディタ、Phase 2b 後）**: タイムライン / テーブル、リアルタイム録音、1ms、8bit スティック、トリガー、JSON/TOML。Tier 3。
- **Phase 3c（OTA 画面、Phase 2c 後）**: バージョン確認、進捗、復旧モード。Tier 3。
- **Phase 3d（設定タブ、Phase 2d 後）**: プロファイル、リマップ、デッドゾーン、振動、6 軸、RGB。Tier 3。
- **完了条件**: Linux / macOS / Windows でビルド・起動し、実機と同期すること。

### Phase 4: USB OTA（保留）
- **状態**: **保留**。USB Boot の TX フレーミングが未文書化のため、証跡が `PROTOCOL_SPEC.md` に追加されるまで実装しない。
- BLE の `"8bt"` / prefix `0x05` を USB に流用しない。USB の復旧は再列挙 / 再接続。
- **Tier**: 3。

### Phase 5: `eightbitdo-uniffi` + モバイルネイティブアプリ（iOS SwiftUI & Android Jetpack Compose）
- **ゴール**: iOS / Android から BLE（および Android USB OTG）経由で PC 版と同等の設定・マクロ編集・OTA 更新を実現する。
- **実装項目**:
  - `eightbitdo-uniffi`: Callback Interface として `NativeBleTransport` / `NativeUsbTransport` を Rust 側へ注入できる設計にし、OS 固有の BLE/USB スタックをネイティブ（CoreBluetooth / Android BluetoothGatt・UsbManager）で駆動しつつ、プロトコル・構造体・OTA ロジックを 100% Rust で共有する。
  - `apps/ios`: SwiftUI UI + `CoreBluetooth` トランスポート + XcodeGen / SwiftPM ビルド構成（`XCFramework` 生成スクリプト含む）
  - `apps/android`: Kotlin + Jetpack Compose UI + Android BLE GATT / USB OTG トランスポート + `cargo-ndk` ビルド構成
- **完了条件**: iOS / Android 実機で `"80ND"` 接続・設定変更・マクロ編集・BLE OTA が動作すること。
- **Tier**: 3。

### 5.3 クリティカルパス

1. Phase 2a `session` 完了 → Tier 3 レビュー → マージ
2. Phase 2b マクロ書き込み → Tier 3 レビュー → マージ
3. Phase 2c BLE OTA → Tier 3 レビュー → マージ
4. Phase 2d 設定書き込み → Tier 3 レビュー → マージ
5. Phase 3 GUI（3a は 2a 後に前倒し可）
6. Phase 5 モバイル

---

## 6. 開発・検証上の安全原則

1. **実機書き込み前の必須バックアップ**: 最初の書き込み実験前に必ず 2768 バイトの原本バイナリと既存マクロ 4×3 スロットをダンプ保存する。
2. **未知フィールドの保全**: 2768 バイト構造体内の予約領域・未編集領域は `read -> modify -> recalculate CRC -> write` の流れで元のバイト列をそのまま保持する。
3. **OTA 開発時の注意**: Bootloader コマンド（特に `EraseFlash` / `WriteFirmware`）は、まずモックトランスポートとパケットダンプで公式アプリの通信ログと完全一致を検証してから実機に適用する。
4. **プロプライエタリ資産の非公開**: 解析に用いた公式 `.apk` / `.exe` / `.dll` や公式ファームウェアバイナリ自体は絶対にリポジトリへコミットしない。
