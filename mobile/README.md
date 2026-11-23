# モバイルアプリ（iOS / Android）

**Padbench**（表示名。識別子は `io.github.haruki0420.padbench`）は、
8BitDo Ultimate 2 Bluetooth Controller（BLE `80ND` / PID `0x600F`）を、
Nintendo Switch 接続したまま設定の読み出し・バックアップを行う
**非公式**モバイルクライアントです。

ネイティブ UI（iOS: SwiftUI / Android: Jetpack Compose）は BLE の
スキャン・接続・権限だけを担当し、プロトコル・2768 バイト設定・マクロは
すべて Rust の `eightbitdo-core` / `eightbitdo-protocol` を
UniFFI（`crates/eightbitdo-uniffi`）経由で共有します。

```
SwiftUI / Compose
      │  NativeTransport（CoreBluetooth / BluetoothGatt）
      ▼
crates/eightbitdo-uniffi   … 同期 API + 専用 Tokio runtime
      │  eightbitdo_core::Transport
      ▼
crates/eightbitdo-core     … Session / config / macro
crates/eightbitdo-protocol … フレーム・CRC・構造体
```

変更履歴は [CHANGELOG.md](CHANGELOG.md) を参照してください。

## 現在の到達点（M0 + 読み取り API）

| 項目 | 状態 |
| --- | --- |
| `eightbitdo-uniffi` クレート | 実装済み。`cargo test -p eightbitdo-uniffi` で 14 テスト成功 |
| 受信キュー（Rust / Android / iOS） | bounded（64 フレーム、溢れたら最古を drop）。Rust・Android に単体テストあり |
| UniFFI の Swift / Kotlin バインディング生成 | 実測で成功（`uniffi` 0.31.2） |
| iOS: SwiftUI 雛形 + CoreBluetooth transport | 実装済み |
| iOS: `xtool dev build --ipa`（署名なし IPA） | **成功**（Linux / xtool 1.24.0） |
| Android: Gradle + Compose + GATT transport 雛形 | 実装済み |
| Android: `assembleDebug` / JVM unit test | **成功**（unit test 8 件 + JNI 同梱 APK。NDK r29 で `libeightbitdo_uniffi.so` を arm64-v8a / x86_64 に同梱） |
| Android: JNI 同梱 APK | **成功**（14 MiB、SHA-256 `879e894c…a351f1`、[ビルド証跡](../docs/MOBILE_BUILD_EVIDENCE.md)） |
| 実機での動作確認 | 未実施（実機なし） |
| 設定書き込み・OTA | 未公開（意図的。後述） |

## Rust / UniFFI のバージョン制約（MSRV 1.85）

ワークスペースの MSRV は **Rust 1.85**（`Cargo.toml` の `rust-version`）です。
CI と同じコマンドで確認できます。

```bash
cargo +1.85.0 check --workspace --all-targets --all-features --locked
```

**`uniffi` は 0.32 系ではなく 0.31.2 に固定しています。** 0.32.2 は
`cargo_metadata 0.23.1`（MSRV 1.86）、`askama 0.16.1` / `textwrap 0.16.4`
（MSRV 1.88）、`cargo-platform 0.3.3`（MSRV 1.91）を引き込み、
`--all-features` の MSRV チェックが失敗するためです。0.31.2 は
`cargo_metadata 0.19.2`（1.78）と `askama 0.14.0`（1.83）で 1.85 に収まります
（`textwrap` は 0.16.2 に固定）。

* `crates/eightbitdo-uniffi/Cargo.toml` の `uniffi` と生成バインディング
  （`scripts/mobile/gen-bindings.sh`）は必ず同じバージョンにそろえる。
  bindgen と crate の不一致は実行時クラッシュにつながります。
* 0.32 系へ上げるときは、MSRV 1.85 を維持するなら上記推移依存の
  ダウングレード（`cargo update --precise`）か、bindgen を workspace 外へ
  分離する必要があります。CI の `msrv` ジョブは
  `cargo check --workspace --all-targets --all-features --locked` を実行します。

## UniFFI 公開 API

同じクレートから Swift / Kotlin の両方を生成します。

### Callback interface（ネイティブ側が実装）

```rust
pub trait NativeTransport: Send + Sync {
    fn send(&self, data: Vec<u8>) -> Result<(), NativeTransportError>;
    fn recv(&self, timeout_ms: u32) -> Result<Option<Vec<u8>>, NativeTransportError>;
    fn mtu(&self) -> u16;
    fn disconnect(&self);
}
```

* `recv` は **プレフィックスなし**の通知フレーム（16 バイトヘッダ + payload）を返す。
* `mtu` は**交渉後の実値**。要求値を返さない（Android 14+ は 517 に固定される
  ことがあるため）。
* `disconnect` は冪等で、進行中の `recv` を終わらせること。ネイティブ側の
  切断通知（iOS `didDisconnectPeripheral` / Android `onConnectionStateChange`）
  でも必ず呼ぶ。

### オブジェクト / 関数

| API | 戻り値 | 説明 |
| --- | --- | --- |
| `MobileSession(transport)` | — | 接続済み transport を渡す。MTU・transport 異常で `MobileError` |
| `readDeviceInfo()` | `DeviceInfo` | ファームウェア版（`0x000B`） |
| `readConfig()` | `DeviceConfig` | 2768B 設定 + CRC 状態 + 要約 |
| `readConfigJson()` | `String` | 上記の JSON（`raw_hex` 含む） |
| `readMacroBackup()` | `MacroBackupView` | 3 スロット × 4 マクロの一覧 |
| `readMacroBackupJson()` | `String` | マクロの JSON（step + raw 含む） |
| `setReportEnable(enabled)` | — | リアルタイム入力ストリーム（`0x0007`） |
| `readInputOnce(timeoutMs)` | `InputSnapshot?` | 入力 1 フレーム（`0x0003`） |
| `disconnect()` | — | 受信スレッド停止を含む切断 |
| `describeConfig(raw)` | `ConfigSummary` | 保存済みダンプの表示（I/O なし） |
| `minimumReadMtu()` | `u16` | セッションに必要な最低 ATT MTU（**161**） |
| `prepareConfigWrite(snapshot, edited)` | `ConfigWriteDraft` | **書き込み準備のみ**（差分・CRC 再計算、送信なし・セッション不要） |
| `configWriteChunkLen(mtu)` | `u16` | MTU に対する 1 パケットの payload 長 |

Rust の `Session` は単一飛行（同時操作禁止）なので、`MobileSession` の
全メソッドは内部 Mutex で直列化しています。呼び出しはブロッキングなので
Kotlin は `Dispatchers.IO`、Swift は専用キューから呼んでください。
`disconnect()` は受信スレッドの join を含むため、メインスレッド（ANR 対象）
では呼ばないでください。

### 受信キューと MTU の前提

**受信キューは bounded です。** コントローラは読み出し中にも 20ms 周期の
入力レポートを送るため、無制限キューはメモリを際限なく消費します。

* Rust `FrameQueue`（`BridgeTransport` 内）: 容量 **64 フレーム**、満杯時は
  **最古を drop** してカウント。落ちたフレームは読み出しの失敗（timeout /
  resync）として表面化し、成功扱いにはなりません。**応答フレーム自体が
  落ちる可能性はありますが、その read は必ず失敗します**（部分転送を成功に
  しない）。将来の書き込み経路を配線するときは、任意フレームではなく
  リアルタイム入力レポートだけを drop 対象にすることを検討します。
* Android `FrameQueue`: 同じ容量・同じ drop-oldest 方針。
* iOS `CoreBluetoothTransport`: 同じ容量・同じ drop-oldest 方針。
* 64 は 2768B 設定読み出し（約 20 通知）に余裕を持たせた値です。

**MTU は読み出しの前提条件です。** 設定読み出しの通知は 1 件あたり
`16（ヘッダ）+ 142（payload）= 158` バイトで、ATT の通知ヘッダ 3 バイトを
含めると **ATT MTU 161 以上**が必要です。

* `MobileSession` のコンストラクタは `mtu < 161` を `InvalidRequest` で拒否。
* Android は `requestMtu(200)` の**交渉を必須**にし、失敗・無応答・
  `onMtuChanged` 異常はすべてエラーにします。**23 へのフォールバックはしません**
  （23 のままだと 158 バイト通知を受信できず読み出しが破綻するため）。
* iOS は `maximumWriteValueLength(for:.withoutResponse) + 3` を MTU とし、
  161 未満ならセッションを作らずエラー表示します。

### 設定書き込みを公開していない理由

`eightbitdo-core` には 2768B の書き込み API（`Session::write_full_config`）が
既にありますが、モバイルの UniFFI には**あえて公開していません**。
書き込みは次を満たすまで出しません。

1. 事前バックアップ（2768B + マクロ 3×4）と SHA-256 記録を UI フローに組み込む
2. 差分確認 UI と失敗時の復旧手順（`ConfigWriteStage` / `ConfigWriteState` の表示）
3. AGENTS.md §5 の Tier 3 レビュー

`prepareConfigWrite` はその第一歩です。**デバイス I/O もフレーム送信もせず、
セッションも不要**な純関数で、入力は「スナップショットの 2768 バイト」と
「編集後の 2768 バイト」だけです。返すのは再計算後 CRC、変更バイト範囲、
送信予定バッファで、フラッシュへの書き込み経路には接続していません。
`write_full_config` を FFI に公開するのは上記 1〜3 が揃った後です。

## main の BLE 単一所有（`ble_lock`）との関係

`origin/main` の BLE 単一所有／ロックファイル（`eightbitdo_core::ble_lock`,
`BleOptions` の lock_path）は **GUI / CLI の BLE トランスポート用**です。
モバイルは CoreBluetooth / BluetoothGatt を `NativeTransport` callback として
注入し、Rust コアは BLE スタックに直接触れません（`eightbitdo-core` を
`default-features = false` でビルド）。そのため `BleOptions` の初期化変更は
モバイル側に影響せず、整合確認も不要です。`ble_lock` は「1 プロセス 1 リンク」
を守るためのもので、モバイルでは OS の BLE API が同じ役割を担います。

## iOS（`mobile/ios/`）

### 前提（Linux ホスト）

| 必要物 | 確認方法 |
| --- | --- |
| xtool 1.24.0（バージョン固定を推奨） | `xtool --version` |
| Darwin Swift SDK | `xtool sdk status` → `Darwin SDK is installed` |
| Apple ID ログイン（署名時のみ必須。unsigned IPA は不要） | `xtool auth status` |
| Rust ターゲット `aarch64-apple-ios` | `rustup target add aarch64-apple-ios` |

xtool は Linux 上で SwiftUI アプリをビルドし、**署名なし IPA** まで生成できます。
署名・実機インストール・App Store 配布は後述の制約があります。

### ビルド

```bash
./scripts/mobile/build-ios.sh
```

内部で以下を実行します。

```bash
# 1. UniFFI バインディング（Swift + Kotlin）
./scripts/mobile/gen-bindings.sh
# 2. Rust staticlib（cdylib は Linux のホストリンカで作れないため staticlib のみ）
cargo rustc -p eightbitdo-uniffi --lib --release \
  --target aarch64-apple-ios --crate-type staticlib
cp target/aarch64-apple-ios/release/libeightbitdo_uniffi.a mobile/ios/libs/
# 3. SwiftPM + xtool
cd mobile/ios && xtool dev build -c release --ipa
```

出力: `mobile/ios/xtool/Padbench.ipa`（**署名なし**、実機には入りません）。

### 実測結果（2026-11-23、Padbench 識別子への統一後）

* `xtool dev build --ipa`: `Build complete! (6.96 secs)` → IPA 生成
* IPA: `mobile/ios/xtool/Padbench.ipa`、1,550,798 bytes（SHA-256 `ff98369b…cab7b`）
* `Payload/Padbench.app/Padbench`: `Mach-O 64-bit arm64 executable`
* `llvm-nm -gU` で `_uniffi_eightbitdo_uniffi_fn_constructor_mobilesession_new` など
  UniFFI シンボル 55 個を確認（Rust staticlib が実際にリンクされている）
* `Info.plist`: `CFBundleIdentifier=io.github.haruki0420.padbench`,
  `CFBundleName` / `CFBundleDisplayName=Padbench`,
  `NSBluetoothAlwaysUsageDescription`（"read its settings and enable live
  input"）, `UIBackgroundModes=[bluetooth-central]`, `MinimumOSVersion=17.0`
* Rust staticlib サイズ: 20,899,048 bytes（release、LTO なし）
* 内部名は `xtool.yml` の `product: Padbench` と `Package.swift` の product 名で指定。
  Swift の target / module 名は `EightBitDo` のまま（生成される UniFFI バインディングの
  `import` を変えないため）

> 旧値（`EightBitDo.ipa` 1,550,522 bytes / `CFBundleIdentifier=dev.eightbitdo.mobile`）は
> [docs/MOBILE_BUILD_EVIDENCE.md](../docs/MOBILE_BUILD_EVIDENCE.md) §1〜§2 に当時の記録として残しています。

### 書き込みモード（without-response / with-response）

プロトコルは通常 write-without-response で駆動しますが、FF11/FF13 の中には
with-response しか広告しない実装があります。フレーム自体は同じなので、
両プラットフォームとも特性の `properties` から書き込みモードを選びます。

* without-response が利用可能ならそれを使用（通常経路）。
* 無ければ with-response を使用し、ATT 応答（iOS `didWriteValueFor` /
  Android `onCharacteristicWrite`）を待ってから次のフレームへ進む。
* どちらの書き込みプロパティも無い場合のみ、接続時に明示エラー。
  iOS は MTU 不足時と同じく `cancelPeripheralConnection` でリンクを閉じます。
* with-response の応答待ち（2 秒）がタイムアウトした場合、**成功にはせず
  エラー**にします。iOS は `WriteAckTracker` の世代番号で遅延
  `didWriteValueFor` の誤帰属を防ぎ、タイムアウト後は再接続まで書き込みを
  拒否します（Android も遅延 `onCharacteristicWrite` を次の write に
  帰属させず再接続を要求）。純ロジックは
  `scripts/mobile/test-write-ack-tracker.sh` で検証できます。

### 接続ライフサイクルと transport の責務

`AppModel` が `CBCentralManager` の接続ライフサイクルを所有し、
`CoreBluetoothTransport` は「接続済み 1 リンクの状態」だけを持ちます。

* 接続・切断・再接続の判断は `AppModel`。`CoreBluetoothTransport` は自分から
  接続を開始/停止しません。
* `centralManager(_:didDisconnectPeripheral:error:)` では、まず
  `transport.disconnect()`（`AppModel.closeCurrentSession()` 経由）を呼び、
  保留中の Rust `recv` を即座にエラーで返します。その後セッションを解放します
  （Rust の join は最大で transport の `recv` タイムアウト分ブロックするため
  メインスレッドでは実行しません）。
* `MobileSession(transport:)` が失敗した場合や MTU が 161 未満の場合は、
  同じ経路で transport を閉じます（リンク状態のリーク防止）。
* 再接続時は旧セッションを必ず閉じてから新しいリンクに接続します。

> iOS 側の自動テストは、Linux の Swift ツールチェーンに CoreBluetooth が無い
> ため実行できません。リンク断の契約（`recv` がエラーを返し、セッションが
> `Disconnected` / `Transport` エラーになること）は Rust 側の
> `platform_receive_failure_surfaces_as_a_mobile_error` テストで固定し、
> Swift 側は `xtool dev build --ipa` のリンク検証で代替しています。

### リンク構成

* `RustCore/module.modulemap` + `EightBitdoFFI.h` を `.systemLibrary` として公開
* `Package.swift` は `#filePath` から `mobile/ios/libs` の**絶対パス**を計算し
  `-L` に渡す（xtool は `xtool/.xtool-tmp` に builder パッケージを作るため、
  相対パスでは解決できない）
* `-leightbitdo_uniffi` で staticlib をリンク

### 署名・配布の制約（重要）

| やりたいこと | Linux の xtool だけで可能か | 必要なもの |
| --- | --- | --- |
| 署名なし IPA の生成 | 可能 | — |
| 実機インストール | **不可** | USB 接続した iPhone + `xtool dev build --sign --ipa`（端末登録・証明書・プロファイル生成） |
| App Store / TestFlight 配布 | **不可** | macOS + Xcode / Transporter（または macOS CI ランナー） |

* `bundleID` は Apple Developer Services 上で永続なので、最初に確定させる
  （`mobile/ios/xtool.yml` の `bundleID: io.github.haruki0420.padbench`）。
* 実機では初回起動後に「設定 > 一般 > VPN とデバイス管理」で開発者を信頼する。
* 無料 Apple ID では配布用証明書・App Store 配布はできない。

### バックグラウンド

`UIBackgroundModes = bluetooth-central` を設定済みですが、
**マクロ書き込み・OTA は前景専用**にする方針です。バックグラウンドでは
アプリが suspend され、コアの 2〜3 秒タイマーや ACK 待ちが破綻します。
読み取り（`readConfig` は数秒）も前景で実行してください。

## Android（`mobile/android/`）

### この環境でのビルド結果（実測）

このホストには Android SDK が `/opt/android-sdk` にあり、Gradle 9.8.2 の
ディストリビューションも `~/.gradle` にキャッシュされていたため、
**Kotlin コンパイル・JVM unit test・debug APK 生成まで実測できました**。

| 項目 | 結果 |
| --- | --- |
| ツールチェーン | AGP **9.5.0** / Gradle **9.8.2**（wrapper 同梱）/ Kotlin **2.4.21**（AGP 9 の built-in Kotlin）/ JDK 21 |
| Android SDK | `/opt/android-sdk`（platforms;android-36, build-tools;36.0.0） |
| `./gradlew testDebugUnitTest` | **BUILD SUCCESSFUL**（`FrameQueueTest` 3 + `AppWriteModeTest` 3 + `RustCoreTest` 2 = 8 テスト、失敗 0） |
| `./gradlew assembleDebug` | **BUILD SUCCESSFUL**（`app/build/outputs/apk/debug/app-debug.apk` = 14,551,011 bytes、JNI 同梱） |
| Rust JNI ライブラリ | **同梱済み**（`lib/arm64-v8a/libeightbitdo_uniffi.so` 1,266,288 bytes / `lib/x86_64/...` 1,193,896 bytes。NDK r29 + cargo-ndk 4.1.2） |

注意: 上記は Kotlin / Compose / リソース / manifest のコンパイル検証です。
**2026-11-23 に NDK r29 と cargo-ndk 4.1.2 を導入し、`libeightbitdo_uniffi.so` を
同梱した APK（arm64-v8a / x86_64）のビルドに成功しました**。手順と APK の
SHA-256・ELF・uniffi シンボル数は [docs/MOBILE_BUILD_EVIDENCE.md](../docs/MOBILE_BUILD_EVIDENCE.md)
に記録しています。

AGP 9 では `org.jetbrains.kotlin.android` プラグインを適用できません（built-in
Kotlin が標準）。本プロジェクトは `com.android.application` +
`org.jetbrains.kotlin.plugin.compose` のみを適用しています。

Kotlin-only APK（Rust JNI 非同梱）では、UniFFI の生成コードが最初に
`System.loadLibrary` を呼んだ時点で `UnsatisfiedLinkError`（`Error` であり
`Exception` ではない）が発生します。`RustCore.availability()` が `Throwable` を
捕捉して UI に「NDK ビルド版の APK が必要」と表示し、プロセスは落ちません
（`RustCoreTest` で JVM 上でも検証）。

また UniFFI 0.31.2 が生成する Kotlin のエラークラスは、フィールド名が
`message` だと Kotlin 2.4 で `override val message` と衝突するため、Rust 側の
エラーフィールドは **`detail`** にしています（生成コードは `detail` を保持し
`message` を override します）。バインディングを再生成するときはこの前提を
変えないでください。

### 導入コマンド（Linux）

```bash
# 1. Android command-line tools + SDK/NDK
mkdir -p ~/android-sdk/cmdline-tools
cd ~/android-sdk/cmdline-tools
# https://developer.android.com/studio#command-line-tools-only の linux 版を解凍し
# ディレクトリ名を `latest` にする（~/android-sdk/cmdline-tools/latest/bin/sdkmanager）
yes | ./latest/bin/sdkmanager --licenses
./latest/bin/sdkmanager "platform-tools" "platforms;android-36" \
  "build-tools;36.0.0" "ndk;27.2.12479018"
export ANDROID_HOME="$HOME/android-sdk"

# 2. Rust Android ターゲット + cargo-ndk（Rust JNI を含める場合）
rustup target add aarch64-linux-android x86_64-linux-android
cargo install cargo-ndk --locked
```

Gradle wrapper は同梱済みです（`mobile/android/gradlew`、Gradle 9.8.2、
`gradle-wrapper.properties` に `distributionSha256Sum` を固定）。
グローバルな Gradle は不要です。`gen-bindings.sh` は生成前に
`Cargo.lock` の uniffi と bindgen バイナリのバージョン一致を検証し、
不一致なら停止します。

### ビルド

```bash
export ANDROID_HOME="$HOME/android-sdk"
./scripts/mobile/build-android.sh            # Rust .so 込みの完全ビルド
./scripts/mobile/build-android.sh --kotlin-only  # Kotlin/Compose + unit test のみ（NDK 不要）
```

完全ビルドは `gen-bindings.sh` → `cargo ndk`（`app/src/main/jniLibs`）→
`./gradlew testDebugUnitTest assembleDebug` を実行します。debug 署名なので
配布不可・実機検証用です。`--kotlin-only` は NDK が無い環境での
コンパイル検証用で、APK に Rust JNI ライブラリは入りません。

### 構成

* `minSdk 26` / `targetSdk 36` / `compileSdk 36`
* Compose Material 3、`ViewModel` + `StateFlow`
* `ble/BleScanner.kt`: `80ND` 名 or service `FF10` でフィルタしてスキャン
* `ble/FrameQueue.kt`: bounded 通知キュー（64、溢れたら最古を drop）。
  `src/test` に単体テストあり
* `ble/BluetoothGattTransport.kt`: `NativeTransport` 実装
  * GATT 操作を単一スレッド executor で直列化
  * 通知は `FrameQueue` 経由（drop-oldest、容量 64）
  * CCCD `0x2902` 有効化・MTU 交渉は**厳格**（失敗・無応答はエラー、
    23 へのフォールバックなし）。MTU は `minimumReadMtu()`（161）以上を検証
  * `PROPERTY_NOTIFY` を要求。書き込みは `PROPERTY_WRITE_NO_RESPONSE`
    があれば without-response、無ければ `PROPERTY_WRITE` で with-response を
    選ぶ両対応（`AppWriteMode`）。どちらも無い場合だけエラー。
    with-response は `onCharacteristicWrite` を待って完了
  * `open()` 失敗・切断通知・`disconnect()` のすべてで GATT client を
    `close()` し executor を `shutdownNow()`
* 接続ライフサイクルは `DeviceViewModel` が所有。再接続前に旧セッションを
  `Dispatchers.IO` で閉じ、`onCleared()` では専用スレッドで閉じます
  （同期 FFI をメインスレッドで呼ばない）
* 権限: `BLUETOOTH_SCAN`（`neverForLocation`）/ `BLUETOOTH_CONNECT`
  （Android 12+）、Android 11 以下は `BLUETOOTH` / `BLUETOOTH_ADMIN` /
  `ACCESS_FINE_LOCATION`
* JNA は `net.java.dev.jna:jna:5.17.0@aar`（UniFFI Kotlin バインディングが必須）

### USB OTG（未実装・M3 以降）

Android には interrupt transfer 専用 API がなく
`UsbDeviceConnection.bulkTransfer` を流用する必要があります。動作は端末依存で
保証できないため、まず BLE を主経路にします。実装時は**読み取り専用**から
検証し、書き込みは行わないでください。

## 生成物とコミット方針

以下は**生成物**なのでコミットしません（`.gitignore` 済み）。

```
mobile/ios/RustCore/                        # modulemap + FFI ヘッダ
mobile/ios/Sources/EightBitDo/Generated/    # UniFFI Swift
mobile/ios/libs/                            # libeightbitdo_uniffi.a
mobile/ios/xtool/                           # .ipa とビルド中間物
mobile/android/app/src/main/java/dev/eightbitdo/mobile/core/  # UniFFI Kotlin
mobile/android/app/src/main/jniLibs/        # cargo-ndk の .so
```

公式バイナリ・IPA・APK は従来どおりコミットしません。
バインディングは `scripts/mobile/gen-bindings.sh` でいつでも再生成できます。

例外として **Gradle wrapper**（`mobile/android/gradlew`,
`gradle/wrapper/gradle-wrapper.jar`, `gradle-wrapper.properties`）は
ビルド再現性のためコミットしています（Gradle 9.8.2、Android 公式の慣例）。

## 既知の制限・残課題

1. **実機未検証**。iOS / Android とも、`80ND` への接続・2768B 読み出し・
   マクロ読み出しは実機で確認していない。iOS は `xtool devices` が空。
2. **Android の Rust JNI はビルド済み・実機未検証**。NDK 29.0.14206865 と
   cargo-ndk 4.1.2 を導入し、`libeightbitdo_uniffi.so` を同梱した
   APK（14,551,011 bytes、arm64-v8a / x86_64）の生成に成功しています。
   JNI ロードの実機/エミュレータ確認は未実施です。
3. **設定書き込み・OTA は未公開**。`prepareConfigWrite` まで。
4. **バックグラウンド書き込み不可**（設計上、前景専用）。
5. **MTU 161 以上が必須**。交渉に失敗した場合は明確なエラーで停止します
   （23 へのフォールバックはしません）。
6. **iOS の MTU** は `maximumWriteValueLength(for: .withoutResponse) + 3` で
   推定する（Apple は ATT MTU を直接公開しない）。
7. **UniFFI のバージョン**は **0.31.2** に固定（MSRV 1.85 のため。前述）。
   bindgen と crate の不一致は実行時クラッシュにつながるため、
   `Cargo.toml` と生成スクリプトを同時に更新する。
8. **App Store 審査**は BLE 利用目的の説明とプライバシーマニフェストが必要。
   カスタムハードウェア連携アプリとして審査可能だが、通過を保証しない。
9. 商標・ライセンス表記はリポジトリ方針（非公式であることの明示）に従う。

## ライセンス

* このリポジトリ: `MIT OR Apache-2.0`
* `uniffi` クレート: **MPL-2.0**（許容。MPL はファイル単位の弱いコピーレフトで、
  Rust 側のリンクに影響しない）
