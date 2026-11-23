# 配布・ライセンス・マネタイズ方針（決定文書）

- **決定日**: 2026-11-23（JST）
- **基準**: `main` `c169392`（2026-11-23）
- **対象**: `haruki0420/8bitdo-ultimate2-suite`
- **位置づけ**: リポジトリの公開範囲・コードライセンス・商標・同梱資産・ストア配布・マネタイズに関する**決定の正本**。README / AGENTS.md / TRADEMARKS.md / THIRD_PARTY_LICENSES.md は本ドキュメントの決定に従い、矛盾する場合は本ドキュメントを優先する。ただし帰属表示・商標表記の細目は、それぞれの文書を正本とする。
- **表記**: **決定** = 2026-11-23 に確定した方針。**未決定／要判断** = 決めていない項目（§7 に列挙）。**未実施** = 方針は決めたが作業が済んでいない項目。推測は「見込み」と書き、事実と区別する。
- **免責**: **本ドキュメントは法的助言ではない。** 法令・ストア規約・税務は変更が速く、解釈は法域で変わる。公開・課金・ストア申請の最終判断は、知的財産権と税務を扱う専門家に確認すること。

---

## 0. 結論

| 領域 | 決定（2026-11-23） |
| --- | --- |
| リポジトリ公開 | **public を維持**する |
| コードライセンス | **MIT OR Apache-2.0**。`LICENSE-MIT` / `LICENSE-APACHE` と Cargo 宣言を一致させる |
| ドキュメント | 特記がない限りコードと同じ MIT OR Apache-2.0 |
| CLA | **導入しない**。当面は inbound=outbound（§2.4） |
| 商標 | 説明的使用のみ。公式バイナリ・ファーム・ロゴを同梱しない。アプリ名に商標・製品名を使わない |
| 同梱フォント | Noto Sans JP（OFL-1.1）と egui 既定フォントを同梱する |
| ライセンス帰属表示 | **実装済み**（GUI のライセンス画面に全文を埋め込み）。配布時に §4.4 を再確認する |
| ストア配布 | 無料配布・IAP なし。MPL-2.0 の runtime 依存と両立させ、GPL / LGPL は選択しない（§5.2・§5.3） |
| マネタイズ | 寄付・スポンサー・有料サポート。ストア課金は条件付きの将来案 |

---

## 1. リポジトリ公開方針

### 1.1 決定: public を維持（2026-11-23）

リポジトリは public のまま維持する。private 化および source-available ライセンスへの転換は行わない。

**理由**

- **OSS の再現性**: 第三者がビルド・実行・検証できることが、このプロジェクトの価値の中心。プロトコル仕様は実機と公式バイナリの解析に基づくため、検証可能性を失うと仕様の信頼も失う。
- **RE 証跡の透明性**: 仕様の根拠（オフセット・コマンド・CRC・順序・実機証跡）を公開したまま、第三者が追試できる状態を保つ。private にすると追試ができなくなる。
- **貢献**: Issue / PR / バグ報告を受けられる。レビュー Tier 体制（[REVIEW_TIERS.md](REVIEW_TIERS.md)）と `main` 保護は整備済みで、外部からの変更を受け入れる土台がある。
- 公開するのは機能仕様と実装に限り、RE の生データと公式資産は公開しない（§1.2）。この線引きが public 維持の前提。

**採用しない案と理由**

- **private**: 検証可能性を失う。star / fork が 0（2026-11-22 時点）で移行コストは低いが、公開した履歴は取り消せず、後から公開する方が不利になる。
- **source-available（BSL / FSL 等）**: 収益源が「ホスティング競合の排除」ではないため、BSL の効力が刺さる場面が乏しい。一方で「OSS と名乗れない」「ディストリのパッケージ対象外になり得る」「コミュニティの反発」というコストは残る。
- **分割公開（コア public / アプリ private）**: ストア資産と署名鍵は公開しない方針（§5.6）だが、リポジトリ全体の分割は現時点では行わない。必要になった時点で再判断する。

### 1.2 公開するもの / 公開しないもの

| 区分 | 内容 | 扱い |
| --- | --- | --- |
| 仕様・実装 | `docs/` のプロトコル仕様・構造体・実機証跡、`crates/`、`mobile/` のコード | 公開 |
| RE 生データ | 逆コンパイル出力、文字列・関数ダンプ、btsnoop ログ、公式アプリの画像・ロゴ・UI 素材 | 公開しない（`.tmp/` と `.gitignore` による除外を維持） |
| 公式バイナリ・ファームウェア | `.apk` / `.exe` / `.dll` / `.so` / ファームウェア `.dat` 等 | 公開しない。**コミット禁止**（AGENTS.md §1・§4） |
| 署名・ストア資産 | 証明書、keystore、provisioning profile、ストア用スクリーンショット・説明文 | 公開しない（`.gitignore` 拡張は未実施） |

**決定**: この線引きを維持する。ファームウェアは自前でミラーせず、公式配布元へリンクする。

### 1.3 決定時点の事実

- GitHub の visibility は **PUBLIC**（`gh repo view`、2026-11-23 確認）。
- `git log --all --diff-filter=A` に公式バイナリ（`.apk` / `.exe` / `.dll` / `.so` / `.dylib` / `.ipa` / `.aab` / `.dat`）の追加履歴は **0 件**（2026-11-23 確認）。
- 追跡ファイルに画像・スクリーンショット（png / jpg / jpeg / gif / webp / pdf）は **0 件**（2026-11-23 確認）。
- `.tmp/` は `.gitignore` 済みで、履歴にも混入していない（2026-11-22 の調査）。決定の根拠に使った調査メモは `.tmp/` にあり、リポジトリには含まれない（§8.2）。
- 脆弱性報告の窓口（`SECURITY.md`）は未整備。public リポジトリで私的な報告経路がないと、脆弱性が公開 Issue に出て修正前に悪用されるリスクがある。書込・OTA の公開前に窓口を整備する（未実施・別 PR）。

---

## 2. コードライセンス

### 2.1 決定: MIT OR Apache-2.0（2026-11-23）

- リポジトリ直下の `LICENSE-MIT` と `LICENSE-APACHE` をライセンス全文の正本とし、`Cargo.toml` の `[workspace.package] license = "MIT OR Apache-2.0"`（15 行目）と一致させる。**両者は一致済み**（2026-11-23 確認）。
- 利用者は MIT または Apache-2.0 のいずれかを選択できる。Rust エコシステムの慣例に合わせる。一般に Apache-2.0 には特許許諾と NOTICE の扱いが含まれるため、`NOTICE` を維持する（法的助言ではない）。
- GitHub の license 検出は `apache-2.0` と表示される（2026-11-23 確認）。これは検出の表示であり、実際の許諾条件は `MIT OR Apache-2.0`。README と Cargo 宣言が正で、表示に合わせてライセンスを変えない。
- ライセンスの変更（GPL 化、source-available 化、デュアルライセンス化）は行わない（§1.1・§2.3）。

**根拠**: `.tmp/visibility/PUBLIC_OR_PRIVATE.md` の推奨（Cargo 宣言との整合、ストア配布との両立）と、`LICENSE-MIT` / `LICENSE-APACHE` / `NOTICE` / `THIRD_PARTY_LICENSES.md` の整備結果。Rust の慣例として MIT / Apache-2.0 のデュアルが広く使われている。

### 2.2 ドキュメントの扱い

**決定**: `docs/` と `README.md` などのドキュメントは、特記がない限りコードと同じ `MIT OR Apache-2.0`。第三者に由来する部分（同梱フォントの OFL、第三者コードの BSD / Unicode / MPL 表示、引用・出典）は、それぞれの権利者のライセンス条件に従う。

将来 CC BY 4.0 などの別ライセンスへ変更する場合は、変更時に README と本ドキュメントへ明記する。現時点では変更しない。

### 2.3 CLA は導入しない（決定）

- **決定**: CLA（著作権譲渡・再ライセンス許諾）は現状導入しない。理由は、ライセンスが permissive のみで、デュアルライセンスや source-available へ転換する予定がないこと。CLA は貢献者の負担になり、小規模プロジェクトでは参加をためらわせる要因になる。
- **受け入れる制約**: CLA がない状態で外部貢献を受け入れると、将来デュアルライセンスや再ライセンスへ転換するには全貢献者の許諾が必要になり、転換は困難になる見込み。この制約を前提に、ライセンス変更を行わない方針（§2.1）と整合させる。
- 将来 CLA が必要になった場合（有料化・デュアルライセンスの判断が出た場合）は、**最初の外部貢献を受け入れる前**に決める。後からでは権利処理が困難（§7）。

### 2.4 貢献時のライセンス同意（DCO 相当）

- **決定**: 当面は DCO の `Signed-off-by` を必須にしない。**inbound=outbound**（PR の提出をもって、その貢献をプロジェクトのライセンス `MIT OR Apache-2.0` で提供することに同意したものとみなす）を扱いとする。
- **未実施**: `CONTRIBUTING.md` には現在、ライセンス同意・DCO・CLA の記述がない（2026-11-23 確認）。上記の扱いを一文で明記する変更を別 PR で行う。本ドキュメントで決定を先に固定し、CONTRIBUTING はこれに追従する。
- inbound=outbound の法的効力の評価は本ドキュメントの範囲外（法的助言ではない）。

---

## 3. 非公式・商標

### 3.1 決定: 説明的使用に限る（2026-11-23）

- "8BitDo" は Shenzhen Bestodo Tech Co., Ltd. の登録商標（米国登録番号 5,830,011）。"Ultimate 2" は同社の製品名で、単独の商標登録の有無は未確認。[TRADEMARKS.md](../TRADEMARKS.md) を表記の正本とする。
- 文章・README・ストア説明で "8BitDo Ultimate 2" と書くのは、対応機種を事実として説明する**説明的使用（nominative / descriptive use）**に限る。`official` / `authorized` / `sponsored` / 公認 / 提携 などの表現は使わない。
- 非提携の明記を README 冒頭・TRADEMARKS.md・ストア説明に置く。現状の記述を維持する。
- 日本での商標登録は未確認（§7）。

### 3.2 同梱しないもの（決定）

- 公式バイナリ（APK / EXE / DLL / SO 等）と公式ファームウェア。
- ロゴ・アイコン・ワードマーク画像、公式アプリのスクリーンショット・UI 素材。
- `.gitignore` による除外を維持する。CI での資産スキャン（`scripts/check-proprietary-assets.sh` 相当）は未導入で、導入には workflow の変更が必要（[THIRD_PARTY_LICENSES.md](../THIRD_PARTY_LICENSES.md) 付録 C）。

### 3.3 アプリ名に商標・製品名を使わない（決定・適用済み）

- 旧表示名は **"Ultimate2 Mobile"**（iOS `CFBundleDisplayName` / Android `app_name`）。8BitDo を含まないが、同社の製品名 "Ultimate 2" を含むため本節の決定に適合せず、2026-11-23 に自前名称 **Padbench** へ変更した。
- **決定（2026-11-23 適用）**: ストア掲載名・アプリ表示名に、第三者の商標（8BitDo）だけでなく**同社の製品名 "Ultimate 2" も使わない**。自前の名称（表示名 **Padbench**）を使い、製品名は「対応機種」の説明文に限る（例: 説明文の "for 8BitDo Ultimate 2" は説明的使用）。
  - 理由: Apple 5.2.1 では、アプリ名・サブタイトル・アイコンでの第三者のブランド利用が典型的な拒否理由になるとされる。Google Play も知的財産・なりすましポリシーでメタデータを審査する。製品名を名前に含めると、非公式の明記だけでは公式アプリとの混同リスクが残る。
  - **適用範囲**: Tauri デスクトップの `productName` / ウィンドウタイトル / `identifier`、egui 版の表示名とウィンドウタイトル、Android の `app_name` / `applicationId` / `namespace`、iOS の `CFBundleDisplayName` / `CFBundleName` / `bundleID`。リポジトリ名 `8bitdo-ultimate2-suite` は既存の GitHub URL を維持するため変更しない。
- 識別子: デスクトップ・モバイル共通で **`io.github.haruki0420.padbench`** に統一した（2026-11-23。iOS は `mobile/ios/xtool.yml`、Android は `applicationId`）。Apple Developer Services 上で永続なので以後変更しない。Android の Kotlin ソースパッケージ `dev.eightbitdo.mobile` と UniFFI の `dev.eightbitdo.mobile.core` は内部コード名前空間であり、表示名・識別子ではないため据え置く。
- iOS の `NSBluetoothAlwaysUsageDescription` は "your 8BitDo Ultimate 2 controller" と記述している。用途説明は説明的使用として維持し、審査ノートで非公式・非提携を説明する（§3.4）。

### 3.4 ストア審査向けの扱い（決定）

- Apple 5.2.1（知的財産）: 第三者の商標・著作物を許諾なくアプリ名・アイコン・メタデータに使わない。ロゴ・ワードマーク画像を使わない方針（§3.2）で対応する。
- **決定**: ストア申請時の App Review ノートに、次の 3 点を記載する。
  1. 非公式・非提携の独立プロジェクトであること。
  2. 対応機種の記述は説明的使用であること。
  3. 公式バイナリ・ファームウェア・ロゴを含まないこと。
- **決定**: OTA（ファームウェア更新）をストア版に載せる場合は、審査ノートで「更新対象は外部デバイスであり、アプリ自身の実行コードを変更しない」と説明する（Apple 2.5.2 と Google Play の実行コード ダウンロード制限への配慮）。OTA のストア公開は実機 wire の確定後とする（AGENTS.md §6.2）。

---

## 4. 同梱フォント

### 4.1 Noto Sans JP (subset)（SIL OFL 1.1）

- GUI は `crates/eightbitdo-gui/assets/fonts/` の `NotoSansJP-{Regular,Medium,Bold}.otf` を同梱する。上流は Noto Sans CJK JP 2.004 のサブセット（Modified Version）。OFL 全文は `crates/eightbitdo-gui/assets/licenses/OFL-NotoSansJP.txt` に置き、`crates/eightbitdo-gui/assets/fonts/OFL.txt` と**同一内容を維持**したうえで、`include_str!` でバイナリに埋め込み、ライセンス画面から閲覧できるようにする（§4.3・§4.4）。
- **決定**: 同梱を継続する。OFL-1.1 の義務を満たすように同梱する。
  - 著作権表示と OFL 全文を、ユーザーが容易に閲覧できる形で含める。
  - 予約名 `Source` をフォント名に使わない（`Noto` は予約名ではない）。
  - フォント単体を販売しない（アプリへの同梱は可）。
  - アプリ内でサブセット版である旨を明記し、表記は "Noto Sans JP (subset)" とする。公式 Noto と誤認させない。

### 4.2 egui 既定フォント（OFL-1.1 / UFL-1.0 / MIT）

- `eframe` の既定 feature `default_fonts` により `epaint_default_fonts` がバイナリに含まれる。内訳は Hack（MIT。DejaVu public domain / Bitstream Vera License を含む）、Noto Emoji（OFL-1.1）、Ubuntu Light（UFL-1.0）、emoji-icon-font（MIT）。
- **決定**: 同梱を継続する。上流 `epaint_default_fonts` 0.29.1 の `fonts/OFL.txt` / `fonts/UFL.txt` / `fonts/Hack-Regular.txt` / `fonts/emoji-icon-font-mit-license.txt` に対応する全文を `crates/eightbitdo-gui/assets/licenses/`（`OFL-NotoEmoji.txt` / `UFL-Ubuntu.txt` / `Hack-BitstreamVera.txt` / `emoji-icon-font-MIT.txt`）に保持し、`crates/eightbitdo-gui/src/licenses.rs` の `include_str!` でバイナリに埋め込んで、ライセンス画面から閲覧できる形で含める（standalone のテキストファイルは同梱しない。§4.3）。
- `LicenseRef-UFL-1.0` は SPDX 標準識別子ではない。`cargo-deny` を導入する場合は例外扱いを検討する（[THIRD_PARTY_LICENSES.md](../THIRD_PARTY_LICENSES.md) 付録 B）。
- Ubuntu Font Licence 1.0 は OFL 1.1 を基礎にしたライセンスで、単体販売の禁止と予約名 "Ubuntu" の扱いを持つ。egui 同梱の `Ubuntu-Light.ttf` は**未改変のままアプリに同梱**するため、改変・単体販売の条件には該当せず、帰属表示で足りると理解している（法的助言ではない）。

### 4.3 帰属の実装状況（実装済み・配布時に確認）

- **実装済み（2026-11-23）**: フォント本体は `include_bytes!` で GUI バイナリに同梱済み（`crates/eightbitdo-gui/src/theme.rs`）。ライセンス全文は `crates/eightbitdo-gui/assets/licenses/` とリポジトリ直下の `LICENSE-MIT` / `LICENSE-APACHE` に置き、`crates/eightbitdo-gui/src/licenses.rs` の `include_str!` でバイナリに埋め込む。GUI の左レール下部「ライセンス」ボタンから開く画面（`App::licenses_dialog`）で、このアプリの MIT / Apache-2.0 全文と、Noto Sans JP (subset)（OFL 1.1）・NotoEmoji（OFL 1.1）・Ubuntu-Light（UFL 1.0）・Hack（MIT + Bitstream Vera + DejaVu）・emoji-icon-font（MIT）の全文をユーザーが閲覧できる。**`epaint_default_fonts` 0.29.1 同梱の OFL.txt / UFL.txt はライセンス本文のみで著作権者行を含まない**ため、egui 既定フォントの著作権表示は画面に併記する（`licenses::ATTRIBUTIONS`）。著作権表示は各フォントの `name` テーブル（name ID 0）から取得し、無い場合は同梱ライセンス原文のヘッダを使う（emoji-icon-font は nameID 0 が無いので MIT 本文から）。Noto Sans JP はこれとは別系統で、同梱している上流 Google Fonts の OFL は冒頭行に著作権者行を含むため、その行（`Copyright 2014-2021 Adobe ...`）を著作権表示として取得する。
- **決定**: 帰属表示の欠落は**バイナリ配布（ストア配布を含む）のリリース blocker** とする。§4.4 を満たすまでバイナリを配布しない。

### 4.4 バイナリ配布前のリリースチェックリスト

- [x] `crates/eightbitdo-gui/assets/licenses/OFL-NotoSansJP.txt`（上流の OFL 全文、改変なし）をバイナリに埋め込む（`licenses.rs`）。`crates/eightbitdo-gui/assets/fonts/OFL.txt` と同一内容を維持する。
- [x] egui の 4 ライセンスファイル（`crates/eightbitdo-gui/assets/licenses/OFL-NotoEmoji.txt` / `UFL-Ubuntu.txt` / `Hack-BitstreamVera.txt` / `emoji-icon-font-MIT.txt`）をバイナリに埋め込む。
- [x] Noto Sans JP の著作権表示（`Copyright 2014-2021 Adobe (http://www.adobe.com/), with Reserved Font Name 'Source'`）を含める。
- [x] NotoEmoji（`Copyright 2013 Google Inc.`）、Ubuntu-Light（`Copyright 2011 Canonical Ltd.`）、Hack（Source Foundry Authors / Bitstream）、emoji-icon-font（John Slegers）の著作権表示を画面に併記する。
- [x] このアプリの `LICENSE-MIT` / `LICENSE-APACHE` 全文をバイナリに埋め込む（MIT の通知同梱・Apache-2.0 §4(a)）。
- [x] Noto Sans JP をサブセット版として表記する（"Noto Sans JP (subset)"。公式 Noto と誤認させない）。
- [x] Ubuntu Light（UFL-1.0）の帰属と UFL 全文を、閲覧できる形で含める。
- [x] ライセンス画面から、上記をユーザーが閲覧できる（`crates/eightbitdo-gui/src/app.rs` の `licenses_dialog`）。
- [ ] `THIRD_PARTY_LICENSES.md` の記載と実際の同梱物が一致している（リリース時に再確認）。
- [ ] **依存クレートの完全な第三者通知を `cargo-about` で生成し、配布物へ同梱する。** リポジトリの `THIRD_PARTY_LICENSES.md` は要約・非網羅であり、これ自体を完全な通知として配布しない（[THIRD_PARTY_LICENSES.md](../THIRD_PARTY_LICENSES.md) 冒頭・§5.3）。
- [x] 同梱する OFL 2 コピー（[`assets/fonts/OFL.txt`](../crates/eightbitdo-gui/assets/fonts/OFL.txt) と [`assets/licenses/OFL-NotoSansJP.txt`](../crates/eightbitdo-gui/assets/licenses/OFL-NotoSansJP.txt)）が上流 Google Fonts の OFL と**バイト一致**（4,388 B、git blob `1c9f4328…`）であることを確認した。上流は固定 commit [`295d98a7a0c17c68f1341eaeea354e7960ea70d3`](https://github.com/google/fonts/commit/295d98a7a0c17c68f1341eaeea354e7960ea70d3) の [`ofl/notosansjp/OFL.txt`](https://github.com/google/fonts/blob/295d98a7a0c17c68f1341eaeea354e7960ea70d3/ofl/notosansjp/OFL.txt)（blob [`1c9f43281b8f216c5461fe9ac729afbade7724e4`](https://api.github.com/repos/google/fonts/git/blobs/1c9f43281b8f216c5461fe9ac729afbade7724e4)）。`crates/eightbitdo-gui/src/licenses.rs` のテストがバイト長・先頭行・末尾改行・2 コピー一致を固定している。
- [x] CI が `cargo test -p eightbitdo-gui licenses` を常設で実行している（`cargo test --workspace --all-features` に含まれる）。リリース時は最新の CI が green であることを確認する。

OFL / UFL の条文解釈と同梱方法の最終判断は、専門家に確認すること（法的助言ではない）。

---

## 5. App Store / Google Play 配布

### 5.1 決定: 初期は無料配布・IAP なし（2026-11-23）

- モバイル（iOS / Android）は無料で配布する。機能制限・Paywall・広告・解析 SDK を入れない。
- アプリ内課金（IAP）、アプリ内の外部決済リンク、アプリ内の寄付リンクを置かない。寄付導線は README と Web サイト（アプリ外）に置く。
- 初期リリースの範囲は読み取りとバックアップ。書き込み・OTA は実機検証と Tier 3 レビューが揃うまで公開しない（AGENTS.md §6.2、[mobile/README.md](../mobile/README.md)）。

**根拠**: `.tmp/monetization/MONETIZATION.md` の推奨（ストアは「配布・信頼・レビュー獲得」のチャネルと割り切り、課金は後回し）と、実装フェーズ（モバイルの書き込み・OTA が未公開）の現状。

### 5.2 MPL-2.0（uniffi / option-ext）との両立（決定）

MPL-2.0 の依存は 2 系統あり、**runtime と build-only を分けて扱う**。

- **runtime（バイナリに組み込まれる）**:
  - `uniffi` 0.31.2 とそのランタイム側クレート（`uniffi_core` / `uniffi_meta` / `uniffi_pipeline`）。`cargo tree -e normal -p eightbitdo-uniffi` で確認（2026-11-23）。
  - `option-ext` 0.2.0（`eightbitdo-gui` → `dirs` → `dirs-sys` 経由。GUI バイナリに入る）。
- **build-only（配布物に含まれない）**: `uniffi_bindgen` / `uniffi_udl`（bindgen。`uniffi` の `cli` feature 経由。`cargo tree -e normal` に現れない）と proc-macro クレート（`uniffi_macros` / `uniffi_internal_macros`）。proc-macro はホストのコンパイル時にのみ動き、バイナリには含まれない（`cargo tree` の normal tree にはビルド用依存として現れる）。生成される Kotlin / Swift バインディングは `.gitignore` 済みでリポジトリに含まれない。

**決定**:

- crates.io の未改変版のみを使い、`[patch]` を置かない（2026-11-23 時点で未改変）。一般に、未改変のまま利用する限り MPL-2.0 は本プロジェクトのコードへ及ばず、アプリ全体の再ライセンスや全ソース開示は不要とされる（法的助言ではない）。
- バイナリ配布時は、**runtime の MPL-2.0 対象**（`uniffi` 系 runtime と `option-ext`）とソースの入手先（crates.io の該当バージョン）を通知する（MPL-2.0 §3.2）。
- `uniffi` / `option-ext` を改変する場合は、改変したファイルを MPL-2.0 で公開する。決定時点で改変の予定はない。
- MPL-2.0 はアプリ全体へのコピーレフトを及ぼさず、追加の利用制限も課さないため、ストア配布と両立し得る（一般論であり法的助言ではない）。

### 5.3 GPL / LGPL を選択しない（決定）

決定時点の第三者クレート 497 件（基準 main `6b39e23`。[THIRD_PARTY_LICENSES.md](../THIRD_PARTY_LICENSES.md) §1）で、GPL / AGPL / SSPL / EUPL / CDDL / EPL は **0 件**。LGPL は選択式ライセンスの中にのみ現れる。

- **決定**: 選択式ライセンス（`OR`）では MIT または Apache-2.0 を選び、**選択するライセンスベースで GPL / LGPL の義務を生じさせない**。GPL / LGPL を単独で課す依存は入れない。
  - `r-efi` 5.3.0 / 6.0.0（`MIT OR Apache-2.0 OR LGPL-2.1-or-later`、2 件）は `getrandom` の **UEFI ターゲット向け**依存。ホスト（Linux）の通常ビルドには含まれない（`cargo tree -e normal --workspace` で 0 件、2026-11-23 確認）。三択から MIT または Apache-2.0 を選択し、LGPL-2.1-or-later は選ばない。
  - `libdbus`（AFL-2.1 または GPL-2.0+ のデュアル）は **AFL-2.1** を選択。通常の Linux 配布では同梱せず、システム提供のものを使う。
  - `hidapi`（未導入）の上流 C 実装は GPL-3.0 / BSD-3-Clause / original の三択のため、**BSD または original** を選択する（`linux-static-hidraw` を推奨）。
- GPL は Apple App Store の利用規約と両立しにくいとされ、FSF も GPL ソフトの App Store 配布を認めていない（VLC for iOS が 2011 年に削除された事例）。この整理を前提に、ストア配布では GPL 系を避ける（法的助言ではない）。
- `cargo-deny` による機械的な拒否は未導入。導入は workflow の追加になるため別 PR（[THIRD_PARTY_LICENSES.md](../THIRD_PARTY_LICENSES.md) 付録 B・C）。

### 5.4 Apple の規定に関する注意

- **5.2.1（知的財産）**: Apple のガイドラインでは、第三者の商標・著作物を許諾なくアプリ名・アイコン・メタデータに使わないこととされている。§3.3 のとおり商標と製品名を表示名に使わず、ロゴ・ワードマーク画像も使わない。
- **2.5.2（自己完結）**: アプリは自己完結し、アプリの機能を変更するコードをダウンロード・実行してはならない。ファームウェアはアプリ自身の実行コードではないが、審査で説明を求められる可能性がある。**決定**: OTA をストア版に出す場合は、審査ノートでの説明を必須にする（§3.4）。**決定**: 審査で OTA が受け入れられない場合は、読み取り・設定書き込みのみのストア版とし、OTA はデスクトップ（CLI / GUI）に限定する。ストア版を理由に OTA の実装自体を削らない。
- **3.1.1（課金）**: デジタル商品の販売には IAP を使う。**決定**: アプリ内に外部決済リンク・寄付リンクを置かない。日本向けの外部リンク決済（iOS 26.2 以降、スマホソフトウェア競争促進法への対応）も初期は使わない（§5.1）。
- **署名・配布**: Apple Developer Program（99 USD/年）と TestFlight を使う。Google Play はデベロッパー登録 25 USD（一括）で、個人アカウント（2023-11-13 以降に作成）は 12 テスター × 14 日のクローズドテストを経ないと production 公開を申請できない。料率・条件は変更が速いため、申請時に一次情報を再確認する。

### 5.5 Google Play

- 個人アカウントのクローズドテスト要件（12 テスター × 14 日）を配布計画に組み込む。
- ストアのメタデータ（アプリ名・説明・スクリーンショット）は §3.3・§3.4 の方針に従う。Google Play も知的財産・なりすましポリシーでメタデータを審査する。
- Google Play はアプリが Play 以外から実行可能コードをダウンロードすることを制限している。**決定**: OTA を実装する場合は「署名検証・公式配布元・デバイス宛のみ」の 3 条件を満たす設計にし、審査で説明できるようにする（実装は未着手）。

### 5.6 署名・ストア資産（決定）

- 証明書・keystore・provisioning profile・ストア用スクリーンショット・説明文をリポジトリに置かない。
- `.gitignore` への署名ファイルの追加は未実施（別 PR）。
- アプリ・署名鍵・ストア素材を private リポジトリへ分離するかは、ストア公開の直前に判断する（§7）。

---

## 6. マネタイズ

### 6.1 決定: 初期は寄付・スポンサー・有料サポート（2026-11-23）

収益導線は次の 3 本とする。いずれも現時点では未整備。

1. **寄付・スポンサー**: GitHub Sponsors（個人スポンサー 0%、組織スポンサー最大 6%）、Ko-fi（tip 0%）。README・リリースノート・Web サイトに導線を置く。`FUNDING.yml` と Sponsors の有効化は未実施。
2. **有料サポート・カスタム開発**: 優先対応、導入支援、マクロ・プロファイル制作。料金表と免責を明記する。未着手。
3. **スポンサー付き機能（bounty）**: 具体の issue、金額、受け入れ条件をセットで提示する。未着手。

- 収益の使途（テスト端末、ホスト費用、ストア審査費）を公表する。
- 期待値は「支持の可視化」と位置づける。この規模・非公式性では、どの単一案も生活できる収益にはならない見込み（`.tmp/monetization/MONETIZATION.md` の試算はいずれも推測）。

### 6.2 条件付きの将来案: ストア課金（未決定）

tip IAP（1 商品 300〜600 円・機能解放なし）や有料アプリは、次のすべてを満たした場合だけ再検討する。現時点では未決定（§7）。

- 無料配布で 6 か月以上の実績（目安: DL 5,000 以上、レビュー 4.0 以上、致命的バグなし）。
- ライセンス（§2）と商標（§3）の整理が済んでいる。
- サポート・返金体制を維持できる。
- 有料化の需要シグナルがある。

デュアルライセンスは、CLA の要否（§2.3・§7）を最初の外部貢献の前に決めた場合のみ検討する。

### 6.3 行わないこと（決定）

- 広告・解析 SDK の導入（プライバシー・UX・コミュニティ反発。ストアのプライバシー表示は「収集なし」を維持する）。
- ファームウェア更新の有料化（brick の責任・公式ツールとの誤認・製造物責任のリスク）。ファームウェア更新は無料・自己責任・experimental 表記で維持する。
- アプリ内への外部決済リンク・寄付リンクの設置（ストア規約違反のリスク）。
- GPL のまま iOS App Store で配布すること。
- 8BitDo の名称・ロゴを前面に出した有料アプリ。

### 6.4 免責

本節は**法的助言・税務助言ではない**。手数料・料率・税務（消費税、プラットフォーム課税、源泉徴収、インボイス）は 2026-11 時点の公開情報に基づく整理であり、変更が速い。実行前に一次情報と専門家（弁護士・税理士）で確認すること。

---

## 7. 未決定事項（要判断）

| # | 項目 | 現状 | 決めるタイミング |
| --- | --- | --- | --- |
| 1 | プロダクト名・ストア掲載名の最終決定 | **決定済み（2026-11-23）**: 表示名 **Padbench**、識別子 `io.github.haruki0420.padbench`（desktop / mobile 共通）。ストアのメタデータもこの名前で申請する（§3.3） | — |
| 2 | CLA の要否 | 導入しない（§2.3）。将来デュアルライセンス / source-available へ転換する場合のみ必要 | 最初の外部貢献を受け入れる前、または有料化を決めた時点 |
| 3 | 有料化の時期・内容 | 初期は寄付・サポート・bounty（§6.1）。tip IAP・有料アプリは条件付きの将来案（§6.2） | 無料配布 6 か月後の実績レビュー時 |
| 4 | App Store 審査方針 | 非公式・説明的使用で申請する方針は決定（§3.4）。OTA をストア版に含めるか、審査ノートの文面は未決定。リジェクト時は §5.4 の代替方針（OTA をデスクトップ限定）に従う | OTA の実機検証完了後、申請前 |
| 5 | 日本での 8BitDo 商標の確認 | J-PlatPat 未確認。米国登録（5,830,011）のみ確認 | ストア申請前（必要なら弁理士に相談） |
| 6 | ストア資産の置き場所 | 公開リポジトリに置かないことは決定（§5.6）。private リポジトリへの分離可否は未決定 | ストア公開前 |

---

## 8. 決定の根拠資料

### 8.1 リポジトリ内（コミット済み）

- [LICENSE-MIT](../LICENSE-MIT) / [LICENSE-APACHE](../LICENSE-APACHE) / [NOTICE](../NOTICE)
- [THIRD_PARTY_LICENSES.md](../THIRD_PARTY_LICENSES.md)（基準 main `6b39e23`、第三者クレート 497）
  - 注: THIRD_PARTY_LICENSES.md の基準は main `6b39e23`、本ドキュメントの基準は main `c169392`。依存数・ライセンス式は依存の追加・更新で変わるため、配布・ストア申請の前に再確認する。
- [TRADEMARKS.md](../TRADEMARKS.md)
- [README.md](../README.md) のライセンス節・免責
- [AGENTS.md](../AGENTS.md) §1・§3・§4
- [REVIEW_TIERS.md](REVIEW_TIERS.md) §2 content-based override
- `Cargo.toml`（`license = "MIT OR Apache-2.0"`）
- `mobile/ios/Info.plist` / `mobile/android/app/src/main/res/values/strings.xml`

### 8.2 調査メモ（リポジトリ外・`.tmp/`）

- `.tmp/visibility/PUBLIC_OR_PRIVATE.md`（2026-11-22。public / private / source-available の比較と推奨）
- `.tmp/monetization/MONETIZATION.md`（2026-11-22。マネタイズ比較と推奨）

いずれも `.gitignore` 済みでリポジトリに含まれない。決定の根拠として参照したが、外部からは参照できない。

### 8.3 一次資料（主なもの）

- GitHub「Licensing a repository」: <https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/licensing-a-repository>
- Apple App Store Review Guidelines（5.2.1 / 2.5.2 / 3.1.1）: <https://developer.apple.com/app-store/review/guidelines/>
- Apple Developer Program: <https://developer.apple.com/programs/> / Small Business Program: <https://developer.apple.com/app-store/small-business-program/>
- Apple 日本向け配信条件: <https://developer.apple.com/support/app-distribution-in-japan/> / <https://www.apple.com/jp/newsroom/2025/12/apple-announces-changes-to-ios-in-japan/>
- Google Play サービス手数料: <https://support.google.com/googleplay/android-developer/answer/112622?hl=ja>
- Google Play 登録（25 USD）: <https://support.google.com/googleplay/android-developer/answer/6112435?hl=en> / クローズドテスト（12 テスター × 14 日）: <https://support.google.com/googleplay/android-developer/answer/14151465?hl=en>
- Google Play Device and Network Abuse: <https://support.google.com/googleplay/android-developer/answer/16559646>
- FSF「App Store GPL Enforcement」: <https://www.fsf.org/blogs/licensing/more-about-the-app-store-gpl-enforcement>
- 著作権法（e-Gov、30 条の 4・10 条 3 項）: <https://laws.e-gov.go.jp/law/345AC0000000048>
- USPTO 商標 8BITDO（登録 5,830,011）: <https://uspto.report/TM/88010697>
- SIL Open Font License 1.1: <https://scripts.sil.org/OFL>
- GitHub Sponsors 手数料・税: <https://docs.github.com/en/sponsors/sponsoring-open-source-contributors/about-sponsorships-fees-and-taxes>
- 国税庁 雑所得（No.1500）: <https://www.nta.go.jp/taxes/shiraberu/taxanswer/shotoku/1500.htm> / プラットフォーム課税（No.6568）: <https://www.nta.go.jp/taxes/shiraberu/taxanswer/shohi/6568.htm>

---

## 9. 変更手続きと見直し

- 本ドキュメントは**ライセンス・法務判断**を含むため、変更は [REVIEW_TIERS.md](REVIEW_TIERS.md) §2 の content-based override により **Tier 3** として扱う。docs-only でも「自動検証による独立レビュー省略」の対象外であり、独立レビューと `review-gate` を経る。
- 見直しのトリガー: ストア公開、外部貢献の受付開始、有料化・課金の開始、8BitDo または第三者からの連絡、ライセンス・依存の変更、年 1 回の定期見直し。
- §7 の未決定事項を決めたときは、本ドキュメントに**決定日と根拠を追記**する。決定を上書きする場合も削除せず、変更履歴として残す。
