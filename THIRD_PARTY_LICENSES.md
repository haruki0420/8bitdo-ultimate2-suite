# 第三者ライセンス（Third-Party Licenses）

本プロジェクトが利用する第三者ソフトウェアと、同梱予定フォントのライセンス条件をまとめます。

> 本ドキュメントは**法的助言ではありません**。記載は公開情報・パッケージメタデータ・各ライセンス原文に基づく整理であり、法的結論ではありません。ライセンスの解釈・配布条件の充足・商標の扱いは、公開・配布・ストア申請の前に知的財産権を扱う弁護士に確認してください。
>
> また、依存バージョンと各プロジェクトのライセンスは将来変更され得ます。**依存を追加・更新するたびに再生成・再確認**してください。

## 1. サマリ

- 基準: base commit `80f8d66`（2026-11-23）の `cargo metadata --offline --format-version 1 --all-features`。
- パッケージ総数 134（workspace メンバー 3 を除く**第三者クレート 131**）。`license` フィールドが空のクレートは **0**。
- **GPL / AGPL / SSPL / EUPL / CDDL / EPL は 0 件。** すべて MIT / Apache-2.0 を選択可能。
- 直接依存はすべて `MIT OR Apache-2.0`（または同等の選択式）で、本プロジェクトの `license = "MIT OR Apache-2.0"` と整合します。

| SPDX 式 | 件数 |
| --- | ---: |
| `MIT OR Apache-2.0` | 93 |
| `MIT` | 20 |
| `Apache-2.0 OR MIT` | 6 |
| `Apache-2.0/MIT` | 3 |
| `Unlicense OR MIT` | 2 |
| `Unlicense/MIT` | 2 |
| `(MIT OR Apache-2.0) AND Unicode-3.0` | 1 |
| `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | 1 |
| `MIT/Apache-2.0` | 1 |
| `MIT/Apache-2.0/BSD-3-Clause` | 1 |
| `Zlib OR Apache-2.0 OR MIT` | 1 |
| **合計** | **131** |

## 2. 直接依存（workspace）

解決バージョンは `Cargo.lock` に基づきます。「利用箇所」は workspace 内のクレート名です。

| クレート | バージョン | SPDX | 利用箇所 | 備考 |
| --- | --- | --- | --- | --- |
| `btleplug` | 0.13.5 | `MIT/Apache-2.0/BSD-3-Clause` | `eightbitdo-core`（既定 feature `ble`） | 三択。本プロジェクトは MIT / Apache-2.0 を選択。Linux では D-Bus / BlueZ 経由 |
| `tokio` | 1.54.1 | `MIT` | `eightbitdo-core`, `eightbitdo-cli` | — |
| `tokio-util` | 0.7.20 | `MIT` | `eightbitdo-cli` | — |
| `serde` | 1.0.229 | `MIT OR Apache-2.0` | `eightbitdo-protocol`（任意 feature `serde`）, `eightbitdo-cli` | — |
| `serde_json` | 1.0.154 | `MIT OR Apache-2.0` | `eightbitdo-cli` | — |
| `clap` | 4.6.9 | `MIT OR Apache-2.0` | `eightbitdo-cli` | — |
| `thiserror` | 2.0.21 | `MIT OR Apache-2.0` | `eightbitdo-core` | — |
| `sha2` | 0.10.9 | `MIT OR Apache-2.0` | `eightbitdo-cli` | — |
| `toml` | 0.8.23 | `MIT OR Apache-2.0` | `eightbitdo-cli` | — |
| `uuid` | 1.26.1 | `Apache-2.0 OR MIT` | `eightbitdo-core` | — |
| `async-trait` | 0.1.92 | `MIT OR Apache-2.0` | `eightbitdo-core`, `eightbitdo-cli`（dev） | — |
| `futures-util` | 0.3.34 | `MIT OR Apache-2.0` | `eightbitdo-core`（任意 feature `ble`） | — |
| `hex` | 0.4.3 | `MIT OR Apache-2.0` | `eightbitdo-cli` | — |
| `unicode-ident` | 1.0.27 | `(MIT OR Apache-2.0) AND Unicode-3.0` | 推移的（proc-macro 経由） | **Unicode-3.0 の帰属義務が加算される**（§3.2） |

> Linux で実際にビルドされる集合は `--all-features` より小さくなります。macOS / Windows / Android 向けのクレート（`objc2*` / `windows*` / `jni*`）も同梱物に応じて帰属表示が必要です。

## 3. 帰属・ライセンス全文の扱い

本プロジェクトのコードは `MIT OR Apache-2.0` で提供され、全文は [LICENSE-MIT](LICENSE-MIT) / [LICENSE-APACHE](LICENSE-APACHE) にあります。依存側の選択式ライセンス（`OR`）は、本プロジェクトでは **MIT または Apache-2.0** を選択します。以下は追加で注意が必要なものです。

### 3.1 BSD-3-Clause（btleplug）

`btleplug` は `MIT/Apache-2.0/BSD-3-Clause` の三択のため、MIT / Apache-2.0 を選択する限り BSD の帰属義務は生じません。ただし安全側に倒し、参考として BSD-3-Clause の著作権表示を記載します。

- Copyright (c) 2020-2021, Nonpolynomial
- blurmac 由来部分: Copyright (c) 2017 Akos Kiss

BSD-3-Clause 全文:

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

### 3.2 Unicode-3.0（unicode-ident）

`unicode-ident` は `(MIT OR Apache-2.0) AND Unicode-3.0` のため、MIT / Apache-2.0 に**加えて** Unicode ライセンスの著作権表示と許可通知を同梱する必要があります。

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

### 3.3 選択式ライセンスの扱い

- `Unlicense OR MIT` / `Unlicense/MIT`（`memchr`, `same-file`, `walkdir`, `winapi-util`）: 本プロジェクトは **MIT** を選択します。MIT の著作権表示は各クレートの `LICENSE` に従います。
- `Zlib OR Apache-2.0 OR MIT`（`objc2-core-bluetooth`）: **MIT または Apache-2.0** を選択します。Zlib の帰属義務は生じません。
- `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`（`wasi`）: Apache-2.0 を選択します。
- Apache-2.0 の依存に `NOTICE` ファイルがある場合は、その NOTICE を保持する必要があります（Apache-2.0 §4(d)）。現行ツリーに NOTICE を持つ依存は**確認されていません**。GUI / モバイル依存の追加時に再確認してください。

### 3.4 予定依存（未追加）の注意

| クレート / 資産 | ライセンス | 注意 |
| --- | --- | --- |
| `egui` / `eframe` | `MIT OR Apache-2.0` | `default_fonts` に同梱フォント（Ubuntu-Light / Hack / NotoEmoji 等）が含まれ、それぞれ別ライセンス。GUI 追加時に帰属を本ファイルへ追記 |
| `hidapi` | Rust ラッパーは `MIT` | **上流 C 実装は GPL-3.0 / BSD-3-Clause / original の三択**。BSD または original を選択し、上流 `LICENSE.txt` を本ファイルへ追加。`linux-static-hidraw` を推奨（libudev 非依存） |
| `uniffi` | `MPL-2.0` | 唯一の弱コピーレフト。ファイル単位の義務。未改変で依存する限りアプリ全体の再ライセンスは不要。生成バインディング（Kotlin / Swift）のヘッダに MPL 表記が付くかは追加時に確認 |
| `dirs` | `MIT OR Apache-2.0` | — |
| Noto Sans JP | `SIL OFL 1.1` | §5 |

### 3.5 システムライブラリ（Cargo の外）

`cargo deny` は Cargo 依存しか検査しません。配布物に `.so` を同梱する場合（AppImage / Flatpak / インストーラ）は `ldd` で洗い出し、追加の帰属表示を行ってください。

| ライブラリ | ライセンス | 使われ方 | 対応 |
| --- | --- | --- | --- |
| libdbus | **AFL-2.1 または GPL-2.0+ のデュアル** | `btleplug` → `bluez-async` → `dbus`（Linux） | **AFL-2.1 を選択**。同梱する場合は AFL の帰属を追加。通常の Linux 配布（システム提供）なら同梱不要 |
| libudev | LGPL-2.1+ | `hidapi` の `linux-native` feature 使用時のみ | 既定の `linux-static-hidraw` なら不要。使う場合は動的リンクと LGPL 条件を確認 |
| BlueZ | GPL-2.0 | OS の D-Bus サービスとして利用 | リンク・同梱しない |
| GPU ドライバ（OpenGL / Vulkan / Metal） | 各ベンダ | GUI 描画 | 通常は同梱しない |

## 4. 生成・更新方法

本ファイルは `cargo metadata` の集計結果を初期版としています。依存の追加・更新時に次を実行して差分を確認してください。

```sh
cargo metadata --format-version 1 --all-features > /tmp/meta.json
# ライセンス式の集計例
python3 - <<'PY'
import json, collections
m = json.load(open('/tmp/meta.json'))
third = [p for p in m['packages'] if p['source'] is not None]
for lic, n in collections.Counter(p.get('license') for p in third).items():
    print(n, '|', lic)
PY
```

本格運用では次のいずれかを推奨します。

- **`cargo-deny`**: 許可リストで GPL 系の混入を機械的に拒否（§付録 B）。
- **`cargo-about`**: `cargo about generate` で帰属一覧を自動生成し、本ファイルの表を置き換える。
- CI で生成物の差分（drift）を検出する（§付録 C）。

## 5. 同梱フォント: Noto Sans JP（SIL OFL 1.1）

### 5.1 現状

- **本コミット（main 系の `docs/licensing`）にはフォント資産はありません。** GUI crate（`crates/eightbitdo-gui`）はまだ main に存在しません。
- GUI ブランチ（`feat/gui-redesign-jp`）には `crates/eightbitdo-gui/assets/fonts/` に `NotoSansJP-*.otf` と `OFL.txt` が追加されています。GUI を main に取り込む時点で、以下を満たしてください。
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

- `crates/eightbitdo-gui/assets/fonts/OFL.txt` に**上流の OFL.txt を改変せず**配置する。
- **サブセット化は可能**（Modified Version 扱い）。サブセット版も OFL で配布し、`Source` を名前に使わず、Adobe / Google の著作権表示を保持し、単体販売しない。アプリ内では「サブセット版である」旨を明記する。
- `Noto` は Google のブランドです。OFL は商標権を許諾しないため、公式 Noto と誤認させない（例: `Noto Sans JP (subset)` と表示）。
- GUI の **About > Open Source Licenses** 等に、OFL 全文と著作権表示、サブセットである旨を表示する。バイナリへの `include_bytes!` だけでは「easily viewed」を満たさない可能性があるため、UTF-8 のテキストも配布物に含める。
- アプリのライセンス（MIT / Apache-2.0）とフォントの OFL は独立です。アプリのライセンス条項で OFL の義務を制限しないでください。
- `egui` の `default_fonts` を使う場合、egui 同梱フォントの帰属も本ファイルとライセンス画面に追加する（§3.4）。

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
  "Unicode-3.0",
  "Unlicense",
  "CC0-1.0",
  "MPL-2.0",
]

# GPL / AGPL / SSPL は allow に無いため拒否される

[bans]
multiple-versions = "warn"
wildcards = "deny"

[sources]
unknown-registry = "deny"
unknown-git = "deny"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
```

> `MPL-2.0` を allow するのは `uniffi`（予定）のためです。`exceptions` は原則使わず、使う場合は理由と期限を PR に記録してください。

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
