# モバイル成果物のビルド検証（2026-11-23）

- 目的: Android JNI 同梱 APK と iOS unsigned IPA のビルド・リンク・パッケージ内容の確認記録（実機 BLE 動作確認は含まない）。
- ホスト環境: Arch Linux x86_64 / rustc・cargo 1.100.0 / OpenJDK 21.0.13 / Android NDK `29.0.14206865` / `cargo-ndk` 4.1.2 / xtool 1.24 系（Darwin SDK 27.2）。

## 1. Padbench 統一後の再ビルド（2026-11-23 15:02 JST、`feat/tauri-gui`）

Android の `namespace` / `applicationId` と iOS の `bundleID` / product 名を `io.github.haruki0420.padbench` / `Padbench` に統一した後の結果（Swift module 名 `EightBitDo` と Kotlin パッケージ `dev.eightbitdo.mobile` は UniFFI バインディング互換のため据え置き）。

| 項目 | Android（JNI 同梱 debug APK） | iOS（unsigned IPA） |
| --- | --- | --- |
| コマンド | `./scripts/mobile/build-android.sh` | `./scripts/mobile/build-ios.sh` |
| 成果物 | `mobile/android/app/build/outputs/apk/debug/app-debug.apk` | `mobile/ios/xtool/Padbench.ipa` |
| サイズ | **14,692,156 bytes（14.0 MiB）** | **1,550,798 bytes（1.48 MiB）** |
| SHA-256 | `c71505fdf0701f3eeadeec8fee1dae7f9a5356ba630f4a757374d5320b98fc0a` | `ff98369bd22611f5db3b0aae0312da460c7053c3ad4e8e0b3fe1c66b935cab7b` |
| 識別子 / 版 | `io.github.haruki0420.padbench` / v0.1.0（code 1）、min/target SDK 26/36 | `io.github.haruki0420.padbench` / `Padbench`、MinimumOSVersion 17.0 |
| Rust バイナリ確認 | `arm64-v8a/libeightbitdo_uniffi.so`（1,266,296B）、`x86_64/libeightbitdo_uniffi.so`（1,193,928B）、`libjnidispatch.so` | `Payload/Padbench.app/Padbench`（Mach-O arm64）、`llvm-nm -gU \| grep -c uniffi` = **55** |
| テスト / 権限 | JVM ユニットテスト **8 件成功**（RustCore 2 / AppWriteMode 3 / FrameQueue 3）、`aapt2 dump badging` 確認 | `NSBluetoothAlwaysUsageDescription`、`UIBackgroundModes=[bluetooth-central]`、署名なし |

## 2. 統一前の初回ビルド記録（main `c169392`）

- **Android**（`dev.eightbitdo.mobile`）: `app-debug.apk` 14,551,011 bytes（SHA-256 `879e894ccd902ea5987d2be708d006aaf4f58809e507775350571284eaa351f1`）。`arm64-v8a/libeightbitdo_uniffi.so`（1,266,288B、ELF aarch64 Android 21、defined dynamic symbols 122 / uniffi 92）、`x86_64`（1,193,896B）。JVM テスト 8 件成功。
- **iOS**（`EightBitDo.ipa`）: 1,550,522 bytes（SHA-256 `e63d7822010b948b4c555c2c561e2183bb4bf9c90599782affdc6f02bedb804f`）。`aarch64-apple-ios` staticlib リンク、署名なし。

## 3. 再現コマンド

```bash
# Android（ANDROID_HOME / ANDROID_NDK_HOME を設定済みであること）
rustup target add aarch64-linux-android x86_64-linux-android
cargo install cargo-ndk --locked
./scripts/mobile/build-android.sh

# iOS（xtool + Darwin SDK 導入済みであること）
./scripts/mobile/build-ios.sh
```

## 4. 未確認・未完了

- Android / iOS 実機での BLE 接続・読み取り・書き込み・JNI ロード起動確認（ホストのエミュレータ実行ファイルは実行権限なしのため未確認）。
- iOS 署名・TestFlight / App Store 配布（macOS + Xcode 必須）。
- Android デバッグ向け `armv7` ABI 追加。
