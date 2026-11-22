# 第三者ライセンス（Third-Party Licenses）

本プロジェクトが利用する第三者ソフトウェアと、同梱フォントのライセンス条件をまとめます。

> 本ドキュメントは**法的助言ではありません**。記載は公開情報・パッケージメタデータ・各ライセンス原文に基づく整理であり、法的結論ではありません。ライセンスの解釈・配布条件の充足・商標の扱いは、公開・配布・ストア申請の前に知的財産権を扱う弁護士に確認してください。
>
> また、依存バージョンと各プロジェクトのライセンスは将来変更され得ます。**依存を追加・更新するたびに再生成・再確認**してください。

> 本ファイルは**要約と参照**です。497 クレート全件の著作権表示・ライセンス本文を網羅するものではありません。現時点で全文を収録しているのは、追加義務のある BSD-3-Clause（btleplug）、Unicode-3.0、SIL OFL 1.1（同梱フォント）です。MPL-2.0（uniffi）・Apache-2.0（Gradle wrapper）・Ubuntu Font Licence（egui 既定フォント）の全文は各クレート／資産に同梱されたライセンスファイルを参照し、配布物には生成した通知を含めてください。バイナリとして配布する場合は、`cargo-about` 等で依存グラフ全体の著作権表示とライセンス本文を生成し、配布物へ同梱してください（§4）。

## 1. サマリ

- 基準: main `6b39e23`（2026-11-23、ブランチ `docs/licensing-refresh` の base）の `cargo metadata --locked --all-features`（cargo 1.100.0）。
- パッケージ総数 **502**（workspace メンバー 5 を除く**第三者クレート 497**）。`license` フィールドが空のクレートは **0**（`license_file` 指定も 0）。497 件はすべて crates.io 由来で、git / path 依存はありません。
- 前版（base `80f8d66`、第三者 131）からの差は **+366 クレート**。主因は GUI（`eframe`/`egui` 0.29.1）とモバイル（`uniffi` 0.31.2）、BLE 単一所有者ロック（`fs2`/`rustix`/`nix`）の追加です。
- **GPL / AGPL / SSPL / EUPL / CDDL / EPL は 0 件。** ライセンス式に MIT を含むクレート（MIT を選択可能）は **444**、Apache-2.0 を含むクレート（Apache-2.0 を選択可能）は **345**。どちらも選択できない **37 クレート**は §3.5 に列挙します（`MPL-2.0` 9、`Unicode-3.0` 単独 18、`Zlib` 3、`BSD-3-Clause` 2、`BSL-1.0` 2、`BSD-2-Clause` / `ISC` / `CC0-1.0` 各 1 など）。
- 直接依存のうち **`uniffi` 0.31.2 だけが MPL-2.0**（唯一の非 MIT/Apache 直接依存）。ほかは MIT または Apache-2.0 を選択可能です（`btleplug` は BSD-3-Clause を含む三択、`uuid` は Apache-2.0/MIT、`nix` は MIT、`fs2` は MIT/Apache-2.0、`rustix` は Apache-2.0 WITH LLVM-exception / Apache-2.0 / MIT）。
- Cargo 管理外の同梱資産も本ファイルで扱います: GUI 同梱の **Noto Sans JP**（OFL-1.1、§5）、**egui 既定フォント**（OFL-1.1 / Ubuntu Font Licence 1.0、§3.6.1）、**Gradle wrapper**（Apache-2.0、§3.6.3）。
- 参考: Linux x86_64 ホストで実際にビルドされる第三者クレートは `cargo tree --locked --workspace --all-features` のユニーク **315**（dev 依存を含む。既定 feature では **298**）。`cargo metadata` の 497 は Cargo.lock 全体＝全ターゲット・全 feature・optional 依存を含む集合です。

| SPDX 式 | 件数 |
| --- | ---: |
| `MIT OR Apache-2.0` | 237 |
| `MIT` | 109 |
| `Apache-2.0 OR MIT` | 36 |
| `MIT/Apache-2.0` | 18 |
| `Unicode-3.0` | 18 |
| `Apache-2.0` | 14 |
| `MPL-2.0` | 9 |
| `Zlib OR Apache-2.0 OR MIT` | 8 |
| `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | 7 |
| `MIT OR Apache-2.0 OR Zlib` | 5 |
| `Apache-2.0/MIT` | 4 |
| `Unlicense OR MIT` | 4 |
| `Zlib` | 3 |
| `BSD-2-Clause OR Apache-2.0 OR MIT` | 2 |
| `BSD-3-Clause` | 2 |
| `BSD-3-Clause OR Apache-2.0` | 2 |
| `BSD-3-Clause OR MIT OR Apache-2.0` | 2 |
| `BSL-1.0` | 2 |
| `MIT OR Apache-2.0 OR LGPL-2.1-or-later` | 2 |
| `MIT OR Zlib OR Apache-2.0` | 2 |
| `Unlicense/MIT` | 2 |
| `(MIT OR Apache-2.0) AND OFL-1.1 AND LicenseRef-UFL-1.0` | 1 |
| `(MIT OR Apache-2.0) AND Unicode-3.0` | 1 |
| `0BSD OR MIT OR Apache-2.0` | 1 |
| `Apache-2.0 AND MIT` | 1 |
| `BSD-2-Clause` | 1 |
| `CC0-1.0` | 1 |
| `ISC` | 1 |
| `MIT / Apache-2.0` | 1 |
| `MIT/Apache-2.0/BSD-3-Clause` | 1 |
| **合計** | **497** |

## 2. 直接依存（workspace）

解決バージョンは `Cargo.lock` に基づきます。「利用箇所」は workspace 内のクレート名です。

| クレート | バージョン | SPDX | 利用箇所 | 備考 |
| --- | --- | --- | --- | --- |
| `btleplug` | 0.13.5 | `MIT/Apache-2.0/BSD-3-Clause` | `eightbitdo-core`（既定 feature `ble`） | 三択。本プロジェクトは MIT / Apache-2.0 を選択。Linux では D-Bus / BlueZ 経由（§3.1） |
| `eframe` | 0.29.1 | `MIT OR Apache-2.0` | `eightbitdo-gui` | 既定 feature（glow / default_fonts / x11 / wayland）。egui 既定フォントの帰属は §3.6.1 |
| `uniffi` | 0.31.2 | `MPL-2.0` | `eightbitdo-uniffi` | 唯一の非 MIT/Apache 直接依存。ファイル単位コピーレフト（§3.3） |
| `fs2` | 0.4.3 | `MIT/Apache-2.0` | `eightbitdo-core`（任意 feature `ble`） | BLE 単一所有者ロックの file lock。MIT を選択 |
| `rustix` | 1.1.6 | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | `eightbitdo-core`（`cfg(unix)`、任意 feature `ble`） | Unix のみ。Apache-2.0 を選択。推移的に `rustix` 0.38.44（winit 経由）も入る |
| `nix` | 0.29.0 | `MIT` | `eightbitdo-core`（`cfg(unix)`、任意 feature `ble`） | Unix のみ。`user` feature で `getpwuid_r` |
| `tokio` | 1.54.1 | `MIT` | `eightbitdo-core`, `eightbitdo-cli`, `eightbitdo-gui`, `eightbitdo-uniffi` | — |
| `tokio-util` | 0.7.20 | `MIT` | `eightbitdo-cli` | — |
| `serde` | 1.0.229 | `MIT OR Apache-2.0` | `eightbitdo-protocol`（任意 feature `serde`）, `eightbitdo-cli`, `eightbitdo-uniffi` | — |
| `serde_json` | 1.0.154 | `MIT OR Apache-2.0` | `eightbitdo-cli`, `eightbitdo-gui`, `eightbitdo-uniffi` | — |
| `clap` | 4.6.9 | `MIT OR Apache-2.0` | `eightbitdo-cli` | — |
| `thiserror` | 2.0.21 | `MIT OR Apache-2.0` | `eightbitdo-core`, `eightbitdo-uniffi` | 推移的に `thiserror` 1.0.69 も入る |
| `sha2` | 0.10.9 | `MIT OR Apache-2.0` | `eightbitdo-cli`, `eightbitdo-gui` | — |
| `toml` | 0.8.23 | `MIT OR Apache-2.0` | `eightbitdo-cli` | — |
| `uuid` | 1.26.1 | `Apache-2.0 OR MIT` | `eightbitdo-core` | — |
| `async-trait` | 0.1.92 | `MIT OR Apache-2.0` | `eightbitdo-core`, `eightbitdo-uniffi`, `eightbitdo-cli`（dev） | — |
| `futures-util` | 0.3.34 | `MIT OR Apache-2.0` | `eightbitdo-core`（任意 feature `ble`） | — |
| `hex` | 0.4.3 | `MIT OR Apache-2.0` | `eightbitdo-cli`, `eightbitdo-gui` | — |
| `chrono` | 0.4.45 | `MIT OR Apache-2.0` | `eightbitdo-gui` | 時刻表示 |
| `dirs` | 5.0.1 | `MIT OR Apache-2.0` | `eightbitdo-gui` | 推移的に `dirs-sys` → `option-ext`（MPL-2.0、§3.3） |
| `unicode-ident` | 1.0.27 | `(MIT OR Apache-2.0) AND Unicode-3.0` | 推移的（`proc-macro2` 経由） | 本プロジェクトのコードから直接使用していない。**Unicode-3.0 の帰属義務が加算される**（§3.2） |

> 利用箇所は workspace メンバーの `Cargo.toml` に基づきます。Linux で実際にビルドされる集合は全ターゲット集計より小さくなります（§1 の参考値を参照）。macOS / Windows / Android / iOS 向けのクレート（`objc2*` / `windows*` / `jni*` など）は同梱物に応じて帰属表示が必要です。

## 3. 帰属・ライセンス全文の扱い

本プロジェクトのコードは `MIT OR Apache-2.0` で提供され、全文は [LICENSE-MIT](LICENSE-MIT) / [LICENSE-APACHE](LICENSE-APACHE) にあります。`LICENSE-APACHE` は Apache-2.0 の公式原文で、APPENDIX の `[yyyy] [name of copyright owner]` を `2026 haruki0420` に置換した以外は改変していません。依存側の選択式ライセンス（`OR`）は、本プロジェクトでは **MIT または Apache-2.0** を選択します（`uniffi` の MPL-2.0 と単一ライセンスのクレートは選択式ではありません。§3.3・§3.5）。以下は追加で注意が必要なものです。

### 3.1 BSD-3-Clause（btleplug / tiny-skia）

`btleplug` は `MIT/Apache-2.0/BSD-3-Clause` の三択のため、MIT / Apache-2.0 を選択する限り BSD の帰属義務は生じません。ただし安全側に倒し、参考として BSD-3-Clause の著作権表示を記載します。

- Copyright (c) 2020-2021, Nonpolynomial
- blurmac 由来部分: Copyright (c) 2017 Akos Kiss

GUI が同梱する `tiny-skia` / `tiny-skia-path`（`sctk-adwaita` → `winit` → `eframe` 経由）は **BSD-3-Clause のみ**で選択の余地がなく、帰属義務があります。

- tiny-skia: Copyright (c) 2011 Google Inc. All rights reserved. / Copyright (c) 2020 Yevhenii Reizner All rights reserved.
- tiny-skia-path: 著作権表示はクレート同梱の `LICENSE` に従います（BSD-3-Clause の同一条文）。

BSD-3-Clause 全文（条文は共通で、著作権表示のみ上記のとおり読み替え）:

```text
Copyright (c) 2020-2021, Nonpolynomial
All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

* Redistributions of source code must retain the above copyright
  notice, this list of conditions and the following disclaimer.

* Redistributions in binary form must reproduce the above copyright
  notice, this list of conditions and the following disclaimer in the
  documentation and/or other materials provided with the distribution.

* Neither the name of btleplug nor the names of its contributors may
  be used to endorse or promote products derived from this software
  without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

### 3.2 Unicode-3.0（ICU 系を含む 19 クレート）

前版では `unicode-ident` のみでしたが、GUI の追加で `eframe` → `webbrowser` → `url` → `idna` → `idna_adapter` → ICU4X 系が入り、Unicode-3.0 のクレートは **19** になりました。MIT / Apache-2.0 に**加えて** Unicode ライセンスの著作権表示と許可通知を同梱する必要があります。

- `Unicode-3.0` 単独（18）: `icu_collections`, `icu_locale_core`, `icu_normalizer`, `icu_normalizer_data`, `icu_properties`, `icu_properties_data`, `icu_provider`, `litemap`, `potential_utf`, `tinystr`, `writeable`, `yoke`, `yoke-derive`, `zerofrom`, `zerofrom-derive`, `zerotrie`, `zerovec`, `zerovec-derive`
- `(MIT OR Apache-2.0) AND Unicode-3.0`（1）: `unicode-ident` 1.0.27（`proc-macro2` 経由）

以下は共通のライセンス本文です。各クレートの著作権年は同一ではないため、配布物では `cargo-about` 等の生成物に含まれる個別の著作権表示を用いてください。

```text
UNICODE LICENSE V3

COPYRIGHT AND PERMISSION NOTICE

Copyright © 1991-2023 Unicode, Inc.

NOTICE TO USER: Carefully read the following legal agreement. BY
DOWNLOADING, INSTALLING, COPYING OR OTHERWISE USING DATA FILES, AND/OR
SOFTWARE, YOU UNEQUIVOCALLY ACCEPT, AND AGREE TO BE BOUND BY, ALL OF THE
TERMS AND CONDITIONS OF THIS AGREEMENT. IF YOU DO NOT AGREE, DO NOT
DOWNLOAD, INSTALL, COPY, DISTRIBUTE OR USE THE DATA FILES OR SOFTWARE.

Permission is hereby granted, free of charge, to any person obtaining a
copy of data files and any associated documentation (the "Data Files") or
software and any associated documentation (the "Software") to deal in the
Data Files or Software without restriction, including without limitation
the rights to use, copy, modify, merge, publish, distribute, and/or sell
copies of the Data Files or Software, and to permit persons to whom the
Data Files or Software are furnished to do so, provided that either (a)
this copyright and permission notice appear with all copies of the Data
Files or Software, or (b) this copyright and permission notice appear in
associated Documentation.

THE DATA FILES AND SOFTWARE ARE PROVIDED "AS IS", WITHOUT WARRANTY OF ANY
KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT OF
THIRD PARTY RIGHTS.

IN NO EVENT SHALL THE COPYRIGHT HOLDER OR HOLDERS INCLUDED IN THIS NOTICE
BE LIABLE FOR ANY CLAIM, OR ANY SPECIAL INDIRECT OR CONSEQUENTIAL DAMAGES,
OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS,
WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION,
ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THE DATA
FILES OR SOFTWARE.

Except as contained in this notice, the name of a copyright holder shall
not be used in advertising or otherwise to promote the sale, use or other
dealings in these Data Files or Software without prior written
authorization of the copyright holder.
```

### 3.3 MPL-2.0（uniffi / option-ext）

MPL-2.0 は **ファイル単位の弱コピーレフト**です。本プロジェクトの唯一の非 MIT/Apache 直接依存で、crates.io の未改変版を使用しています。

| クレート | バージョン | 経路 |
| --- | --- | --- |
| `uniffi` | 0.31.2 | `eightbitdo-uniffi` の直接依存 |
| `uniffi_bindgen`, `uniffi_core`, `uniffi_internal_macros`, `uniffi_macros`, `uniffi_meta`, `uniffi_pipeline`, `uniffi_udl` | 0.31.2 | `uniffi` の内部クレート（feature `cli` で bindgen も入る） |
| `option-ext` | 0.2.0 | `eightbitdo-gui` → `dirs` → `dirs-sys` |

義務と扱い:

- クレートを**改変しない限り**、本プロジェクトのコード（MIT / Apache-2.0）へ MPL は及びません。アプリ全体の再ライセンスや全ソース開示は不要です。
- 改変したファイルのみ MPL-2.0 となり、そのソースを MPL-2.0 で提供する必要があります。`Cargo.toml` に `[patch]` は無く、現時点で改変はありません。改変する場合は本節と配布物のソース提供を更新してください。
- バイナリ配布時は、MPL-2.0 の対象であることと、ソースの入手先（crates.io の該当バージョン）を通知してください（MPL-2.0 §3.2 Executable Form）。
- MPL-2.0 はアプリ全体へのコピーレフトを及ぼさず、追加の利用制限も課さないため、App Store / Google Play などのストア配布と両立し得ます（一般論であり法的助言ではありません）。
- UniFFI が生成する Kotlin / Swift バインディングに MPL 表記が付くかは生成物で確認してください（生成物は `.gitignore` 済みで、リポジトリには含まれません）。

### 3.4 選択式ライセンス（OR）の扱い

- `Unlicense OR MIT` / `Unlicense/MIT`（`memchr`, `same-file`, `walkdir`, `winapi-util` など 6）: 本プロジェクトは **MIT** を選択します。MIT の著作権表示は各クレートの `LICENSE` に従います。
- `Zlib OR Apache-2.0 OR MIT`（`bytemuck`, `objc2-*` など 8）: **MIT または Apache-2.0** を選択します。Zlib の帰属義務は生じません。
- `MIT OR Apache-2.0 OR Zlib` / `MIT OR Zlib OR Apache-2.0`（`glow`, `raw-window-handle`, `miniz_oxide` など 7）: MIT または Apache-2.0 を選択します。
- `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`（`rustix` 2 バージョン, `wasi` など 7）: Apache-2.0 を選択します。
- `BSD-3-Clause OR Apache-2.0`（`moxcms`, `pxfm`）、`BSD-3-Clause OR MIT OR Apache-2.0`（`num_enum`, `num_enum_derive`）、`BSD-2-Clause OR Apache-2.0 OR MIT`（`zerocopy`, `zerocopy-derive`）: MIT または Apache-2.0 を選択します。
- `MIT OR Apache-2.0 OR LGPL-2.1-or-later`（`r-efi` 2）: MIT または Apache-2.0 を選択し、**LGPL の義務は生じません**。
- `0BSD OR MIT OR Apache-2.0`（`adler2`）、`Apache-2.0/MIT`（4）、`MIT/Apache-2.0`（18）: MIT または Apache-2.0 を選択します。
- `Apache-2.0 AND MIT`（`dpi`）: 両方の条件を満たす必要があります（Apache-2.0 の NOTICE 義務を含む）。
- `MIT/Apache-2.0/BSD-3-Clause`（`btleplug`）: §3.1。

### 3.5 単一ライセンスのクレート（選択不可・帰属必須）

| ライセンス | クレート | 件数 | 義務 |
| --- | --- | ---: | --- |
| `Zlib` | `foldhash`, `slotmap`, `zlib-rs` | 3 | 著作権表示と「This notice may not be removed or altered from any source distribution」を保持 |
| `BSD-3-Clause` | `tiny-skia`, `tiny-skia-path` | 2 | §3.1 |
| `BSD-2-Clause` | `arrayref` | 1 | 著作権表示・条件の保持 |
| `ISC` | `libloading` | 1 | 著作権表示・許可通知の保持 |
| `BSL-1.0` | `clipboard-win`, `error-code` | 2 | 著作権表示と Boost ライセンス全文を同梱 |
| `CC0-1.0` | `hexf-parse` | 1 | 表示義務なし（帰属は任意） |
| `MPL-2.0` | `uniffi` 系 8 + `option-ext`（§3.3） | 9 | ファイル単位コピーレフト |
| `Unicode-3.0` | `icu_*` / `zerovec` など 18（§3.2） | 18 | Unicode ライセンスの表示 |
| `(MIT OR Apache-2.0) AND OFL-1.1 AND LicenseRef-UFL-1.0` | `epaint_default_fonts` | 1 | §3.6.1 |

### 3.6 帰属が必要な同梱資産

#### 3.6.1 egui の既定フォント（`epaint_default_fonts` 0.29.1）

GUI は `eframe` を既定 feature で使うため、`egui` の `default_fonts` が有効になり、`epaint_default_fonts` がバイナリに含まれます。ライセンス式は `(MIT OR Apache-2.0) AND OFL-1.1 AND LicenseRef-UFL-1.0` で、同梱フォントごとに条件が異なります。

| ファイル | フォント | ライセンス |
| --- | --- | --- |
| `Hack-Regular.ttf` | Hack | MIT（+ DejaVu public domain、Bitstream Vera License） |
| `NotoEmoji-Regular.ttf` | Noto Emoji | SIL OFL 1.1 |
| `Ubuntu-Light.ttf` | Ubuntu Light | Ubuntu Font Licence 1.0（**非 SPDX**） |
| `emoji-icon-font.ttf` | emoji-icon-font | MIT |

- クレートには `fonts/OFL.txt` / `fonts/UFL.txt` / `fonts/Hack-Regular.txt` / `fonts/emoji-icon-font-mit-license.txt` が同梱されています。**バイナリ配布物にはこれら 4 ファイルを含めてください。**
- `LicenseRef-UFL-1.0` は SPDX 標準識別子ではありません。`cargo-deny` を導入する場合は例外扱いを検討してください（付録 B）。
- これらはシステムフォントではありません。日本語表示自体は GUI 同梱の Noto Sans JP（§5）を使います。

#### 3.6.2 Noto Sans JP（同梱済み）

`crates/eightbitdo-gui/assets/fonts/` に `NotoSansJP-{Regular,Medium,Bold}.otf` と `OFL.txt` がコミットされています。詳細と義務は §5 を参照してください。

#### 3.6.3 Gradle wrapper（Apache-2.0、コミット済み jar）

`mobile/android/` の Gradle wrapper はビルド再現性のためリポジトリにコミットしています（Gradle 9.8.2）。

- 対象: `mobile/android/gradlew`, `mobile/android/gradlew.bat`, `mobile/android/gradle/wrapper/gradle-wrapper.jar`, `mobile/android/gradle/wrapper/gradle-wrapper.properties`
- `gradle-wrapper.jar` の `META-INF/MANIFEST.MF` に `SPDX-License-Identifier: Apache-2.0`、jar 内に `META-INF/LICENSE`（Apache-2.0 全文）が含まれます。帰属: Gradle, Inc.（The Gradle Authors）。
- 再配布時は Apache-2.0 の全文（[LICENSE-APACHE](LICENSE-APACHE) と同一）と著作権表示を保持してください。jar には NOTICE が含まれていないため、追加の NOTICE 要件はありません（Apache-2.0 §4(d)）。jar を改変する場合は改変の明示が必要です（§4(b)）。
- Gradle 本体（配布 zip）や依存はコミットしていません。`gradle-wrapper.properties` の `distributionSha256Sum` で取得物を固定しています。
- wrapper を更新する場合は、生成元 Gradle バージョン・SHA-256・jar 内のライセンス表示を再確認してください。

### 3.7 追加された直接依存の注意

| クレート | バージョン | SPDX | 注意 |
| --- | --- | --- | --- |
| `eframe` / `egui` | 0.29.1 | `MIT OR Apache-2.0` | 既定 feature（glow / default_fonts / x11 / wayland）で GUI に組み込み。wgpu 系は既定では有効化していません。同梱フォントは §3.6.1 |
| `fs2` | 0.4.3 | `MIT/Apache-2.0` | BLE 単一所有者ロックの file lock。MIT を選択 |
| `rustix` | 1.1.6（直接）/ 0.38.44（winit 経由） | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | Unix のみ。Apache-2.0 を選択 |
| `nix` | 0.29.0 | `MIT` | Unix のみ。`user` feature で `getpwuid_r` |
| `chrono` | 0.4.45 | `MIT OR Apache-2.0` | GUI の時刻表示 |
| `dirs` | 5.0.1 | `MIT OR Apache-2.0` | GUI のバックアップ先。`dirs-sys` → `option-ext`（MPL-2.0、§3.3） |
| `uniffi` | 0.31.2 | `MPL-2.0` | §3.3 |

> USB HID（`hidapi`）はまだ追加されていません。追加時は、Rust ラッパーは `MIT` ですが**上流 C 実装が GPL-3.0 / BSD-3-Clause / original の三択**のため、BSD または original を選択し、上流 `LICENSE.txt` を本ファイルへ追加してください（`linux-static-hidraw` を推奨。§3.9 の libudev 項も参照）。

### 3.8 NOTICE ファイルを持つ依存

- `cfg_aliases` 0.1.1 / 0.2.2 が `NOTICES.md` を持ちます（`tectonic_cfg_support::target_cfg!` 由来の MIT コード。Apache-2.0 の NOTICE ではありません）。`cargo-about` の生成物には含めてください。
- それ以外に、展開済みの依存ソースを走査した限り `NOTICE` ファイルは確認されていません（2026-11-23 時点）。Apache-2.0 の依存に NOTICE がある場合は保持が必要です（Apache-2.0 §4(d)）。依存更新時に再確認してください。
- 自プロジェクトの [NOTICE](NOTICE) は依存側 NOTICE とは別物です。

### 3.9 システムライブラリ（Cargo の外）

`cargo deny` は Cargo 依存しか検査しません。配布物に `.so` を同梱する場合（AppImage / Flatpak / インストーラ）は `ldd` で洗い出し、追加の帰属表示を行ってください。

| ライブラリ | ライセンス | 使われ方 | 対応 |
| --- | --- | --- | --- |
| libdbus | **AFL-2.1 または GPL-2.0+ のデュアル** | `btleplug` → `bluez-async` → `dbus`（Linux） | **AFL-2.1 を選択**。同梱する場合は AFL の帰属を追加。通常の Linux 配布（システム提供）なら同梱不要 |
| libudev | LGPL-2.1+ | `hidapi` の `linux-native` feature 使用時のみ | 既定の `linux-static-hidraw` なら不要。使う場合は動的リンクと LGPL 条件を確認 |
| BlueZ | GPL-2.0 | OS の D-Bus サービスとして利用 | リンク・同梱しない |
| OpenGL / EGL / GLX（Mesa 等） | MIT 系 | GUI（`glow` / `glutin`） | 通常はシステム提供。AppImage / Flatpak で同梱する場合は Mesa 等の帰属を追加 |
| X11 / xcb / libxkbcommon / Wayland client | MIT 系 | GUI（`winit` / `glutin`、x11 / wayland feature） | 通常はシステム提供。同梱する場合は各ライブラリの帰属を追加 |
| GPU ドライバ（OpenGL / Vulkan / Metal） | 各ベンダ | GUI 描画 | 通常は同梱しない |
| Android SDK / NDK / Gradle / Kotlin ツールチェーン | Apache-2.0 等 | モバイルのビルド時にのみ使用 | 配布物に同梱しない（Gradle wrapper は §3.6.3） |

## 4. 生成・更新方法

本ファイルは `cargo metadata` の集計結果を正としています。依存の追加・更新時に次を実行し、SPDX 集計表・件数・直接依存表を更新してください。基準コミット（現在は main `6b39e23`）も併せて更新します。

```sh
# 対象コミットの Cargo.lock を固定して集計する（--locked を必須にする）
cargo metadata --locked --all-features --format-version 1 > /tmp/meta.json
python3 - <<'PY'
import json, collections
m = json.load(open('/tmp/meta.json'))
third = [p for p in m['packages'] if p['source'] is not None]
print('packages', len(m['packages']), 'workspace', len(m['workspace_members']), 'third', len(third))
print('license 空', sum(1 for p in third if not p.get('license')))
for lic, n in collections.Counter(p.get('license') for p in third).most_common():
    print(n, '|', lic)
PY
```

- `cargo metadata` の件数は Cargo.lock 全体（全ターゲット・全 feature・optional 依存を含む）です。Linux ホストで実際にビルドされる範囲は `cargo tree --locked --workspace --all-features` で確認できます。
- 旧版との差分は `git diff <旧 base>..<新 base> -- Cargo.lock` と本ファイルの件数で確認します。
- 未マージのブランチが追加する依存は本一覧に反映されません。依存を追加する PR では、その PR 内で本ファイルの該当行（§2 の直接依存表、§3 の特殊ライセンス）を更新してください。

本格運用では次のいずれかを推奨します。

- **`cargo-deny`**: 許可リストで GPL 系の混入を機械的に拒否（付録 B）。
- **`cargo-about`**: `cargo about generate` で帰属一覧を自動生成し、本ファイルの表を置き換える。
- CI で生成物の差分（drift）を検出する（付録 C）。

## 5. 同梱フォント: Noto Sans JP（SIL OFL 1.1）

### 5.1 現状

- **main `6b39e23` の時点で GUI（`crates/eightbitdo-gui`）が存在し、フォント資産もコミット済みです。** `crates/eightbitdo-gui/assets/fonts/` に次のファイルがあります。
  - `NotoSansJP-Regular.otf`（7461 グリフ、JIS X 0208 第 1・2 水準 + UI 文字列）
  - `NotoSansJP-Medium.otf` / `NotoSansJP-Bold.otf`（各 1054 グリフ、UI 文字列のみ）
  - `OFL.txt`（上流の OFL 全文、改変なし）
  - `README.md`（サブセット手順・上流バージョン）、`build_fonts.py`（再生成スクリプト）
- 上流は Noto Sans CJK JP 2.004 で、サブセット化（グリフ削減・縦組みテーブル等の削除）は Modified Version 扱いです。詳細は `crates/eightbitdo-gui/assets/fonts/README.md` を参照してください。
- フォントの著作権表示（上流 OFL.txt の冒頭）:

```text
Copyright 2014-2021 Adobe (http://www.adobe.com/), with Reserved Font Name 'Source'
```

### 5.2 OFL 1.1 の義務（要点）

1. フォント単体（個々のコンポーネントを含む）を**販売しない**。アプリに同梱してアプリを販売するのは可。
2. 同梱・再配布時は**著作権表示と OFL 全文**を、ユーザーが容易に閲覧できる形で含める（standalone のテキストファイル、人間が読めるヘッダ、または機械可読メタデータ）。
3. 改変版（Modified Version）に**予約名（Reserved Font Name）を使用しない**。本フォントの予約名は **`Source` のみ**で、`Noto` は予約名ではない。
4. 著作権者・著作者名を改変版の宣伝・推奨に使用しない（貢献の acknowledgment を除く）。
5. フォントは改変の有無にかかわらず **OFL のまま**配布し、他のライセンスにしない。フォントで作成した文書は対象外。

### 5.3 同梱時の要件

- `crates/eightbitdo-gui/assets/fonts/OFL.txt` に**上流の OFL.txt を改変せず**配置する（現状は配置済み）。
- **サブセット化は可能**（Modified Version 扱い）。サブセット版も OFL で配布し、`Source` を名前に使わず、Adobe / Google の著作権表示を保持し、単体販売しない。アプリ内では「サブセット版である」旨を明記する。
- `Noto` は Google のブランドです。OFL は商標権を許諾しないため、公式 Noto と誤認させない（例: `Noto Sans JP (subset)` と表示）。
- **未対応（残課題）**: GUI の **About > Open Source Licenses** 等のライセンス画面は未実装です。現状 `OFL.txt` はリポジトリにあるだけで、バイナリには `include_bytes!` していません。バイナリ配布前に、OFL 全文と著作権表示、サブセットである旨を閲覧できる形で同梱してください（standalone のテキストファイル、またはライセンス画面）。
- アプリのライセンス（MIT / Apache-2.0）とフォントの OFL は独立です。アプリのライセンス条項で OFL の義務を制限しないでください。
- `egui` の `default_fonts` も有効なため、egui 同梱フォントの帰属（OFL / UFL / MIT / Bitstream Vera）も本ファイルとライセンス画面に追加する（§3.6.1）。

## 付録 A: SIL Open Font License 1.1 全文

```text
Copyright 2014-2021 Adobe (http://www.adobe.com/), with Reserved Font Name 'Source'

This Font Software is licensed under the SIL Open Font License, Version 1.1.
This license is copied below, and is also available with a FAQ at:
https://scripts.sil.org/OFL


-----------------------------------------------------------
SIL OPEN FONT LICENSE Version 1.1 - 26 February 2007
-----------------------------------------------------------

PREAMBLE
The goals of the Open Font License (OFL) are to stimulate worldwide
development of collaborative font projects, to support the font creation
efforts of academic and linguistic communities, and to provide a free and
open framework in which fonts may be shared and improved in partnership
with others.

The OFL allows the licensed fonts to be used, studied, modified and
redistributed freely as long as they are not sold by themselves. The
fonts, including any derivative works, can be bundled, embedded,
redistributed and/or sold with any software provided that any reserved
names are not used by derivative works. The fonts and derivatives,
however, cannot be released under any other type of license. The
requirement for fonts to remain under this license does not apply
to any document created using the fonts or their derivatives.

DEFINITIONS
"Font Software" refers to the set of files released by the Copyright
Holder(s) under this license and clearly marked as such. This may
include source files, build scripts and documentation.

"Reserved Font Name" refers to any names specified as such after the
copyright statement(s).

"Original Version" refers to the collection of Font Software components as
distributed by the Copyright Holder(s).

"Modified Version" refers to any derivative made by adding to, deleting,
or substituting -- in part or in whole -- any of the components of the
Original Version, by changing formats or by porting the Font Software to a
new environment.

"Author" refers to any designer, engineer, programmer, technical
writer or other person who contributed to the Font Software.

PERMISSION & CONDITIONS
Permission is hereby granted, free of charge, to any person obtaining
a copy of the Font Software, to use, study, copy, merge, embed, modify,
redistribute, and sell modified and unmodified copies of the Font
Software, subject to the following conditions:

1) Neither the Font Software nor any of its individual components,
in Original or Modified Versions, may be sold by itself.

2) Original or Modified Versions of the Font Software may be bundled,
redistributed and/or sold with any software, provided that each copy
contains the above copyright notice and this license. These can be
included either as stand-alone text files, human-readable headers or
in the appropriate machine-readable metadata fields within text or
binary files as long as those fields can be easily viewed by the user.

3) No Modified Version of the Font Software may use the Reserved Font
Name(s) unless explicit written permission is granted by the corresponding
Copyright Holder. This restriction only applies to the primary font name as
presented to the users.

4) The name(s) of the Copyright Holder(s) or the Author(s) of the Font
Software shall not be used to promote, endorse or advertise any
Modified Version, except to acknowledge the contribution(s) of the
Copyright Holder(s) and the Author(s) or with their explicit written
permission.

5) The Font Software, modified or unmodified, in part or in whole,
must be distributed entirely under this license, and must not be
distributed under any other license. The requirement for fonts to
remain under this license does not apply to any document created
using the Font Software.

TERMINATION
This license becomes null and void if any of the above conditions are
not met.

DISCLAIMER
THE FONT SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO ANY WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT
OF COPYRIGHT, PATENT, TRADEMARK, OR OTHER RIGHT. IN NO EVENT SHALL THE
COPYRIGHT HOLDER BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY,
INCLUDING ANY GENERAL, SPECIAL, INDIRECT, INCIDENTAL, OR CONSEQUENTIAL
DAMAGES, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
FROM, OUT OF THE USE OR INABILITY TO USE THE FONT SOFTWARE OR FROM
OTHER DEALINGS IN THE FONT SOFTWARE.
```

## 付録 B: `deny.toml` の雛形（未適用）

以下は将来 `cargo-deny` を導入する場合の設定案です。**本 PR では `deny.toml` を追加しておらず、CI も変更していません。** 追加は依存・workflow の変更にあたるため、独立レビュー（Tier 2 以上）を経て別 PR で行ってください。

```toml
[graph]
all-features = true

[licenses]
confidence-threshold = 0.9
allow = [
  "MIT",
  "Apache-2.0",
  "Apache-2.0 WITH LLVM-exception",
  "BSD-2-Clause",
  "BSD-3-Clause",
  "ISC",
  "Zlib",
  "0BSD",
  "BSL-1.0",
  "Unicode-3.0",
  "Unlicense",
  "CC0-1.0",
  "MPL-2.0",
  "OFL-1.1",
]

# GPL / AGPL / SSPL / LGPL（単独選択）は allow に無いため拒否される。
# `r-efi` の LGPL-2.1-or-later は MIT / Apache-2.0 を選択できるため、式として許可される。

[bans]
multiple-versions = "warn"
wildcards = "deny"

[sources]
unknown-registry = "deny"
unknown-git = "deny"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
```

> - `MPL-2.0` を allow するのは `uniffi`（直接依存）と `option-ext` のためです。`exceptions` は原則使わず、使う場合は理由と期限を PR に記録してください。
> - `epaint_default_fonts` の `LicenseRef-UFL-1.0` は SPDX 標準識別子ではないため、この allow リストだけでは通らない可能性があります。導入時に `[licenses] private` / `exceptions` のどちらで扱うかを決め、判断理由を記録してください。

## 付録 C: CI 追加案（未適用）

`cargo-deny` と公式資産スキャンを CI に追加する場合の案です。**本 PR では `.github/workflows/ci.yml` を変更していません。** workflow と script の追加は独立レビュー必須（Tier 3 相当）です。

```yaml
  licenses:
    name: licenses
    runs-on: ubuntu-latest
    timeout-minutes: 10
    steps:
      - uses: actions/checkout@<pin>
        with:
          persist-credentials: false
      - uses: dtolnay/rust-toolchain@<pin>
        with:
          toolchain: stable
      - uses: Swatinem/rust-cache@<pin>
      - uses: taiki-e/install-action@<pin>
        with:
          tool: cargo-deny
      - run: cargo deny check licenses bans sources

  assets:
    name: no-proprietary-assets
    runs-on: ubuntu-latest
    timeout-minutes: 5
    steps:
      - uses: actions/checkout@<pin>
        with:
          persist-credentials: false
      - run: bash scripts/check-proprietary-assets.sh
```

`scripts/check-proprietary-assets.sh` の案:

```bash
#!/usr/bin/env bash
set -euo pipefail

bad_ext='\.(apk|exe|dll|so|dylib|pdb|lib|dat|hex|img|fw|ipa|aab)$'
if git ls-files | grep -Ei "$bad_ext"; then
  echo "ERROR: proprietary asset extension detected" >&2
  exit 1
fi

# テスト fixture として必要な場合は、ここで明示的に許可する
echo "OK: no proprietary assets"
```

> `*.bin` はバックアップやテスト fixture でも使われ得るため、資産スキャンの禁止拡張子には入れず、`.gitignore` とパス限定で扱っています。

fixture の配置規約: `.dat` / `.bin` を含むテスト fixture は `tests/fixtures/` または `crates/<crate>/tests/fixtures/` に置きます（`.gitignore` はこの 2 つのパスだけを例外にしています）。それ以外の場所の `.dat` / `.bin` は公式資産として除外されます。
