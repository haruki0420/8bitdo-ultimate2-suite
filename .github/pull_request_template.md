## 変更内容

<!-- 何を、なぜ変更したか。仕様上の根拠（docs の該当箇所）も書いてください。 -->

## Review Tier

- [ ] **Tier 1**: docs / コメント / 整形 / 振る舞いを変えないリファクタ / テスト追加のみ
- [ ] **Tier 2**: 通常のコード / プロトコルのデータ構造・コマンドビルダー / CI・ツール / プロセス文書
- [ ] **Tier 3**: 実機書き込み / Flash / OTA / CRC / オフセット / フレーミング / レビューゲート / リリース / 大規模な混合差分

判定理由:

<!-- 例: crc.rs と commands.rs の書き込み経路を変更するため Tier 3 -->

## 故障リスト（Tier 2 / 3）

| 故障 | 検出したテスト |
| --- | --- |
|  |  |

## 検証

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- [ ] `cargo clippy -p eightbitdo-protocol --no-default-features --all-targets --locked -- -D warnings`
- [ ] `cargo test --workspace --locked`
- [ ] `cargo test --workspace --all-features --locked`
- [ ] `cargo test -p eightbitdo-protocol --no-default-features --locked`
- [ ] `cargo check -p eightbitdo-protocol --no-default-features --locked`
- [ ] `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --locked`
- [ ] `rustup toolchain install 1.85.0`
- [ ] `cargo +1.85.0 check --workspace --all-targets --all-features --locked`
- [ ] `cargo +1.85.0 check -p eightbitdo-protocol --no-default-features --all-targets --locked`
- [ ] `docs/PROTOCOL_SPEC.md` / `docs/STRUCT_BT2_2768B.md` との整合性を確認した
- [ ] 予約領域・未知バイトを破壊しないことを確認した
- [ ] 実機で未検証の項目は、その旨を明記した

### Tier 3 追加チェック

- [ ] モックトランスポートでパケット列・CRC・チャンク境界を検証した
- [ ] 実機書き込み前に 2768B 設定とマクロ 4×3 スロットをバックアップする手順を確認した
- [ ] OTA の場合、Erase → Write → Header → Save と `"8bt"` 復旧を確認した

## 独立レビュー

- [ ] 著者以外のレビュアーが Tier に応じてレビューした
- [ ] 「マージ可」の判定を得た
- [ ] レビューした head SHA に `review-gate` status (`success`) を付けた
- [ ] レビューコメントをすべて解決した
