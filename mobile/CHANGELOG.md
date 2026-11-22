# mobile/ — CHANGELOG

モバイル（iOS / Android）まわりの変更履歴。バージョンはアプリの
`versionName` ではなくマイルストーン（M0 = 読み取り API + ビルド骨組み）に対応します。

## Unreleased（M0）

### Added

- `crates/eightbitdo-uniffi`: `NativeTransport` callback interface、
  専用 Tokio runtime の同期 `MobileSession`（version / config / macro backup /
  input / disconnect）、`describeConfig`、`prepareConfigWrite`（書き込み準備のみ）。
- `mobile/ios/`: SwiftUI アプリ、CoreBluetooth `NativeTransport`、
  `WriteAckTracker`、xtool/SwiftPM からの Rust staticlib リンク。
- `mobile/android/`: Jetpack Compose アプリ、`BleScanner`、
  `BluetoothGattTransport`、bounded `FrameQueue`、`AppWriteMode`、
  UniFFI ネイティブライブラリ検出（`RustCore`）、JVM unit tests。
- `scripts/mobile/`: `gen-bindings.sh`（uniffi crate / bindgen の
  バージョン一致検証つき）、`build-ios.sh`、`build-android.sh`
  （`--kotlin-only` 対応）、`test-write-ack-tracker.sh`。
- Gradle wrapper 9.8.2（`distributionSha256Sum` 固定）をコミット。

### Changed

- `uniffi` を 0.31.2 に固定（0.32 系の推移依存が MSRV 1.85 を超えるため）。
  `textwrap 0.16.2` / `askama 0.14` / `cargo_metadata 0.19` を使用。
- iOS / Android の受信キューを bounded（64 フレーム、drop-oldest）にし、
  落ちたフレームは読み出しの失敗として表面化させる（成功扱いにしない）。
- `MobileSession` は MTU 161 未満を拒否。Android は MTU 交渉を必須化
  （23 へのフォールバックなし）、iOS は MTU 不足時に
  `cancelPeripheralConnection` も行う。
- 書き込みモードは特性の properties から選択: without-response 優先、
  無ければ with-response、どちらも無ければ明示エラー。
- Android は AGP 9.5.0 / Gradle 9.8.2 / Kotlin 2.4.21（AGP built-in Kotlin）に
  合わせ、`org.jetbrains.kotlin.android` プラグインを外した。

### Fixed

- iOS with-response のタイムアウトを **成功として返さない**よう修正。
  `WriteAckTracker` の世代番号で遅延 `didWriteValueFor` の誤帰属を防ぎ、
  タイムアウト後は再接続まで書き込みを拒否する。
- Android の遅延 `onCharacteristicWrite` も同様に、タイムアウト後は
  次の write に帰属させず再接続を要求する。
- Kotlin-only APK（Rust JNI 非同梱）で `UnsatisfiedLinkError` により
  プロセスが落ちる問題を修正。`RustCore.availability()` が `Throwable` を
  捕捉して UI に「NDK ビルド版が必要」と表示する。
- Android `FrameQueue` を `ReentrantLock` + `Condition` に修正
  （`Any()` の `wait/notifyAll` は Kotlin 2.x で未解決）。
- Rust `FrameQueue` の close 時 lost-wakeup を `Notified::enable` で修正。
- UniFFI エラーフィールドを `detail` に変更（Kotlin 2.4 で生成コードの
  `message` が衝突するため）。
- 接続ライフサイクル: 再接続時に旧セッションを閉じ、GATT client / executor を
  すべての経路で解放。`disconnect` の join は transport の `recv` タイムアウトで
  上限を設ける（Drop では join しない）。

### Known limitations

- 実機（iPhone / Android 実機）での BLE 読み出しは未検証。
- Android の Rust JNI は NDK + cargo-ndk 導入後の完全ビルドが必要。
- 設定フラッシュ書き込み・OTA は未公開（Tier 3 レビュー + バックアップ UI 待ち）。
- iOS の署名・App Store 配布は macOS が必要。
