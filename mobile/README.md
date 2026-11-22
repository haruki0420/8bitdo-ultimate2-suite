# モバイルアプリ（iOS / Android）

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

## 現在の到達点（M0 + 読み取り API）

| 項目 | 状態 |
| --- | --- |
| `eightbitdo-uniffi` クレート | 実装済み。`cargo test -p eightbitdo-uniffi` で 7 テスト成功 |
| UniFFI の Swift / Kotlin バインディング生成 | 実測で成功（`uniffi` 0.32.2） |
| iOS: SwiftUI 雛形 + CoreBluetooth transport | 実装済み |
| iOS: `xtool dev build --ipa`（署名なし IPA） | **成功**（Linux / xtool 1.24.0） |
| Android: Gradle + Compose + GATT transport 雛形 | 実装済み。SDK/NDK が無いため**ビルド未実施** |
| Android: APK ビルド | 未実施（下記の導入手順が必要） |
| 実機での動作確認 | 未実施（実機なし） |
| 設定書き込み・OTA | 未公開（意図的。後述） |

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
* `disconnect` は冪等で、進行中の `recv` を終わらせること。

### オブジェクト / 関数

| API | 戻り値 | 説明 |
| --- | --- | --- |
| `MobileSession(transport)` | — | 接続済み transport を渡す。失敗時は `MobileError` |
| `readDeviceInfo()` | `DeviceInfo` | ファームウェア版（`0x000B`） |
| `readConfig()` | `DeviceConfig` | 2768B 設定 + CRC 状態 + 要約 |
| `readConfigJson()` | `String` | 上記の JSON（`raw_hex` 含む） |
| `readMacroBackup()` | `MacroBackupView` | 3 スロット × 4 マクロの一覧 |
| `readMacroBackupJson()` | `String` | マクロの JSON（step + raw 含む） |
| `setReportEnable(enabled)` | — | リアルタイム入力ストリーム（`0x0007`） |
| `readInputOnce(timeoutMs)` | `InputSnapshot?` | 入力 1 フレーム（`0x0003`） |
| `disconnect()` | — | 受信スレッド停止を含む切断 |
| `describeConfig(raw)` | `ConfigSummary` | 保存済みダンプの表示（I/O なし） |
| `prepareConfigWrite(snapshot, edited)` | `ConfigWriteDraft` | **書き込み準備のみ**（差分・CRC 再計算、送信なし） |
| `configWriteChunkLen(mtu)` | `u16` | MTU に対する 1 パケットの payload 長 |

Rust の `Session` は単一飛行（同時操作禁止）なので、`MobileSession` の
全メソッドは内部 Mutex で直列化しています。呼び出しはブロッキングなので
Kotlin は `Dispatchers.IO`、Swift は専用キューから呼んでください。

### 設定書き込みを公開していない理由

`eightbitdo-core` には 2768B の書き込み API（`Session::write_full_config`）が
既にありますが、モバイルの UniFFI には**あえて公開していません**。
書き込みは次を満たすまで出しません。

1. 事前バックアップ（2768B + マクロ 3×4）と SHA-256 記録を UI フローに組み込む
2. 差分確認 UI と失敗時の復旧手順（`ConfigWriteStage` / `ConfigWriteState` の表示）
3. AGENTS.md §5 の Tier 3 レビュー

`prepareConfigWrite` はその第一歩で、I/O なし・送信なしで
「どのバイトが変わるか」「再計算後 CRC」だけを返します。

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

出力: `mobile/ios/xtool/EightBitDo.ipa`（**署名なし**、実機には入りません）。

### 実測結果（2026-11-23、この環境）

* `xtool dev build --ipa`: `Build complete! (7.10 secs)` → IPA 生成
* IPA サイズ: 1,548,492 bytes
* `Payload/EightBitDo.app/EightBitDo`: `Mach-O 64-bit arm64 executable`
* `llvm-nm` で `_uniffi_eightbitdo_uniffi_fn_constructor_mobilesession_new` など
  UniFFI シンボル 59 個を確認（Rust staticlib が実際にリンクされている）
* `Info.plist`: `CFBundleIdentifier=dev.eightbitdo.mobile`,
  `NSBluetoothAlwaysUsageDescription`, `UIBackgroundModes=[bluetooth-central]`,
  `MinimumOSVersion=17.0`
* Rust staticlib サイズ: 20,941,368 bytes（release、LTO なし）

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
  （`mobile/ios/xtool.yml` の `bundleID: dev.eightbitdo.mobile`）。
* 実機では初回起動後に「設定 > 一般 > VPN とデバイス管理」で開発者を信頼する。
* 無料 Apple ID では配布用証明書・App Store 配布はできない。

### バックグラウンド

`UIBackgroundModes = bluetooth-central` を設定済みですが、
**マクロ書き込み・OTA は前景専用**にする方針です。バックグラウンドでは
アプリが suspend され、コアの 2〜3 秒タイマーや ACK 待ちが破綻します。
読み取り（`readConfig` は数秒）も前景で実行してください。

## Android（`mobile/android/`）

### この環境ではビルドしていません

このホストには Android SDK / NDK / Gradle がありません
（`sdkmanager` / `gradle` / `kotlinc` 不在。`adb` のみ）。
**重い SDK を勝手に導入しない**方針のため、APK ビルドは未実施です。
コードと Gradle 構成は置いてあり、下記の導入後に `assembleDebug` できます。

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

# 2. Rust Android ターゲット + cargo-ndk
rustup target add aarch64-linux-android x86_64-linux-android
cargo install cargo-ndk --locked

# 3. Gradle（wrapper jar はコミットしていないので一度だけ生成する）
cd mobile/android
gradle wrapper --gradle-version 8.14.3
```

### ビルド

```bash
export ANDROID_HOME="$HOME/android-sdk"
./scripts/mobile/build-android.sh
```

内部で `gen-bindings.sh` → `cargo ndk`（`app/src/main/jniLibs`）→
`./gradlew assembleDebug` を実行します。debug 署名なので配布不可・実機検証用です。

### 構成

* `minSdk 26` / `targetSdk 36` / `compileSdk 36`
* Compose Material 3、`ViewModel` + `StateFlow`
* `ble/BleScanner.kt`: `80ND` 名 or service `FF10` でフィルタしてスキャン
* `ble/BluetoothGattTransport.kt`: `NativeTransport` 実装（GATT 操作を
  単一スレッド executor で直列化、通知は `LinkedBlockingQueue`、
  CCCD `0x2902` 有効化、`requestMtu(200)` は**実測値**だけ採用）
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

## 既知の制限・残課題

1. **実機未検証**。iOS / Android とも、`80ND` への接続・2768B 読み出し・
   マクロ読み出しは実機で確認していない。iOS は `xtool devices` が空。
2. **Android APK 未ビルド**。SDK/NDK 導入が必要（上記）。
3. **設定書き込み・OTA は未公開**。`prepareConfigWrite` まで。
4. **バックグラウンド書き込み不可**（設計上、前景専用）。
5. **iOS の MTU** は `maximumWriteValueLength(for: .withoutResponse) + 3` で
   推定する（Apple は ATT MTU を直接公開しない）。
6. **UniFFI のバージョン**は `0.32.2` に固定。bindgen と crate の不一致は
   実行時クラッシュにつながるため、`Cargo.toml` と生成スクリプトを同時に更新する。
7. **App Store 審査**は BLE 利用目的の説明とプライバシーマニフェストが必要。
   カスタムハードウェア連携アプリとして審査可能だが、通過を保証しない。
8. 商標・ライセンス表記はリポジトリ方針（非公式であることの明示）に従う。

## ライセンス

* このリポジトリ: `MIT OR Apache-2.0`
* `uniffi` クレート: **MPL-2.0**（許容。MPL はファイル単位の弱いコピーレフトで、
  Rust 側のリンクに影響しない）
