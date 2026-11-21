# 独立レビュー Tier

PR のリスクに応じてレビューの深さを変えます。目的は「docs に重すぎるレビューをしない」「危険な差分に軽すぎるレビューをしない」ことです。

**このファイルを Tier 判定の唯一の正本（single source of truth）とします。** `AGENTS.md` / `CONTRIBUTING.md` / PR テンプレートは要約とリンクだけを持ちます。

## 1. Tier 表

| Tier | 対象 | レビュー | 故障リスト |
| --- | --- | --- | --- |
| **1** | コメント・整形、通常の docs（規範仕様・ゲート文書を除く）、**純粋なテスト追加のみ** | 1 名。diff + テスト + リンク/コマンド | 振る舞いを変える場合のみ 1〜2 件（純粋なテスト追加はレビュアー側で 1〜2 件注入） |
| **2** | 通常のコード（書き込み経路を除く）、プロトコルのデータ構造・読み取り系ビルダー、プロセス文書、規範仕様の編集（CRC/offset/flash/OTA/framing 以外）、依存・ツールチェーン・リポジトリ衛生、テストの変更/削除、リファクタ | 2 名（仕様検収 + 実装/回帰）。変更ファイル + 直接の呼び出し側 + テスト | コード 3〜5 件。docs は検出方法 3〜5 件 |
| **3** | 実機書き込み / Flash / OTA / backup-restore / CRC / offset / framing / マクロ step、セキュリティ（trust-boundary / crypto / signature / authz / secrets）、ワークフロー・ゲート実装、リリース、大規模な混合差分 | 2 名 + 別セッションの敵対的パス（計 3 セッション）。関連ファイル全体 + 呼び出し側 + モック | コード 10 件以上。docs/ゲートは検出方法 5 件以上 |

### 判定ルール

- **混合 PR は最大 Tier**（docs + code なら code の Tier）。
- 著者が PR で Tier を提案し、**レビュアーは昇格できる。降格はできない。**
- 迷ったら 1 段上に倒す。
- 行数は目安にしない。10 行の CRC 変更は Tier 3、1000 行の README 整形は Tier 1。
- **セマンティックな変更**が Tier 3 のパスに入る場合は、差分が小さくても Tier 3。
- **セキュリティ関連の振る舞い変更**（認証/認可、暗号/署名検証、secret の扱い、trust-boundary の入力検証、権限変更）は、パスにかかわらず Tier 3。docs の文言だけの security 修正は Tier 2。
- **新しい crate / workspace member / ファイル**がデバイス書き込み・Flash・OTA・backup/restore・ゲート変更を引き起こせる場合は、ディレクトリにかかわらず Tier 3。
- Tier 3 のパスに対するコメント・整形のみの変更は、**意味が変わらないことをレビュアーが確認して記録した場合に限り** Tier 1/2 にできる。
- revert / hotfix / security fix は、触ったパスの最大 Tier を継承する。trust-boundary の振る舞いを変える security fix は Tier 3。緊急時の降格はしない。
- push によってリスク面が変わった場合（docs PR にコードが混ざった等）は、**再 Tier 判定と再レビュー**を行う。
- 「大規模」は行数ではなく、Tier 3 のリスク領域をまたぐかで判断する。

## 2. パス別のトリガー

### Tier 2 以上

- `AGENTS.md` / `CLAUDE.md` / `CONTRIBUTING.md`
- `docs/REVIEW_TIERS.md` / `docs/BRANCH_PROTECTION.md`
- `docs/PROTOCOL_SPEC.md` / `docs/STRUCT_BT2_2768B.md` / `docs/IMPLEMENTATION_PLAN.md`
  - **CRC / offset / flash / OTA / framing / マクロ手順の変更は Tier 3。** 誤字・リンク・表現の修正は Tier 2。
- `.github/pull_request_template.md`
- `Cargo.toml` / `Cargo.lock` / `rust-toolchain.toml`
- `.gitignore`（公式バイナリの除外が維持されているか確認する）
- `.github/dependabot.yml`

### Tier 3

- `.github/workflows/**` / `.github/CODEOWNERS`
- ruleset JSON、`review-gate` status の投稿スクリプト、required check 名、ゲート手順の実装
- `crates/eightbitdo-protocol/src/crc.rs`
- `crates/eightbitdo-protocol/src/header.rs`（フレーミング）
- `crates/eightbitdo-protocol/src/macro_data.rs`
- `crates/eightbitdo-protocol/src/config_bt2.rs` のオフセット / CRC / マクロ Flash 関連
- `crates/eightbitdo-protocol/src/commands.rs` の設定書き込み / マクロ / OTA コマンド
- `crates/eightbitdo-core/**` の書き込み・セッション・OTA・backup/restore
- `crates/eightbitdo-cli/**` の書き込みコマンド
- `crates/eightbitdo-uniffi/**` の書き込み / OTA 公開 API
- `packaging/**` / リリース設定
- `unsafe` / FFI / ファームウェアのダウンロード・検証 / secrets / build script
- `AGENTS.md` / `docs/REVIEW_TIERS.md` の **フラッシュ順序・バックアップ・OTA 復旧** など安全-critical な手順
- 上記の領域に**新規ファイルを追加**する場合

> **同一ファイル内の判定**: `docs/BRANCH_PROTECTION.md` / `CONTRIBUTING.md` の散文・リンク修正は Tier 2。ruleset JSON、required checks、status 投稿スクリプト、ゲートを検証/投稿するコマンドの変更は Tier 3。

> **安全-critical な記述はファイル名で判断しない**: Flash 順序、backup/restore、OTA 復旧、マクロ Flash 手順、ゲート手順を変更する docs/チェックリスト（PR テンプレート、`CONTRIBUTING.md` を含む）は Tier 3。

> **新規 crate / workspace member の write 能力**: `crates/eightbitdo-gui` など将来の crate を含め、デバイス書き込み・Flash・OTA・backup/restore・ゲート変更を引き起こせる新規ファイルは Tier 3。レビュアーは新規 crate の write 能力を確認する。

> 「レビューゲート」の Tier 3 トリガーは **ゲートの実装**（workflow / ruleset / status 自動化 / required check）を指します。ゲートを**説明するだけ**の docs 変更（`BRANCH_PROTECTION.md` の文言、PR テンプレートの説明）は Tier 2 です。

## 3. Tier 1（軽量）

- コメント、整形、通常の docs、**純粋なテスト追加**。
- コードのリファクタは Tier 1 にしない（Tier 2 以上）。
- テストの変更・削除は Tier 1 にしない（Tier 2）。
- 規範仕様（`PROTOCOL_SPEC` / `STRUCT` / `IMPLEMENTATION_PLAN`）、ゲート文書、プロセス文書は Tier 1 にしない。
- レビュアー 1 名。diff、関連テスト、リンク、コマンド例を確認する。
- 振る舞いを変えない docs/コメント/整形は、故障注入なし。
- 純粋なテスト追加では、レビュアー側で 1〜2 件の故障を注入し「テストが落ちるか」を確認する。振る舞いを変える docs は Tier 2/3 とし、故障リストを必須にする。

## 4. Tier 2（標準）

- レビュアー 2 名。役割を分ける。
  - **仕様検収**: `docs/` との一致、オフセット、CRC、エンディアン、コマンド引数。
  - **実装/回帰**: API、エラー処理、境界値、呼び出し側、テストの抜け。
- 範囲は「変更ファイル + 直接の呼び出し側 + テスト」。変更ファイルだけを読んで終わらせない。
- 故障リストを 3〜5 件作り、それぞれ「検出方法」と「結果」を記録する。
  - コード: テストで検出する。
  - docs/プロセス: リンクチェック、grep、コマンド実行、シナリオ通読で検出する（3〜5 件）。

## 5. Tier 3（安全-critical）

- レビュアー 2 名 + **別セッションの敵対的パス**（合計 3 セッション）。
  - 敵対的パスは、正常系の確認ではなく「仕様の矛盾」「境界値」「失敗系」「中断・再試行」「予約バイト破壊」を探す。
  - コスト制約がある場合も、実装レビュアーとは別のセッションで敵対的パスを 1 パス追加する（2 名 + 1 パスを維持）。
- 範囲は関連ファイル全体 + 呼び出し側 + モックトランスポート。
- 故障リストをコード 10 件以上、docs/ゲート 5 件以上作る。可能なら `cargo-mutants` などの自動変異も併用する。- **モックトランスポート検証は、プロトコル / トランスポート / Flash / OTA の変更に限り必須**。リリース・ゲート・セキュリティのみの変更では、ruleset 監査・権限監査・秘密情報チェックで代替する。
- 実機書き込みを含む場合は、**書き込み前の 2768B バックアップ**（ファイル名とハッシュを記録）と、復元手順の確認を必須にする。
- OTA の場合は次の順序と境界を確認する。

  ```text
  EraseFlash 0x0004 (4KB セクタ単位、10 秒タイムアウト)
  → WriteFirmware 0x0003 (64B 固定 + CRC-16/MODBUS)
  → WriteVersion 0x00C4
  → WriteFirmwareVersion 0x0002 (+ CRC16)
  → WriteAPPID (gamepad_type != 1 のときのみ、cmd 0x0002, offset 0x0801401C)
  → JumpApp 0x0006
  ```

  - 4KB セクタ境界、64B チャンク、最終チャンクの 64B 固定、CRC の範囲を確認する。
  - 失敗時は `"8bt"` ブートローダーへ再接続して復旧できることを確認する。
  - マクロの `Erase 0x0104 → Write 0x0103 → ヘッダ 0x0001 → Save 0x0006` と混同しない。

### Tier 3 の追加チェック（変更種別ごと）

- **プロトコル / Flash / OTA**: モックでのパケット列・CRC・チャンク境界、4KB 境界、10 秒 erase、64B 固定、復旧。
- **ゲート / セキュリティ**: ruleset 監査（`conditions` / `bypass_actors` / `current_user_can_bypass` / `required_status_checks` / `approvals 0`）、workflow 権限、status 偽装の可否、secret の扱い、action の pin。
- **リリース**: 署名・配布物・バージョン・ロールバック。

## 6. レビュアーの独立性と合議

- レビュアーは **著者とも互いとも別セッション** でレビューする。互いの指摘を見る前に、同じ diff / SHA に対して独立に判定する。
- Tier 2/3 は **全レビュアーが同じ head SHA で「マージ可」** を出すまでマージしない。
- 1 名でも blocker / 要修正を出したら `review-gate` の `success` を付けない。必要なら `failure` を付ける。
- **リードレビュアー**（仕様検収レビュアー）が、全員の判定を確認してから status を付ける。
- 判定が割れた場合は、Tier を 1 段上げるか、3 名目のレビューを追加する。マージしない。
- 同じ SHA の `failure` を、新しいレビューなしに `success` で上書きしない。誤りだった場合は理由を PR に残す。
- PR には「役割 / レビュアー（セッション ID）/ レビューした SHA / 判定」を記録する。

## 7. 故障リストの書き方

件数を満たすことではなく、**検出したい故障を先に列挙**します。カウントはコードの下限です。docs/ゲートはテストの有無ではなく、検出方法（リンクチェック、grep、コマンド実行、目視）で 3〜5 / 5+ 件を記録します。

**以下の表は例であり、実際のテスト結果ではありません。** PR では自分の故障を記入してください。

| 故障 | 検出方法（テスト / コマンド / 目視） | 結果 | 残存リスク |
| --- | --- | --- | --- |
| CRC の多項式を `0x8408` ↔ `0xA001` で取り違える | CRC テストベクタ | 例: 検出 | なし |
| `0x92C + slot * 0x0C` を `+ slot * 4` にする | SixAxis オフセットテスト | 例: 検出 | なし |
| `align32` を `align16` にする | マクロ長テスト | 例: 検出 | なし |
| RT/LT を入れ替える | マクロステップのバイト配置テスト | 例: 検出 | なし |
| BLE RX に `0x04` プレフィックスを要求する | 受信フレーミングテスト | 例: 検出 | なし |
| `cmd 0x0103` の 64B クランプを外す | チャンク分割テスト | 例: 検出 | なし |
| OTA の最終チャンクを 64B 未満で送る | ファームウェア分割テスト | **未検出** | 最終チャンクの 64B パディング未実装（Issue #2 で修正予定） |
| docs の相対リンクを壊す | リンクチェック | 例: 検出 | なし |
| Tier 表の記述が 2 箇所でずれる | grep / 目視 | 例: 検出 | なし |
| セキュリティトリガーが抜ける | チェックリスト照合 | 例: 検出 | なし |

検出できない場合は、テストを追加するか、Tier を上げるか、残存リスクとして PR に明記します。

## 8. レビュー記録

- レビュー結果は PR のレビューコメントに残す。
- レビューした head SHA に `review-gate` status を付ける（[docs/BRANCH_PROTECTION.md](BRANCH_PROTECTION.md)）。
- status の description に Tier と SHA を含める（例: `Tier 2 review of abc1234`）。
- 修正を push したら新しい SHA で再レビューする。前の SHA の status は引き継がれない。
- Tier、故障リスト、レビュアー表は PR 本文に残す。

## 9. 強制範囲

`main-protection` ruleset が強制するのは **`review-gate` status の存在** までです。Tier 判定・レビュー実施・故障リストの内容は手続きであり、write 権限者は迂回できます。強制力を上げる方法は [docs/BRANCH_PROTECTION.md](BRANCH_PROTECTION.md) の「セキュリティ上の限界」を参照してください。
