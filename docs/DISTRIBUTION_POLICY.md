# 配布・ライセンス・マネタイズ方針（決定文書）

- 決定日: **2026-11-23**（基準: `main` `c169392`、対象: `haruki0420/8bitdo-ultimate2-suite`）
- 位置づけ: 公開範囲・ライセンス・商標・同梱資産・ストア配布・マネタイズの**決定の正本**（帰属表示・商標表記の細目は [THIRD_PARTY_LICENSES.md](../THIRD_PARTY_LICENSES.md) と [TRADEMARKS.md](../TRADEMARKS.md)）。
- 免責: 本書は法的助言・税務助言ではない。公開・課金・ストア申請の最終判断は専門家に確認すること。

## 1. 決定サマリ

| 領域 | 決定（2026-11-23） |
| --- | --- |
| リポジトリ公開 | **public 維持**（private 化・source-available 化しない） |
| コード / ドキュメント | **`MIT OR Apache-2.0`**（`LICENSE-MIT` / `LICENSE-APACHE` / `Cargo.toml` 一致済み、`NOTICE` 維持） |
| CLA / 貢献同意 | **CLA 非導入**、当面は DCO 必須にせず **inbound=outbound**（`CONTRIBUTING.md` への明記は別 PR） |
| 商標・アプリ名 | 説明的使用のみ。アプリ表示名・識別子に商標・製品名を使わず **Padbench** / `io.github.haruki0420.padbench` に統一 |
| 同梱フォント | Noto Sans JP (subset, OFL-1.1) と egui 既定 4 フォントを同梱。ライセンス全文と著作権表示を GUI 内に埋め込み済み |
| ストア配布 | **無料配布・IAP なし**。MPL-2.0（runtime）と両立させ、GPL / LGPL は選択しない |
| マネタイズ | 寄付・スポンサー・有料サポート・bounty（未整備）。ストア課金は条件付きの将来案 |

## 2. リポジトリ公開と資産の線引き

- **公開**: `docs/` の仕様・実機証跡、`crates/`、`apps/`、`mobile/` のコード。
- **公開しない（コミット禁止）**: RE 生データ（逆コンパイル出力・btsnoop）、公式バイナリ・ファームウェア（`.apk`/`.exe`/`.dll`/`.so`/`.dat` 等。履歴 0 件を確認済み）、公式ロゴ・UI 素材、署名鍵・keystore・provisioning profile・ストア素材。ファームウェアはミラーせず公式配布元へリンクする。
- 未実施: 脆弱性報告窓口（`SECURITY.md`）の整備（書込・OTA 公開前）、署名ファイルの `.gitignore` 拡張、CI 資産スキャン。

## 3. 商標・非公式表記

- "8BitDo" は Shenzhen Bestodo Tech Co., Ltd. の登録商標（米国登録 5,830,011、日本登録は未確認）。"Ultimate 2" は同社製品名。使用は対応機種を示す**説明的使用**に限り、`official` / `authorized` / `sponsored` / 公認 / 提携は使わない。
- 表示名・識別子は全プラットフォームで **Padbench** / `io.github.haruki0420.padbench` に統一済み（リポジトリ名 `8bitdo-ultimate2-suite` と内部パッケージ `dev.eightbitdo.mobile` は維持）。
- **ストア審査ノート必須 3 点**: ①非公式・非提携の独立プロジェクト ②対応機種は説明的使用 ③公式バイナリ・ファームウェア・ロゴ不使用。OTA を載せる場合は「更新対象は外部デバイスでありアプリ自身の実行コードを変更しない」旨を追記する。

## 4. 同梱フォントとバイナリ配布チェックリスト

帰属表示の欠落は**バイナリ配布のリリース blocker**とする。

- **実装済み（egui 版 `crates/eightbitdo-gui/src/licenses.rs`）**: `LICENSE-MIT` / `LICENSE-APACHE` 全文、`Noto Sans JP (subset)`（OFL-1.1、上流 Google Fonts `295d98a7` と 4,388B バイト一致、`Copyright 2014-2021 Adobe ...`、予約名 `Source` 不使用・単体販売禁止）、egui 既定 4 フォント（NotoEmoji OFL-1.1 / Ubuntu-Light UFL-1.0 / Hack MIT+Bitstream Vera+DejaVu / emoji-icon-font MIT）の全文と著作権者行をバイナリに埋め込み、「ライセンス」画面で閲覧可能（CI `licenses` テスト固定）。
- **配布前の残項目**:
  - [ ] `THIRD_PARTY_LICENSES.md` と実際の同梱物の一致を再確認する。
  - [ ] 依存クレートの完全な第三者通知を **`cargo-about`** で生成して配布物へ同梱する（`THIRD_PARTY_LICENSES.md` は要約のため代替不可）。

## 5. ストア配布と依存ライセンス

- **初期配布**: 無料・機能制限なし・広告/解析 SDK なし・アプリ内決済/寄付リンクなし（寄付導線は README・Web サイトのみ）。初期範囲は読取・バックアップで、書込・OTA は実機検証 + Tier 3 レビュー後に公開する。
- **MPL-2.0 との両立**: runtime に含まれる `uniffi` 0.31.2 系と `option-ext` 0.2.0 は crates.io 未改変版のみ使用（`[patch]` 禁止）し、配布時に該当対象とソース入手先を通知する（build-only の `uniffi_bindgen` / proc-macro は配布物に含まれない）。
- **GPL / LGPL 非選択**: 第三者クレート（基準 `6b39e23` で 497 件）に単独 GPL/LGPL は 0 件。選択式ライセンスは MIT / Apache-2.0 / AFL-2.1（`libdbus`）/ BSD（将来の `hidapi`）を選び、GPL/LGPL 義務を生じさせない。
- **ストア固有要件**:
  - Apple（Developer Program 99 USD/年）: 5.2.1（知的財産）、2.5.2（OTA が審査不可ならストア版は読取・設定書込のみとし OTA はデスクトップ限定）、3.1.1（アプリ内外部決済・寄付リンク禁止）。
  - Google Play（登録 25 USD、個人アカウントは **12 テスター × 14 日** のクローズドテスト必須）: OTA 実装時は「署名検証・公式配布元・デバイス宛のみ」の 3 条件を満たす設計にする。

## 6. マネタイズと禁止事項

- **初期導線（未整備）**: ①GitHub Sponsors / Ko-fi ②有料サポート・カスタム開発 ③スポンサー付き機能（bounty）。使途を公表する。
- **条件付き将来案（未決定）**: tip IAP（300〜600 円・機能解放なし）や有料アプリは、無料配布 6 か月以上（目安 DL 5,000+ / 評価 4.0+ / 致命バグなし）・ライセンス/商標整理・サポート体制・需要シグナルが揃った場合のみ再検討する。
- **行わないこと**: 広告・解析 SDK、OTA の有料化、アプリ内外部決済/寄付リンク、GPL での App Store 配布、8BitDo 名称・ロゴを前面に出した有料アプリ。

## 7. 未決定事項と変更手続き

| 項目 | 現状 | 判断時期 |
| --- | --- | --- |
| CLA の要否 | 非導入（デュアルライセンス転換時のみ必要） | 最初の外部貢献前または有料化決定時 |
| 有料化の時期・内容 | 未決定（条件付き将来案） | 無料配布 6 か月後 |
| OTA のストア同梱・審査ノート文面 | 未決定（不可時はデスクトップ限定） | OTA 実機検証完了後・申請前 |
| 日本での 8BitDo 商標確認 | J-PlatPat 未確認 | ストア申請前 |
| ストア資産の private 分離 | 公開リポジトリ非配置のみ決定 | ストア公開前 |

- 本書はライセンス・法務判断を含むため、変更は [REVIEW_TIERS.md](REVIEW_TIERS.md) により **Tier 3**（docs-only のレビュー省略不可）。決定を上書きする場合は削除せず履歴として残す。
