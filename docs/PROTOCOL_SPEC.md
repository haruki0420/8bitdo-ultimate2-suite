# 8BitDo Ultimate 2 Bluetooth Controller (PID `0x600F`) 通信プロトコル完全仕様書

本書は、8BitDo Ultimate Software V2（Android 版 `libadvancelib.so` / DEX、および Windows 版 `8BitDoAdvance.dll` / `8BitDo Ultimate Software V2.dll`）の静的解析に基づき、**8BitDo Ultimate 2 Bluetooth Controller (`PID 0x600F`)** を中心とした通信プロトコル・パケットフレーミング・CRC-16・設定コマンド・リアルタイム入力ストリーム・ファームウェア更新仕様を定義する。

---

## 1. 概要と同時接続トポロジー

### 1.1 デュアルリンク（Switch + BLE GATT 同時接続）の仕組み

8BitDo Ultimate 2 Bluetooth Controller は、ホスト（Nintendo Switch または 2.4GHz レシーバ `0x6011`）へゲームプレイ用の入力接続（Bluetooth Classic または 2.4GHz）を維持したまま、PC やスマートフォンから **BLE GATT 経由で同時に設定変更・プロファイル切替・リアルタイム入力監視・OTA ファームウェア更新** を行える構成になっている。

- **ゲーム入力プレーン**: Bluetooth Classic（Switch モード）または 2.4GHz レシーバ（`PID 0x6011`）経由で Switch / PC にコントローラとして接続。
- **設定・制御プレーン (BLE GATT)**: コントローラがアドバタイズ名 **`"80ND"`** としてペリフェラルアドバタイズを開始し、コンパニオンアプリ（PC / スマートフォン）が GATT サービス `0000FF10-...` へ接続する。
- **USB 有線接続**: PC 等へ USB ケーブルで直結した場合は、BLE GATT の代わりに USB HID インターフェース（Report ID `0x81` / `0x02`）上で同一の 16 バイトヘッダプロトコル（`cumstom_header_t`）をやり取りする。

```mermaid
graph LR
    Switch["Nintendo Switch / PC<br/>(ゲームプレイホスト)"]
    Ctrl["8BitDo Ultimate 2 Bluetooth<br/>PID: 0x600F"]
    App["PC / スマホ アプリ<br/>(Rust / Swift / Kotlin)"]
    API["8BitDo Firmware API<br/>(HTTPS REST)"]

    Switch <-- "Bluetooth Classic または 2.4GHz (0x6011)<br/>通常ゲーム入力" --> Ctrl
    App <-- "BLE GATT (名前: '80ND', Service: FF10 / Char: FF11)<br/>または USB HID (0x81 / 0x02)" --> Ctrl
    App <-- "Bootloader OTA 時: BLE '8bt' (Service: FF20 / Char: FF21)" --> Ctrl
    App <-- "HTTPS (/firmware/select, /firmware/download)" --> API
```

### 1.2 コントローラ側の BLE 設定モード起動手順

コントローラの電源が入っている状態（または Switch 等へ接続中）で、以下の操作を行うことで BLE 設定用ペリフェラル（`"80ND"`）が起動する。

1. コントローラの電源をオンにする（Switch 等に接続済みでも可）。
2. **Profile ボタンを 3 秒間長押し**する（Status LED が高速点滅に変わるまで押し続ける）。
   - 解析済み文字列リソース（Android APK）: `"2. Press and hold the Profile button for 3 \nseconds until the Status LED will blink rapidly."`
3. Status LED の高速点滅開始後、コントローラが BLE アドバタイズ名 **`"80ND"`** を送出するため、クライアントからスキャン・GATT 接続を行う。

---

## 2. デバイス VID / PID・モデル対応表

8BitDo 製品の共通ベンダー ID は **`VID = 0x2DC8`** である。  
`PIDVID`（Android）および `AdvanceSuper.Hid.VIDPID` / `8BitDoAdvance.dll::enum_hid`（Windows）から確認された対応表を示す。

### 2.1 Ultimate 2 ファミリーおよび関連デバイス

| モデル名 | 内部 PID (App / Boot) | 10進数 | BLE アドバタイズ名 | Firmware API `firmware_type` | 備考 |
| --- | --- | --- | --- | --- | --- |
| **Ultimate 2 Bluetooth Controller** | **`0x600F`** (`PID_UltimateBT2` / `UT2_PID`) | `24591` | **`"80ND"`** (App) / **`"8bt"`** (Boot) | **`97`** (`0x61`) | 本仕様書の中心対象。Windows USB 接続時は XInput モードで `0x310B` (`xinput == 2`)、更新時 `0x3208` として列挙される場合がある |
| **Ultimate 2 Bluetooth 2.4G Receiver** | **`0x6011`** (`PID_UltimateBT2RR`) | `24593` | — | **`98`** (`0x62`) | Ultimate 2 Bluetooth 付属 2.4GHz レシーバ |
| **Ultimate 2 Wireless Controller** | **`0x6012`** (`PID_Ultimate2`) | `24594` | — | **`94`** (`0x5E`) | XInput 時 `0x310B` (`xinput == 1`) |
| **Ultimate 2 Wireless Adapter** | **`0x6013`** (`PID_Ultimate2RR`) | `24595` | — | **`95`** (`0x5F`) | Ultimate 2 Wireless 用ドングル |
| **Ultimate 2C Bluetooth** | **`0x301A`** (`PID_UltimateBT2C`) | `12314` | — | **`108`** (`0x6C`, App) / **`96`** (`0x60`, Boot) | Bluetooth 2C モデル |

### 2.2 その他の同一プロトコル系 VID / PID・USB 列挙 PID

| 項目 / 定数名 | PID (Hex) | 用途・モデル |
| --- | --- | --- |
| `PID_Xinput` | **`0x310B`** | Windows XInput 列挙 PID（HID `UsagePage = 0xFF7A`, `Usage = 0x0001` のサブインターフェースを併用） |
| Boot / Update 切替用 USB PID | **`0x3208`** | Windows `enum_hid` / `HIDManage.readGamePadNewVersion` で使用される更新・切替時 USB PID（検出時は内部フラグ設定後 `getDevicePID1` で内部 PID を照会） |
| `PID_IDLE` | **`0x3109`** | USB アイドル / 待機状態 PID |
| `PID_USB_Ultimate` / `Classes` / `2` / `BTRR` / `USB2_IDLE` | `0x3100` / `0x3104` / `0x3105` / `0x3106` / `0x3107` | USB 有線・ドングル群 |
| `PID_QINGCHUN2` / `PID_QINGCHUN2RR` | `0x310A` / `0x301C` | 青春版 2 / レシーバ |
| `PID_UltimateBT` | `0x6007` | 初代 Ultimate Bluetooth |
| `PID_Pro3` / `PID_Pro3USB` / `PID_Pro3DOCK` | `0x6009` / `0x600A` / `0x600D` | Pro 3 シリーズ |
| `PID_Pro2` / `PID_Pro2_CY` | `0x6003` / `0x6006` | Pro 2 シリーズ（`_CY` は BLE Characteristic `FF13`/`FF23` を使用） |
| `PID_Ultimate2_4` / `PID_Ultimate2_4RR` | `0x3012` / `0x3013` | Ultimate 2.4G シリーズ |
| `PID_HitBox` / `PID_HitBoxRR` | `0x600B` / `0x600C` | Arcade HitBox 2 (`firmware_type`: `129` / `130`, `BootPID 0x600B` は `101`) |
| `PID_N64BT` / `PID_N64` / `PID_N64RR` | `0x3019` / `0x3004` / `0x9028` | N64 シリーズ (`firmware_type`: `78` / `71` / `103`) |
| `PID_Lashen` | `0x301E` | Lashen シリーズ (`firmware_type`: `100`) |

### 2.3 Windows HID 列挙時の UsagePage / Usage フィルタ (`8BitDoAdvance.dll::enum_hid`)

Windows の `FindHid` / `enum_hid` は `HidD_GetPreparsedData` + `HidP_GetCaps` により複合 HID デバイスから制御用インターフェースを選別する。

| 対象 USB PID (`VID = 0x2DC8`) | `UsagePage` | `Usage` | 備考 |
| --- | --- | --- | --- |
| `0x310B` (`PID_Xinput`), `0x2084`, `0x3004` | **`0xFF7A`** | **`0x0001`** | XInput 併用時のベンダー制御インターフェース |
| `0x5205`, `0x5206` | **`0xFF00`** | **`0x0001`** | Vendor-defined UsagePage `0xFF00` |
| `0x5201`, `0x5209`, `0x520A`, `0x520B`, `0x520C` | **`0x008C`** | **`0x0001`** | — |
| `0x2039`, `0x2054`, `0x2056` | **`0x000C`** | **`0x0001`** | Consumer Control インターフェース |
| `0x600F`, `0x6009`, `0x6013`, `0x301A`, `0x600B`, `0x3208` 等 | フィルタなし（VID/PID 直接一致） | — | `enum_hid` 上で `VID/PID` 一致後そのまま `CreateFile` で開かれる |

> **実装上の注意**: Windows 環境で `0x600F` が XInput モード（`0x310B`）として見えている場合は **`UsagePage = 0xFF7A, Usage = 0x0001`** のデバイスノードを開く必要がある。また `ReadHidAdvance` / `WriteHid_reportID` のバッファサイズは **64 バイトペイロード + Report ID 1 バイト = 65 バイト (`0x41`)** 固定で動作する。

---

## 3. BLE GATT トランスポート仕様

### 3.1 アドバタイズ名・UUID 一覧

| モード | アドバタイズ名 | Service UUID | Characteristic UUID (Write / Notify) | CCCD UUID |
| --- | --- | --- | --- | --- |
| **App Mode** (`0x600F`) | **`"80ND"`** | `0000FF10-0000-1000-8000-00805F9B34FB` | `0000FF11-0000-1000-8000-00805F9B34FB` | `00002902-0000-1000-8000-00805F9B34FB` |
| **App Mode** (`_CY` 系) | モデル依存 | `0000FF10-0000-1000-8000-00805F9B34FB` | `0000FF13-0000-1000-8000-00805F9B34FB` | `00002902-0000-1000-8000-00805F9B34FB` |
| **Bootloader Mode** (`PID=2`) | **`"8bt"`** | `0000FF20-0000-1000-8000-00805F9B34FB` | `0000FF21-0000-1000-8000-00805F9B34FB` | `00002902-0000-1000-8000-00805F9B34FB` |
| **Bootloader Mode** (`_CY` 系) | `"8bt"` | `0000FF20-0000-1000-8000-00805F9B34FB` | `0000FF23-0000-1000-8000-00805F9B34FB` | `00002902-0000-1000-8000-00805F9B34FB` |

- Characteristic (`FF11` / `FF21`) は **Write と Notify の双方**を担う単一キャラクタリスティック。
- 接続後、必ず CCCD (`00002902-...`) へ `[0x01, 0x00]`（Enable Notification）を書き込んでからコマンド送信を開始する。

### 3.2 MTU ネゴシエーションとチャンクサイズ制御

`SHBLEUtils` および `libadvancelib_pkt.c`（`Generate_CustomDataCmd` / `Send_CustomCmdPacket`）の制御仕様は以下の通り。

1. **初期 MTU リクエスト**:
   - Android クライアントは GATT 接続時に `requestMtu(100)` (`0x64`) を要求する。
   - `MAX_ATT_MTU` グローバル変数はネゴシエート結果またはモード切替によって更新される。
2. **1 パケットあたりの最大ペイロード長 (`chunk_len`)**:
   - 通常コマンド（`PID != 0x202F, 0x0553`）:
     - パケット総送信バイト上限: `MAX_ATT_MTU - 2`（先頭 1B プレフィックス含むため、ヘッダ 16B + ペイロードの合計上限は `MAX_ATT_MTU - 2` とみなされる、実質ペイロード上限は **`MAX_ATT_MTU - 0x12`**（`MAX_ATT_MTU - 18`）バイト）。
     - 例: `MAX_ATT_MTU = 100` のとき最大ペイロード `82` バイト。`MAX_ATT_MTU = 64` のとき `46` バイト。
   - **`cmd == 0x0103`（マクロデータ書き込み）**:
     - `chunk_len` は **`0x40`（64 バイト）を上限にクランプ**される（`min(chunk_len, 0x40)`）。
     - 呼び出し元の `getBT2_WriteMacro(slot, macro_idx)` では、マクロの `total_len` を `0x20`（32 バイト）単位に切り上げて転送する。
   - **`cmd == 0x0104`（マクロデータ消去）**:
     - チャンク上限クランプをスキップし、`bVar5 = true` としてそのまま送信する。
   - **`cmd == 0x0007`（`SetReportEnable`）実行時**:
     - `getSetReportEnableChannel()` 実行時に `MAX_ATT_MTU` を **`0x3F`（63 バイト）** へ変更する。これはリアルタイム入力ストリーム（12 バイト通知）中の MTU / パケットサイズを固定するためである。
   - 例外 PID (`0x202F`): `MAX_ATT_MTU = 0x3E` (62)、チャンク上限 `MAX_ATT_MTU - 0x13`。

---

## 4. パケットフレーミングと 16 バイトヘッダ (`cumstom_header_t`)

### 4.1 16 バイトヘッダ (`cumstom_header_t`) 定義

全マルチバイト整数は **Little-Endian (LE)**。

| オフセット | サイズ | 型 | フィールド名 | 説明 |
| --- | --- | --- | --- | --- |
| `+0x00` | 2B | `u16` | `cmd` | コマンド ID（例: `0x0001` Write, `0x0002` Read, `0x000B` Version） |
| `+0x02` | 2B | `u16` | `subcmd_or_param` | サブコマンド・引数・スロット指定（例: `0x0067` / `0x0123` Save, `(mode << 8) \| slot`） |
| `+0x04` | 2B | `u16` | `chunk_len` | このパケットに含まれるペイロード実長（送信要求コマンドでペイロード無しなら `0`） |
| `+0x06` | 2B | `u16` | `crc16` | ペイロードに対する CRC-16/MODBUS（条件付き、未使用時は `0x0000`） |
| `+0x08` | 4B | `u32` | `total_len` | 転送全体の総バイト数（または読み出し要求総長。`0x000B` 等では識別値 `0x01020030` 等） |
| `+0x0C` | 4B | `u32` | `offset` | 分割転送時の開始オフセット。ただし `cmd == 0x0001` / `0x0066`（`PID == 0x600F, 0x6009, 0x202F, 0x2072`）および `cmd == 0x000B` では `total_len` 側の値が設定される |

> **例外 (`cmd = 0x0104` `EraseBT2Macro`)**: このコマンドはペイロードを持たないが、公式実装 (`getBT2_EraseMacro`) はチャネルの `total_len` と `chunk_len` をともに `0x1000` に設定し、`Generate_CustomDataCmd` が `chunk_len` フィールドを wire header `+0x04` に書く。実際のペイロードは `len = 0` である。`chunk_len` の一般則（ペイロード実長）だけから `0` と解釈しないこと。

```text
 0      1 2      3 4      5 6      7 8            11 12           15
+--------+--------+--------+--------+---------------+---------------+
|  cmd   | subcmd |chunklen| crc16  |   total_len   |    offset     |  (16 bytes, LE)
+--------+--------+--------+--------+---------------+---------------+
```

### 4.2 BLE 送信フォーマット (`Send_CustomCmdPacket`)

`0x600F` を含む標準 BLE デバイスでは、16 バイトヘッダの前に 1 バイトのパケット種別プレフィックス（`cmd_packet`）を付与して GATT Characteristic (`FF11` / `FF21`) へ Write する。

- **App Mode 送信 (`cmd_packet = 0x04`)**:
  ```text
  [0x04] (1B) + cumstom_header_t (16B) + payload (chunk_len B)
  ```
- **Bootloader Mode 送信 (`cmd_packet = 0x05`)**:
  ```text
  [0x05] (1B) + cumstom_header_t (16B) + payload (chunk_len B)
  ```
- **送信長上限**: `1 + min(16 + chunk_len, MAX_ATT_MTU - 2)` バイト（ただし `cmd == 0x0103` のとき `chunk_len <= 0x40`）。
- **例外 (`cmd == 0x0104` `EraseBT2Macro`)**: ヘッダ `+0x04` には `chunk_len = 0x1000` が入るが、ペイロードは送らない（実送信長は 17B + プレフィックス）。このコマンドに上の `16 + chunk_len` の式を適用しない。

### 4.3 BLE 受信フォーマット (`HIDChannel_readBleHidData`)

`readBleHidData` は `PIDVID` に応じて受信 Notify バッファの先頭オフセットと照合ルールを切り替える。**`0x600F`（Ultimate 2 Bluetooth）では送信時と異なり、受信パケットに先頭 `0x04` プレフィックスが付かない点に注意が必要である。**

1. **先頭オフセットの分岐**:
   - `PID == 0x202F` / `0x3019` / `0x9023`: 先頭 **2 バイトをスキップ**（`ptr = rx + 2`）して `cumstom_header_t` として扱う。
   - **`PID == 0x600F` (Ultimate 2 BT) および `0x6009` (Pro 3) を含むその他の機種**: 先頭バイトをスキップせず、**`rx[0..16]` をそのまま 16 バイト `cumstom_header_t`**、`rx[16..]` をペイロードとして解釈する。
2. **応答・通知のディスパッチ規則**:
   - **App チャネル応答一致**: 先頭 `u16` が `0x0004`（またはチャネル比較一致）のルートでは、`pkt[2]`（またはヘッダ側 `cmd` 下位バイト）が進行中チャネルの識別子 `Channel[+0x32]` と一致し、かつ `*(u32*)(pkt + 8)` が `Channel[+0x40]` と一致した場合に `handleChannel()` へ渡す。
   - **非同期（Unsolicited）通知**:
     - `cmd == 0x0012`: 設定許可完了通知。`HIDChannel.allowConfigDone()` を発火。
     - `cmd == 0x0003` または `subcmd == 0x0003`: リアルタイム入力レポート。ペイロード 12 バイトを `KeysAndJoy` コールバックへ通知。
     - `cmd == 0x000E`: コントローラ側エラー通知。
   - **Bootloader チャネル応答一致**: Bootloader 応答では `*(u16*)pkt == 0x00F5` 等（`0xF5` を含む先頭タグ）や `pkt[2] == Channel[+0x32]` により進行中 Boot チャネルと照合する。

### 4.4 USB HID / 2.4G HID フレーミング（Windows `8BitDoAdvance.dll` / Android USB）

USB 有線接続および 2.4GHz ドングル経由の HID 通信は、**65 バイト固定（Report ID 1 バイト + 64 バイトレポート）** で行われる。

- **送信 (`WriteHid_reportID(..., reportID = 0x81, len = 0x40)`)**:
  - `8BitDoAdvance.dll::send_16B_inner` は `tx[0] = 0x81` を設定し、PID が `{0x600F, 0x6012, 0x6009, 0x600B, 0x2028, 0x2062}` の場合にペイロード長バイトを挟まず以下を送信する（残余バイトは `0x00` パディングで合計 65 バイト）:
    ```text
    [0x81, 0x04] (2B) + cumstom_header_t (16B) + payload (最大 0x20 = 32B) + [0x00 padding]
    ```
  - 補足: `send_custom_inner` では USB HID パケットあたりの `chunk_len` を最大 **`0x20`（32 バイト）** に制限する（`0x04`(1B) + `header`(16B) + `32B` = `49B <= 64B`）。
  - Bootloader 時の USB HID PID 読み出し（`getDevicePID1_inner`）では、**受信** レポートの先頭 `[0x00 (Report ID), 0x05, ...]` を検証する。`[0x00, 0x05]` は **RX 専用** であり、USB Boot の **TX** フレーミングではない。
  - **USB Boot の TX フレーミングは未文書化**: `EraseFlash` `0x0004` / `WriteFirmware` `0x0003` / `WriteVersion` `0x00C4` / `WriteAPPID` `0x0002` / `JumpApp` `0x0006` を USB Boot で送る際の framing は、現時点の解析では確定していない。USB OTA 書込を実装・検証する PR は、この節に TX フレーミングの証跡を追加するまで **停止** し、該当項目を `N/A + 理由（USB Boot TX 未文書化）` とする。BLE の `"8bt"` / prefix `0x05` を USB に流用しない。USB の復旧は再列挙（re-enumeration）/再接続であり、BLE の `"8bt"` 再接続ではない。

- **受信 (`wait_ack` / `read_hid_64` / `StepActivity`)**:
  - `ReadFile` で `0x41`（65）バイトを読み取り、先頭の Report ID 1 バイトを除いた 64 バイトバッファ `rx[0..64]` に対して以下を検証する:
    - App Mode: `rx[0] == 0x02 && rx[1] == 0x04` であることを確認し、`rx[2..18]` を `cumstom_header_t`、`rx[18..]` をペイロードとして解釈する。
    - Bootloader Mode (`getDevicePID1_inner`): 生読み取り `0x41` バイトの先頭が `[0x00, 0x05]` かつ `cmd == 0x0000, subcmd == 0x0008` であることを検証し、`+0x12` オフセットから PID（2B LE）を読む。

---

## 5. チェックサム / CRC-16 アルゴリズム（2 種類）

プロトコルでは用途の異なる **2 種類の独立した CRC-16** が使われている。両者とも初期値 `0xFFFF`、LSB-first（右シフト）、最終 XOR なしだが、**生成多項式が異なる**ため混同に注意すること。

| 関数名（`libadvancelib.so`） | 標準規格相当 | 初期値 | 方向 | 多項式（反転表記） | 最終 XOR | テストベクタ (`"123456789"`) | 主な用途 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `ANSI_CRC_16_sh` | **CRC-16/MCRF4XX** | `0xFFFF` | LSB-first | **`0x8408`** (`0x1021` 反転) | `0x0000` | **`0x6F91`** | 2768B 設定構造体 (`record_ultimatebt2`) の整合性チェック（`+0x0C` 格納） |
| `crc16` | **CRC-16/MODBUS** | `0xFFFF` | LSB-first | **`0xA001`** (`0x8005` 反転) | `0x0000` | **`0x4B37`** | パケットヘッダ `+0x06` (`crc16` フィールド) および Bootloader 書込データ検証 |

### 5.1 設定構造体用 `ANSI_CRC_16_sh` (`0x8408`)

`record_ultimatebt2`（全長 `0x0AD0` = 2768 バイト）の書き込み・マクロ更新前にバッファ全体に対して計算し、計算結果を `record_ultimatebt2[0x0C..0x10]` (`u32 LE`) に格納する。入力長 `0` の場合は `0xFFFF` を返す。

```rust
/// 設定構造体用 CRC-16 (libadvancelib.so::ANSI_CRC_16_sh / CRC-16/MCRF4XX)
/// テストベクタ: ansi_crc16_sh(b"123456789") == 0x6F91
pub fn ansi_crc16_sh(data: &[u8]) -> u16 {
    if data.is_empty() {
        return 0xFFFF;
    }
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= b as u16;
        for _ in 0..8 {
            let lsb = crc & 1;
            crc >>= 1;
            if lsb != 0 {
                crc ^= 0x8408;
            }
        }
    }
    crc
}
```

### 5.2 パケット / ブートローダー用 `crc16` (`0xA001`)

- **適用条件（App Mode）**:
  - `libadvancelib_pkt.c`: `gamepad_type == 1` のとき計算し、ヘッダ `+0x06` へ格納する（ただし `cmd == 7 && PID == 0x2072`、および `(cmd & 0xFFFE) == 8 && flag == 0` の場合は計算をスキップ）。
  - Windows `8BitDoAdvance.dll::send_custom_inner`: `PID` が `{0x6006, 0x3010, 0x3011, 0x6007, 0x3109, 0x6009, 0x6012, 0x600F, 0x600B, 0x2062}` に一致する場合に計算しヘッダ `+0x06` へ格納。
- **適用条件（Bootloader Mode）**:
  - `cmd == 0x0002`（バージョン書き込み）および `cmd == 0x0003`（64 バイトファームウェアチャンク書き込み）でペイロードに対する CRC を計算してヘッダ `+0x06` へ格納。
- バイナリ `.rodata` (`0x1d554`) にはルックアップ配列 `[0x0000, 0xA001]` がそのまま配置されている。入力長 `<= 0` の場合は `0xFFFF` を返す。

```rust
/// パケットヘッダ / ブートローダー用 CRC-16 (libadvancelib.so::crc16 / CRC-16/MODBUS)
/// テストベクタ: packet_crc16(b"123456789") == 0x4B37
pub fn packet_crc16(data: &[u8]) -> u16 {
    if data.is_empty() {
        return 0xFFFF;
    }
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= b as u16;
        for _ in 0..8 {
            let lsb = crc & 1;
            crc >>= 1;
            if lsb != 0 {
                crc ^= 0xA001;
            }
        }
    }
    crc
}
```

---

## 6. App Mode 全コマンド一覧とシーケンス

### 6.1 接続・初期化シーケンス

Android 版（`StepActivity` + `GamepadManager` + `Ultimate2AdvanceUI`）および Windows 版に基づく標準初期化シーケンスは次の通り。

```mermaid
sequenceDiagram
    participant Host as PC / スマホ クライアント
    participant Ctrl as Ultimate 2 BT (0x600F / "80ND")

    Host->>Ctrl: BLE 接続 → Service FF10 / Char FF11 発見
    Host->>Ctrl: CCCD (0x2902) に [0x01, 0x00] 書込 (Notify 有効化)
    Host->>Ctrl: MTU 要求 (100 bytes)
    Host->>Ctrl: cmd=0x000B (ReadVersion, total_len=0x01020030)
    Ctrl-->>Host: Version 応答 (gamepadVersion)
    Host->>Ctrl: cmd=0x0010, subcmd=0x0001 (setPhonePlatform)
    Ctrl-->>Host: ACK
    Host->>Ctrl: cmd=0x0007, subcmd=0x0001 (isReportEnable(1), MAX_ATT_MTU=0x3F)
    Note over Host,Ctrl: Windows USB では cmd=0x0013 (StartConfig) を実行
    Host->>Ctrl: cmd=0x0011, subcmd=state (setConfigState)
    Ctrl-->>Host: 非同期通知 cmd=0x0012 (allowConfigDone)
    Host->>Ctrl: cmd=0x0002 (offset=0, len=0x74 ヘッダ / offset=0x12 スロット / len=0x0AD0 全設定)
    Ctrl-->>Host: 分割応答パケット列 (total_len=0x0AD0 等)
    Host->>Ctrl: cmd=0x0102 (必要スロットのマクロ詳細読み出し)
    Ctrl-->>Host: マクロステップ配列 (1ステップ = 10B)
```

### 6.2 App Mode コマンド一覧

送信時は先頭 `0x04` プレフィックス + 16 バイト `cumstom_header_t` + ペイロード。

| `cmd` | `subcmd_or_param` | `total_len` / `offset` | 方向 | 機能・用途 |
| --- | --- | --- | --- | --- |
| **`0x0001`** | `0x0000` | `total_len = 0x0AD0`（常に全体長）、`chunk_len` = このパケットの実データ長（例: 部分書込 `0x74` / `0x0C`）、`offset` = 構造体内オフセット | Host → Ctrl | **設定構造体書き込み**（全体 `2768B` または各項目差分書き込み。部分書込でも `total_len` は `0x0AD0` のまま。下記 6.3 参照） |
| **`0x0002`** | `0x0000` | 全体: `offset=0, total_len=0x0AD0`<br/>ヘッダ: `offset=0, total_len=0x74`<br/>現在スロット: `offset=0x12, total_len=2` | Host → Ctrl | **設定構造体読み出し**。応答は `cumstom_header_t` に続き要求データ片が返る |
| **`0x0003`** | `0x0000` | `total_len=0x0C`, `offset=0x0C` | Ctrl ↔ Host | **リアルタイム入力レポート取得 / 通知**（12 バイト、セクション 7 参照） |
| **`0x0006`** | **`0x0067`** (BLE)<br/>**`0x0123`** (Windows USB) | `total_len=0, offset=0` | Host → Ctrl | **Save（Flash 永続化）**。書き込み系（`0x0001` / `0x0103`）完了後に必ず発行。**BLE(Android) と USB(Windows) でサブコマンドが異なる点に注意**（6.4 参照） |
| **`0x0007`** | `0x0000` (無効) / `0x0001` (有効) | `total_len=0, offset=0` | Host → Ctrl | **`SetReportEnable`**。リアルタイム入力通知の開始/停止。発行時にクライアント側 `MAX_ATT_MTU = 0x3F` へ設定 |
| **`0x000B`** | `0x0000` | `total_len = 0x01020030` (`gamepad_type==2` 時。`offset` にも同値) | Host → Ctrl | **`ReadVersion`**。ファームウェアバージョン読み出し |
| **`0x000C`** | **`0x0067`** | `total_len=4, offset=4` | Host → Ctrl | **`EnterBoot`**。App Mode から Bootloader Mode (`"8bt"`) へ再起動遷移 |
| **`0x000E`** | — | — | Ctrl → Host | エラー応答通知（`readBleHidData` でエラーコードログ出力） |
| **`0x000F`** | **`0x000F`** | `total_len=0, offset=4` | Host → Ctrl | **`DisConnectBluetooth`**。Bluetooth 設定接続の明示的切断 |
| **`0x0010`** | **`0x0001`** | `total_len=0, offset=0` | Host → Ctrl | **`SetPhonePlatform`**。UT2 接続開始時 (`enterUT2`) にホスト種別を通知 |
| **`0x0011`** | `state` (`int`) | `total_len=0, offset=0` | Host → Ctrl | **`SetConfigState`**。設定状態の遷移通知（`Channel+0x31 = 0x0A`） |
| **`0x0012`** | `0x0000` | — | Ctrl → Host | **`AllowConfigDone`**。コントローラが設定受付可能になった際の非同期通知 |
| **`0x0013`** | `0x0000` | `total_len=0, offset=0` (Windows `StartConfig` flag=`1`) | Host → Ctrl | **Windows USB HID 用 `StartConfig`**（最大 30 回リトライし `cmd=0x13` ACK を待機） |
| **`0x0015`** | `slot` (`0..2`) | `total_len=0, offset=0` | Host → Ctrl | **`WriteSLOT`**。アクティブプロファイルスロット切替 |
| **`0x0102`** | `(mode << 8) \| slot` | `total_len = steps * 10`, `offset = macro_idx << 12` (`macro_idx * 0x1000`) | Host → Ctrl | **`ReadBT2Macro`**。指定スロット・マクロ番号のステップ列を読み出し（1 ステップ = 10B） |
| **`0x0103`** | `(mode << 8) \| slot` | `total_len = align32(steps * 10)` (`((steps * 10) + 0x1F) & ~0x1F`), `offset = (macro_idx << 12) + chunk_index * chunk_size`（`chunk_index` はパケットごとに進む。`chunk_size` は BLE `0x40` / USB HID `0x20`、または交渉済みサイズ。上限超過は拒否またはクランプ） | Host → Ctrl | **`WriteBT2Macro`**。BLE 最大 **`0x40` (64B)** / USB HID 最大 **`0x20` (32B)** 分割でマクロを書き込み、完了後に `0x0006` (Save) を実行。`chunk_size` が上限を超える場合は拒否または上限へクランプし、上限超過をテストする |
| **`0x0104`** | `(mode << 8) \| slot` | `total_len = 0x1000`, `chunk_len = 0x1000`（wire header `+0x04`。公式実装のチャネル設定値。ペイロードなし `len = 0`）, `offset = macro_idx << 12` | Host → Ctrl | **`EraseBT2Macro`**。指定スロット・マクロ領域（4KB 単位）を消去 |

> ※ `mode` は `record_ultimatebt2` 内オフセット `+0x10` の `GamepadMode` 値。
>
> **実機ソースでの再確認（x86_64 `libadvancelib.so`）**: 設定読み出しは **`cmd=0x0002` / `subcmd=0x0000`**、設定書き込みは **`cmd=0x0001` / `subcmd=0x0000`**。`STRUCT_BT2_2768B.md` 旧版にあった `subcmd=0x0101` / `0x0102` は誤記であり、`getUT2_ReadCofigChannel` / `getBT2_WriteCofigChannel` のチャネル構造体 `+0x4E` はどちらも `0` である。

### 6.3 Ultimate 2 Bluetooth 設定データ構造体 (`record_ultimatebt2`, `0x0AD0` = 2768 バイト) 部分書き込みオフセット一覧

`cmd = 0x0001` は構造体全体 (`0x0AD0` B) の一括送信だけでなく、機能別の部分オフセット書き込みにも対応している。部分書込でも **`total_len` は常に `0x0AD0`** で、部分的なデータ長は `chunk_len` に載せる。いずれの場合も事前に `ANSI_CRC_16_sh(buf, 0x0AD0)` を計算し `+0x0C` へ格納する。

| 機能 / セッター (`libadvancelib.so`) | 先頭オフセット | サイズ | 備考 |
| --- | --- | --- | --- |
| ヘッダ / CRC / モード / 現スロット | `0x0000` .. `0x0073` | `0x74` (116B) | `+0x0C`: `u32` `ANSI_CRC_16_sh`, `+0x10`: `u16/u32` `GamepadMode`, `+0x12`: `u16` `Curslot` |
| プロファイル名 (`getBT2_WriteFileName`) | `0x0014 + slot * 0x20` | `0x20` (32B) | スロット別プロファイル名（UTF-16BE、詳細は [STRUCT_BT2_2768B.md](STRUCT_BT2_2768B.md)） |
| 振動 (`getBT2_WriteRumble`) | `0x0074 + slot * 0x0C` | `0x0C` (12B) | スロット別バイブレーション設定 |
| スティック (`getBT2_WriteJoyStick`) | `0x0098 + slot * 8` | `0x08` (8B) | デッドゾーン / カーブ等 |
| トリガー (`getBT2_WriteTrigger`) | `0x00B0 + slot * 8` | `0x08` (8B) | L2/R2 デッドゾーン・範囲等 |
| 特殊設定 (`getBT2_WriteSpecial`) | `0x00C8 + slot * 8` | `0x08` (8B) | 各種トグル設定 |
| キーマッピング (`getBT2_WriteMapKey`) | `0x00E4 + slot * 0x5C + key_idx * 4` | `0x04` (4B) | `total_len` 指定には `0x0AD0` 併用、スロットあたり `0x5C` = 92B |
| マクロ要約 (`getBT2_WriteMacro(slot)`) | `0x068C + slot * 0xD8` | `0xD8` (216B) | スロット内 4 マクロ × `0x34` (52B)。各マクロの `+0x22`（構造体 `0x06B6 + slot*0xD8 + macro*0x34`）に `u16` ステップ数 |
| ジャイロ / 6軸 (`getBT2_WriteSixAxis`) | `0x092C + slot * 0x0C` | `0x0C` (12B) | 6 軸設定（※ Android 版 `libadvancelib.so` には `offset=0` を送るバグがあり、Windows 版の `0x092C + slot * 0x0C`・`total_len=0x0AD0` が正規） |
| ダイナミック照明 (`getBT2_WriteDyNamic`) | `0x0950 + slot * 0x0C` | `0x0C` (12B) | Fire Ring LED 動的テーマ |
| ホットホイール照明 (`getBT2_WriteHotWheel`) | `0x0974 + slot * 0x10` | `0x10` (16B) | カラーホイール等 |
| シングル照明 (`getBT2_WriteSingle`) | `0x09A4 + slot * 0x64` | `0x64` (100B) | 単色・24 LED カラー配列 |

- **マクロ詳細ステップ (`MacroDatas`, 1 ステップ = 10 バイト)**:
  - `S_UltimateBT2Advance.setMacroStruct` より、1 ステップは以下の `5 x u16` (Little-Endian, 計 10 バイト) で構成される:
    - `+0x00` (`u16`): `msTimes`（保持/待機時間 ms）
    - `+0x02` (`u16`): `keys`（ボタン入力ビットまたはマップキーID）
    - `+0x04` (`u16`): `triggerValue`（トリガー入力値）
    - `+0x06` (`u16`): `leftJoy`（左スティック座標パック値）
    - `+0x08` (`u16`): `rightJoy`（右スティック座標パック値）

### 6.4 USB HID と BLE での Save サブコマンド差異

- **BLE (Android `getSaveCustomConfigChannel`)**: `cmd = 0x0006`, **`subcmd = 0x0067`** を使用。BLE の `Save` タイムアウトは仕様書に記載がないため、transport の `wait_ack` 既定値とリトライ方針を実装で記録する。
- **Windows USB HID (`8BitDoAdvance.dll::saveUltimateBT2`)**: `cmd = 0x0006`, **`subcmd = 0x0123`** を使用（**USB HID の Save タイムアウトは `3` 秒**）。
- いずれも応答で `cmd == 0x0006` と対応する `subcmd` の ACK が返る。プラットフォームやトランスポートに応じて適切な `subcmd` を選択すること（誤った `subcmd` では Flash 保存が確定しない可能性がある）。**3 秒は Windows USB HID の値であり、BLE に適用しない。**

---

## 7. リアルタイムコントローラ入力ストリーム（マクロ録音・ボタンテスト用）

### 7.1 ストリームの有効化 / 無効化

- **開始**: `cmd = 0x0007`, `subcmd = 0x0001` (`SetReportEnable(1)`) を送信する。この際、クライアント内部の `MAX_ATT_MTU` は `0x3F` (63) に切り替えられる。
- **停止**: `cmd = 0x0007`, `subcmd = 0x0000` (`SetReportEnable(0)`) を送信する。
- **手動ポーリング**: `cmd = 0x0003`, `subcmd = 0x0000`, `total_len = 0x0C`, `offset = 0x0C` (`readKeysAndJoy`) でも同一フォーマットの 12 バイトを取得できる。
- 有効化中、コントローラ側に入力変化があると `cmd == 0x0003`（または `subcmd == 0x0003`）の 12 バイト通知パケットが Notify で連続送信される。

### 7.2 12 バイト入力レポートの完全バイト定義 (`StepActivity.readKeyWith`)

受信ペイロード（12 バイト、Little-Endian）の各フィールド定義は以下の通り。

| オフセット | サイズ | 型 | フィールド | 計算式・値域・補足 |
| --- | --- | --- | --- | --- |
| `0x00..0x03` | 4B | `u32 LE` | **`Key`** | 32-bit ボタン押下ビットマスク（下表 7.3 参照） |
| `0x04` | 1B | `u8` | **`LeftStick_X`** | 中心値 `127` / `128`。アプリ側正規化値 = `val - 127`（※ `LX` と `LY` が共に `{127, 128}` なら `(0, 0)` とみなす不感帯判定あり） |
| `0x05` | 1B | `u8` | **`LeftStick_Y`** | 中心値 `127` / `128`。アプリ側正規化値 = `val - 127` |
| `0x06` | 1B | `u8` | **`RightStick_X`** | 中心値 `127` / `128`。アプリ側正規化値 = `val - 127`（※ `RX` と `RY` が共に `{127, 128}` なら `(0, 0)` とみなす） |
| `0x07` | 1B | `u8` | **`RightStick_Y`** | 中心値 `127` / `128`。アプリ側正規化値 = `val - 127` |
| `0x08` | 1B | `u8` | **`Trigger_R`** | アナログ右トリガー値 (`0..255`) ※ `is3E` / `is3Xbox` 機種のみここが `Trigger_L` となる |
| `0x09` | 1B | `u8` | **`Trigger_L`** | アナログ左トリガー値 (`0..255`) ※ `is3E` / `is3Xbox` 機種のみここが `Trigger_R` となる |
| `0x0A` | 1B | `u8` | **`Battery`** | バッテリー残量値（`!is3E()` で有効） |
| `0x0B` | 1B | `u8` | `Reserved` | 予約 / 未使用（`0x00`） |

### 7.3 `Key` ビットマスク (`u32 LE`, `KeyMap`)

| ビットマスク (Hex) | ビット位置 | ボタン名 (`KeyMap_*`) | ビットマスク (Hex) | ビット位置 | ボタン名 (`KeyMap_*`) |
| --- | --- | --- | --- | --- | --- |
| `0x0000_0001` | bit 0 | `Start` (`+`) | `0x0001_0000` | bit 16 | `Turbo` |
| `0x0000_0002` | bit 1 | `L3` | `0x0002_0000` | bit 17 | `Home` |
| `0x0000_0004` | bit 2 | `R3` | `0x0004_0000` | bit 18 | `BT_CON` |
| `0x0000_0008` | bit 3 | `Select` (`-`) | `0x0020_0000` | bit 21 | `P3` |
| `0x0000_0010` | bit 4 | `X` | `0x0040_0000` | bit 22 | `ScreenShot` |
| `0x0000_0020` | bit 5 | `Y` | `0x0080_0000` | bit 23 | `Turbo_3` |
| `0x0000_0040` | bit 6 | `Right` (D-Pad) | `0x0100_0000` | bit 24 | `Turbo_Auto` |
| `0x0000_0080` | bit 7 | `Left` (D-Pad) | `0x0200_0000` | bit 25 | `P1` (背面パドル) |
| `0x0000_0100` | bit 8 | `Down` (D-Pad) | `0x0400_0000` | bit 26 | `P2` (背面パドル) |
| `0x0000_0200` | bit 9 | `Up` (D-Pad) | `0x0800_0000` | bit 27 | `Swap` |
| `0x0000_0400` | bit 10 | `L1` | `0x4000_0000` | bit 30 | `P4` |
| `0x0000_0800` | bit 11 | `R1` | — | — | — |
| `0x0000_1000` | bit 12 | `B` | — | — | — |
| `0x0000_2000` | bit 13 | `A` | — | — | — |
| `0x0000_4000` | bit 14 | `L2` | — | — | — |
| `0x0000_8000` | bit 15 | `R2` | — | — | — |

---

## 8. BLE OTA / USB ファームウェアアップデート仕様

### 8.1 8BitDo ファームウェア配信 API (`HttpsUtils` / Windows `ReadHttp`)

#### ベース URL
- **英語圏・グローバル (`en`)**: `https://support.8bitdo.com/restapi/link/v1/client`
- **中国語圏 / デフォルト (`zh`)**: `https://support.8bitdo.cn/restapi/link/v1/client`
- *(Windows クライアント内別エンドポイント参照例: `dl.8bitdo.com:8080`, `dl.8bitdo.cn:8080`)*

#### ファームウェア一覧・最新情報取得エンドポイント
- **メソッド / パス**:
  `POST {base_url}/firmware/select?firmware_type={type}&beta={beta}&soft_type=5`
  - **`firmware_type`**: **`97`** (`0x61`, Ultimate 2 Bluetooth `0x600F` / `XInput=2` / `BootPID=0x600F`)、`98` (レシーバ `0x6011`)、`94` (Ultimate 2 Wireless `0x6012`)、`95` (Adapter `0x6013`)、`108` / `96` (Ultimate 2C BT `0x301A`)
  - **`beta`**: `1`（ベータ版含む、または設定依存で `0`/`1`）
  - **`soft_type`**: `5`（Ultimate Software V2 系クライアント識別子）
- **HTTP ヘッダ & ボディ**:
  - ヘッダ: **`Content_Type: application/x-www-form-urlencoded`**（※ 実装上アンダースコア `_` 表記がハードコードされている）
  - ボディ: 空の FormBody
- **レスポンス JSON 構造**:
  `{"data": "[{...}]"}`（`data` 文字列内に JSON 配列がシリアライズされているため 2 段階パースが必要）
  - **`Firmware` オブジェクト主要フィールド**:
    - `beta` (`int`), `m_beta` (`int`)
    - `created_at` (`string`)
    - `exists` (`int`/`bool`)
    - `fileName` (`string`)
    - `fileSize` (`long`)
    - `fileURL` (`string`, ダウンロード URL。Windows では `/firmware/download?firmware_key=...` 形式も併用)
    - `file_version` (`string`), `ic_version` (`string`)
    - `firmware_key` (`string`)
    - `firmware_type` (`int`)
    - `md5` (`string`)
    - `readme` (`string`, 中国語), `readme_en` (`string`, 英語)

> **Android 版 MD5 検証クセ（実装上の注意）**:  
> `HttpsUtils$2`（`getDownloadFirwamre`）では、レスポンスの `firmware.md5` を **ダウンロードしたファイル本体バイト列の MD5 ではなく、`String.valueOf(contentLength)` の MD5**（`Const.getMD5(String.valueOf(contentLength))`）と比較している箇所がある。サードパーティ製アップデータを自作する際は、API の `md5` 値が「実ファイルバイナリの MD5」か「`fileSize` 文字列の MD5」かを実レスポンスで確認すること（実ファイル整合性は `.dat` 内部ヘッダ＋Bootloader 書込時 CRC-16 でも保護される）。

### 8.2 ファームウェアバイナリ (`.dat`) 構造と OTA シーケンス

ダウンロードした `.dat` ファイルは先頭 **`0x1C` (28 バイト) のイメージヘッダ** + 実データ部で構成される（マルチセクションイメージでは後続セクションに別の 28B ヘッダが続く）。

- **先頭 28 バイトイメージヘッダ (`header[0..0x1C]`)**:
  - `+0x00` (`u32`): `firmwareFile_Version`（ファームウェアバージョン値）
  - `+0x04` (`u32`): `load_base_addr`（書き込み先 Flash ベースアドレス）
  - `+0x08` (`u32`): `image_data_len`（このセクションの有効イメージサイズ）
  - `+0x0C..0x17`: 付加メタデータ（`WriteVersion` `cmd=0x00C4` では `+0x00..0x18` の 24 バイトを送付）

```mermaid
sequenceDiagram
    participant Host as クライアント
    participant App as Controller (App Mode "80ND")
    participant Boot as Controller (Bootloader "8bt")

    Host->>App: cmd=0x000C, subcmd=0x0067 (enterBoot)
    App-->>Host: ACK / BLE 切断
    Host->>Boot: BLE スキャン ("8bt") → Service FF20 / Char FF21 接続 + CCCD + MTU
    Host->>Boot: prefix=0x05, cmd=0x0008, subcmd=0x0067 (readPID, total_len=2, 5s)
    Boot-->>Host: PID 応答 (0x600F)
    Host->>Boot: prefix=0x05, cmd=0x0005, subcmd=0x0067 (readVersionInBoot)
    Boot-->>Host: Boot Version 応答
    loop 4KB セクタ単位 (offset = 0x0000, 0x1000, ...)
        Host->>Boot: prefix=0x05, cmd=0x0004 (EraseFlash, addr=base+offset, total_len=0x1000, timeout=10s)
        Boot-->>Host: Erase 完了 ACK
        loop 64B チャンク単位 (writeoffset = 0x0000 .. 0x0FC0 step 0x40)
            Host->>Boot: prefix=0x05, cmd=0x0003 (WriteFirmware 64B + CRC-16/MODBUS, timeout=1s)
            Boot-->>Host: Chunk ACK
        end
    end
    Host->>Boot: prefix=0x05, cmd=0x00C4 (WriteVersion 24B ヘッダ, timeout=3s)
    Host->>Boot: prefix=0x05, cmd=0x0002 (WriteFirmwareVersion 4B + CRC16, offset=0x01017040)
    Note over Host,Boot: gamepad_type == 1 (0x600F 等) の場合は WriteAPPID をスキップ
    Host->>Boot: prefix=0x05, cmd=0x0006 (jumpApp, offset=0x01018000)
    Note over Boot: App Mode へ再起動完了
```

### 8.3 Bootloader コマンド一覧 (`cmd_packet = 0x05`)

Bootloader Mode (`"8bt"`, Service `0000FF20-...`, Char `0000FF21-...`) では先頭 1 バイトを **`0x05`** とする。

| `cmd` | `subcmd` | `offset` | `total_len` / `chunk_len` | `crc16` | タイムアウト | 機能 (`libadvancelib_boot.c`) |
| --- | --- | --- | --- | --- | --- | --- |
| **`0x0002`** | `0x0000` | `0x01017040` (`gamepad_type==1`)<br/>または `0x08014034` | `total_len = 4`, `chunk_len = 4` | あり (`crc16`) | 即時 (`0`ms設定) | **`WriteFirmwareVersion`**: `firmwareFile_Version` (`.dat` 先頭 4B) を書き込み |
| **`0x0002`** | `0x0000` | `0x0801401C` | `total_len = 0x0C` (12B) | あり | `5000 ms` | **`WriteAPPID`**: 12B `Device_Serial` を暗号化して書き込み（※ **`gamepad_type == 1` のときスキップ**） |
| **`0x0003`** | `0x0000` | `load_base_addr + sector_off + chunk_off` | `total_len = image_len` (`header+8`), `chunk_len = 0x40` (64B) | **あり** (`0x40`B 分の `crc16`) | `1000 ms` | **`WriteFirmware`**: 64B チャンク単位で Flash 書込。`.dat` 内参照元は `sector_off + chunk_off + 0x1C` |
| **`0x0004`** | `0x0000` | `load_base_addr + sector_off` | `total_len = 0x1000` (4096B) | なし (`0`) | **`10000 ms`** | **`EraseFlash`**: 4KB セクタ単位で Flash 消去 |
| **`0x0005`** | **`0x0067`** | `0x01017040` (`gamepad_type==1`) | `total_len = 4` | なし | `5000 ms` | **`readVersionInBoot`**: Bootloader 内から現ファームウェアバージョン取得 |
| **`0x0006`** | `0x0000` | `0x01018000` (`gamepad_type==1`) | `total_len = 0` | なし | `5000 ms` (送信設定 `0`) | **`jumpApp`**: アプリケーション領域へジャンプして通常モードへ復帰 |
| **`0x0008`** | **`0x0067`** | `0x00000000` | `total_len = 2` | なし | `5000 ms` (5s) | **`readPID` (BLE Boot)**: Bootloader でのデバイス PID 読み出し（※ Windows USB Boot では `cmd=0x0000, subcmd=0x0008`、5s） |
| **`0x00C4`** | `0x0000` | `0x00000000` | `total_len = header[8]` (イメージ総長), `chunk_len = 0x18` (24B) | なし | **`3000 ms`** | **`WriteVersion`**: 24B イメージヘッダメタデータを書き込み |

### 8.4 エラー復旧・ブリック防止の重要設計ポイント

1. **4KB セクタ単位の消去と書込の厳守**:
   - `EraseFlash` (`cmd=0x0004`) は必ず **`0x1000` (4096 バイト) 境界**で実行し、消去タイムアウトを **10 秒 (`10000 ms`)** 確保する。消去完了 ACK を受け取る前に `WriteFirmware` (`cmd=0x0003`) を送ってはならない。
2. **64B 固定チャンクと `crc16` 付与**:
   - `WriteFirmware` は 1 チャンク **`0x40` (64 バイト)** 固定、必ずペイロード 64 バイトに対する **CRC-16/MODBUS (`0xA001`)** をヘッダ `+0x06` に設定する。
3. **2 段階バージョン確定の順序**:
   - 全 4KB セクタ書き込み後、`WriteVersion` (`cmd=0x00C4`) → `WriteFirmwareVersion` (`cmd=0x0002`, `0x01017040`) → （非 `gamepad_type==1` のみ `WriteAPPID`）→ 最後に `jumpApp` (`cmd=0x0006`) の順序を崩さないこと。`WriteFirmwareVersion` 完了前に電源断してもコントローラは Bootloader (`"8bt"`) に留まるため再試行が可能。
4. **`gamepad_type == 1` (`0x600F` 等) での `WriteAPPID` スキップ**:
   - `libadvancelib_boot.c::WriteAPPID` は `gamepad_type == 1` の場合に APPID 暗号化書き込みを明示的にスキップする。`0x600F` に対して不要な APPID 書込を行わないこと。
5. **中断時のリカバリ**:
   - BLE: `EraseFlash` または `WriteFirmware` の途中で切断された場合、コントローラは App 領域が無効なため電源投入後も Bootloader (`"8bt"`) として起動し続ける。クライアントは `"8bt"` へ再接続して `readPID` (`cmd=0x0008`, 5s) → 最初から全セクタ消去・再書込を行うことで復旧できる。
   - USB: BLE の `"8bt"` 再接続は使わない。USB は再列挙（re-enumeration）/再接続で Bootloader に入り直す。ただし USB Boot の TX フレーミング（`EraseFlash` / `WriteFirmware` / `WriteVersion` / `WriteAPPID` / `JumpApp`）は未文書化のため、USB OTA 書込を行う PR は §4.4 の停止規則に従い、`PROTOCOL_SPEC` に証跡を追加するまで実装しない。
