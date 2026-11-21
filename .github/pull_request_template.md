## 変更内容

<!-- 何を、なぜ変更したか。仕様上の根拠（docs の該当箇所）も書いてください。 -->

## Review Tier

該当する **最大 Tier のみ** チェックしてください。詳細は [docs/REVIEW_TIERS.md](docs/REVIEW_TIERS.md) を参照。

- [ ] **Tier 1**: コメント・整形、通常の docs（`AGENTS.md` / `CONTRIBUTING.md` / `docs/BRANCH_PROTECTION.md` / 規範仕様 / `.github/**` を除く）、純粋なテスト追加
- [ ] **Tier 2**: 通常のコード（書き込み経路を除く）、読み取り系ビルダー、プロセス文書、依存・ツールチェーン、リファクタ、テスト変更
- [ ] **Tier 3**: 実機書き込み / Flash / OTA / backup-restore / CRC / offset / framing / マクロ step / セキュリティ / ゲート実装 / リリース

判定理由:

<!-- 例: config_bt2.rs と commands.rs の書き込み経路を変更するため Tier 3 -->

## 故障リスト（Tier 2 / 3）

Tier 1 はこのセクションを削除可。件数はコードの下限であり、docs のノルマではありません。

| 故障 | 検出方法（テスト / コマンド / 目視） | 結果 | 残存リスク |
| --- | --- | --- | --- |
|  |  | 検出 / 未検出 |  |

## 独立レビュー

| 役割 | レビュアー（セッション ID） | レビューした SHA | 判定 |
| --- | --- | --- | --- |
| 仕様検収 |  |  | マージ可 / 要修正 |
| 実装 / 回帰 |  |  | マージ可 / 要修正 |
| 敵対的パス（Tier 3） |  |  | マージ可 / 要修正 |

- [ ] 全レビュアーが **同じ head SHA** で「マージ可」を出した
- [ ] リードレビュアーが `review-gate` status を付けた（description に Tier と SHA）
- [ ] レビューコメントをすべて解決した

## 検証

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- [ ] `cargo clippy -p eightbitdo-protocol --no-default-features --all-targets --locked -- -D warnings`
- [ ] `cargo test --workspace --locked`
- [ ] `cargo test --workspace --all-features --locked`
- [ ] `cargo test -p eightbitdo-protocol --no-default-features --locked`
- [ ] `cargo check -p eightbitdo-protocol --no-default-features --all-targets --locked`
- [ ] `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --locked`
- [ ] `rustup toolchain install 1.85.0`
- [ ] `cargo +1.85.0 check --workspace --all-targets --all-features --locked`
- [ ] `cargo +1.85.0 check -p eightbitdo-protocol --no-default-features --all-targets --locked`
- [ ] `docs/PROTOCOL_SPEC.md` / `docs/STRUCT_BT2_2768B.md` との整合性を確認した
- [ ] 予約領域・未知バイトを破壊しないことを確認した
- [ ] 実機で未検証の項目は、その旨を明記した

### Tier 3 追加チェック

#### プロトコル / Flash / OTA

- [ ] モックトランスポートでパケット列・CRC・チャンク境界を検証した
- [ ] 4KB セクタ境界、10 秒 erase タイムアウト、64B 固定チャンクを確認した
- [ ] OTA: `EraseFlash 0x0004` → `WriteFirmware 0x0003`（64B + CRC-16/MODBUS）→ `WriteVersion 0x00C4` → `WriteFirmwareVersion 0x0002` → 必要なら `WriteAPPID` → `JumpApp 0x0006` の順序を確認した
- [ ] 失敗時に `"8bt"` ブートローダーへ再接続して復旧できることを確認した
- [ ] 実機書き込み前に 2768B 設定とマクロ 4×3 スロットをバックアップし、ファイル名とハッシュを記録した
- [ ] バックアップからの復元手順を確認した

#### ゲート / セキュリティ / リリース

- [ ] ruleset の conditions / bypass_actors / required checks を監査した
- [ ] workflow の権限、status 偽装の可否、secret の扱いを確認した
- [ ] action の pin、署名、配布物、ロールバック手順を確認した
