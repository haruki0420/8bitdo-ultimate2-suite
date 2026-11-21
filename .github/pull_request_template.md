## 変更内容

<!-- 何を、なぜ変更したか。仕様上の根拠（docs の該当箇所）も書いてください。 -->

## 検証

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `cargo test -p eightbitdo-protocol --features serde`
- [ ] `cargo check -p eightbitdo-protocol --no-default-features`
- [ ] `docs/PROTOCOL_SPEC.md` / `docs/STRUCT_BT2_2768B.md` との整合性を確認した
- [ ] 予約領域・未知バイトを破壊しないことを確認した
- [ ] 実機で未検証の項目は、その旨を明記した

## 独立レビュー

- [ ] 著者以外のレビュアーがレビューし、`reviewed` ラベルを付けた
- [ ] レビューコメントをすべて解決した
