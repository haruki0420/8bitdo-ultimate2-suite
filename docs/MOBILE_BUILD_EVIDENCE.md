# モバイル成果物のビルド検証（2026-11-23）

- 対象 main: `c169392`
- ホスト: Arch Linux x86_64、rustc/cargo 1.100.0、JDK 21.0.13、Android SDK `/opt/android-sdk`
- この文書は **ビルド・リンク・パッケージ内容の確認** の記録です。実機 BLE の動作確認は含みません。
- **注（2026-11-23）**: §1〜§2 の識別子は表示名・識別子を Padbench に統一する前の実測値です。以下の値は当時の記録として保持します。**統一後の再ビルド結果は §0**（`applicationId` / `bundleID` = `io.github.haruki0420.padbench`、Android の `namespace` / iOS の内部名も Padbench）。

## 0. 2026-11-23 再ビルド（識別子を Padbench に統一した後）

`apps/desktop` の Tauri 移行 PR で、Android の `namespace` / `applicationId` と iOS の `bundleID` / 内部名（`xtool.yml` の `product`、SwiftPM の product）を `io.github.haruki0420.padbench` / `Padbench` に変更しました。同一ホストで再ビルドした結果です。

- **ビルド元**: 作業ツリー（`feat/tauri-gui`）。Android の `namespace` / `applicationId` と iOS の `bundleID` などの識別子変更は `5d9c75c`、iOS の内部名（`Package.swift` / `xtool.yml`）と README は `ba515cf`。Rust コードは両コミットで不変
- **ビルド日時**: 2026-11-23 15:02 JST（Android / iOS とも）
- **ホスト**: Arch Linux x86_64、rustc/cargo 1.100.0、JDK 21.0.13、Android SDK `/opt/android-sdk`（NDK 29.0.14206865）、xtool `/usr/bin/xtool`

### 0.1 Android（JNI 同梱 APK）

| 項目 | 値 |
| --- | --- |
| NDK | `29.0.14206865`（`/opt/android-sdk/ndk`） |
| コマンド | `PATH="$HOME/.cargo/bin:$PATH" ANDROID_NDK_HOME=/opt/android-sdk/ndk/29.0.14206865 ./scripts/mobile/build-android.sh` |
| APK | `mobile/android/app/build/outputs/apk/debug/app-debug.apk` |
| サイズ / SHA-256 | **14,692,156 bytes（14.0 MiB）** / `c71505fdf0701f3eeadeec8fee1dae7f9a5356ba630f4a757374d5320b98fc0a` |
| package / version | **`io.github.haruki0420.padbench`** / versionCode 1, versionName 0.1.0 |
| min / target SDK | 26 / 36 |
| 同梱 .so（`unzip -l`） | `arm64-v8a/libeightbitdo_uniffi.so` 1,266,296 bytes / `x86_64/libeightbitdo_uniffi.so` 1,193,928 bytes / `libjnidispatch.so` / `libandroidx.graphics.path.so` |

- `aapt2 dump badging` で `package: name='io.github.haruki0420.padbench'` を確認。
- JVM ユニットテスト（`./gradlew testDebugUnitTest --rerun`）は **8 件成功 / failures 0**（RustCore 2、AppWriteMode 3、FrameQueue 3。`app/build/test-results/testDebugUnitTest/*.xml` で確認）。
- **未確認**: 実機 / エミュレータでの起動と JNI ロード（§4）。

### 0.2 iOS（unsigned IPA）

| 項目 | 値 |
| --- | --- |
| ツール | xtool 1.24 系（Darwin SDK 27.2 artifact bundle）/ Rust target `aarch64-apple-ios` |
| コマンド | `./scripts/mobile/build-ios.sh` |
| IPA | `mobile/ios/xtool/Padbench.ipa`（旧 `EightBitDo.ipa` から改名） |
| サイズ / SHA-256 | **1,550,798 bytes（1.48 MiB）** / `ff98369bd22611f5db3b0aae0312da460c7053c3ad4e8e0b3fe1c66b935cab7b` |
| 内部名 | `Payload/Padbench.app/Padbench`（Mach-O 64-bit arm64） |
| Info.plist | `CFBundleIdentifier=io.github.haruki0420.padbench`, `CFBundleName/CFBundleDisplayName=Padbench`, `MinimumOSVersion=17.0`, `NSBluetoothAlwaysUsageDescription` あり, `UIBackgroundModes=[bluetooth-central]` |
| UniFFI シンボル | `llvm-nm -gU Payload/Padbench.app/Padbench \| grep -c uniffi` = **55**（Rust staticlib がリンクされている） |
| 署名 | なし（Linux では署名不可） |

- `xtool.yml` の `product: Padbench` と `Package.swift` の package / library product 名を `Padbench` に変更しました。Swift の target / module 名は `EightBitDo` のままです（生成済み UniFFI バインディングの `import` を変えないため。Android の Kotlin パッケージ `dev.eightbitdo.mobile` を据え置いたのと同じ方針）。
- **未確認**: 実機インストール・起動、署名、App Store 配布（§4）。

## 1. Android（JNI 同梱 APK）

| 項目 | 値 |
| --- | --- |
| NDK | `29.0.14206865`（`sdkmanager --install "ndk;29.0.14206865"`） |
| Rust targets | `aarch64-linux-android`, `x86_64-linux-android` |
| cargo-ndk | 4.1.2 |
| ビルド | `ANDROID_HOME=/opt/android-sdk ANDROID_NDK_HOME=/opt/android-sdk/ndk/29.0.14206865 ./scripts/mobile/build-android.sh` |
| APK | `mobile/android/app/build/outputs/apk/debug/app-debug.apk` |
| サイズ / SHA-256 | 14,551,011 bytes（13.9 MiB） / `879e894ccd902ea5987d2be708d006aaf4f58809e507775350571284eaa351f1` |
| package / version | `dev.eightbitdo.mobile` / versionCode 1, versionName 0.1.0 |
| min / target SDK | 26 / 36 |
| 権限 | `BLUETOOTH_SCAN`（neverForLocation）、`BLUETOOTH_CONNECT` ほか（AndroidManifest） |

APK 内の Rust ライブラリ（`unzip -l`）:

```text
lib/arm64-v8a/libeightbitdo_uniffi.so   1,266,288 bytes
lib/x86_64/libeightbitdo_uniffi.so      1,193,896 bytes
lib/*/libjnidispatch.so
```

ELF とシンボルの確認:

```text
$ file lib/arm64-v8a/libeightbitdo_uniffi.so
ELF 64-bit LSB shared object, ARM aarch64, dynamically linked, for Android 21, built by NDK r29

$ file lib/x86_64/libeightbitdo_uniffi.so
ELF 64-bit LSB shared object, x86-64, dynamically linked, for Android 21, built by NDK r29

$ nm -D --defined-only lib/arm64-v8a/libeightbitdo_uniffi.so | wc -l       # 122（defined dynamic symbols）
$ nm -D --defined-only lib/arm64-v8a/libeightbitdo_uniffi.so | grep -c uniffi  # 92（うち名前に uniffi を含む）
```

JVM ユニットテスト（`./gradlew testDebugUnitTest`）は 8 件成功（FrameQueue 3 / AppWriteMode 3 / RustCore 2）。Kotlin コンパイルと APK パッケージまで確認しています。

**未確認**: APK を実機/エミュレータで起動していません。ホストの emulator 実行ファイルが実行権限なし（`-rwxr--r-- root:root`）のため、エミュレータでの JNI ロード確認はできませんでした。実機では `libeightbitdo_uniffi.so` が同梱されているため、`RustCore.availability()` は `AVAILABLE` を返す見込みです（未確認）。

## 2. iOS（unsigned IPA）

| 項目 | 値 |
| --- | --- |
| ツール | xtool（Darwin SDK 27.2 artifact bundle） |
| Rust target | `aarch64-apple-ios`（staticlib） |
| ビルド | `./scripts/mobile/build-ios.sh` |
| IPA | `mobile/ios/xtool/EightBitDo.ipa` |
| サイズ / SHA-256 | 1,550,522 bytes（1.48 MiB） / `e63d7822010b948b4c555c2c561e2183bb4bf9c90599782affdc6f02bedb804f` |
| 署名 | なし（Linux では署名不可） |

`cargo rustc -p eightbitdo-uniffi --lib --release --target aarch64-apple-ios --crate-type staticlib` で生成した `libeightbitdo_uniffi.a` を `mobile/ios/libs/` に置き、xtool が Swift アプリとリンクしています。

**未確認**: 実機インストール・起動、App Store 配布。署名には macOS + Xcode（または信頼済みデバイスを接続した xtool）が必要です。

## 3. 再現コマンド

```bash
# Android（JNI 同梱）
export ANDROID_HOME=/opt/android-sdk
export ANDROID_NDK_HOME=/opt/android-sdk/ndk/29.0.14206865
rustup target add aarch64-linux-android x86_64-linux-android
cargo install cargo-ndk --locked
./scripts/mobile/build-android.sh

# iOS（unsigned IPA）
rustup target add aarch64-apple-ios
./scripts/mobile/build-ios.sh
```

## 4. 未完了

- Android / iOS 実機での BLE 接続・読み取り・書き込み確認。
- Android エミュレータでの起動確認（emulator の実行権限を直すか、実機で確認）。
- iOS 署名・TestFlight / App Store 配布。
- Android の Rust JNI は release ビルド。デバッグ時の ABI 追加（`armv7`）は未対応。
