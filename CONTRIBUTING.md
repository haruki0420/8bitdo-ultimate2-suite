# コントリビューションガイド

このリポジトリでは、`main` の履歴と品質を守るために **すべての変更を Pull Request 経由** で行います。AI エージェントも人間も同じルールです。

## 1. `main` の保護

`main` には [docs/BRANCH_PROTECTION.md](docs/BRANCH_PROTECTION.md) の ruleset を適用します（PR #1 のマージ前に有効化・検証）。

- `main` への直接 push 禁止（Pull Request 必須）
- 必須 CI: `fmt` / `clippy` / `test` / `msrv`
- 必須レビューゲート: **レビュー済みコミット SHA に紐づく `review-gate` commit status**
- force push・ブランチ削除禁止
- linear history（squash merge のみ）
- 未解決のレビューコメントがある限りマージ不可
- ブランチが `main` より古い場合はマージ不可（up-to-date 必須）

> `review-gate` はラベルではなく **commit status** です。新しいコミットを push すると SHA が変わり、status が無い新しいコミットは自動的にマージできなくなります。これにより「レビュー後にコミットを差し替えて通す」迂回を防ぎます。

## 2. 変更の流れ

1. Issue または PR の説明に、変更目的と仕様上の根拠を書く。
2. `main` から作業ブランチを切る。
   - `feat/...` / `fix/...` / `docs/...` / `chore/...`
3. 実装・テストを追加する。
4. ローカルで以下を通す。

   ```sh
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
   cargo clippy -p eightbitdo-protocol --no-default-features --locked -- -D warnings
   cargo test --workspace --locked
   cargo test --workspace --all-features --locked
   cargo test -p eightbitdo-protocol --no-default-features --locked
   cargo check -p eightbitdo-protocol --no-default-features --locked
   RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --locked
   rustup toolchain install 1.85.0
   cargo +1.85.0 check --workspace --all-targets --all-features --locked
   cargo +1.85.0 check -p eightbitdo-protocol --no-default-features --locked
   ```

5. PR を作成する。
6. CI がすべて通ることを確認する。
7. **著者以外**のレビュアー（別セッションの AI エージェント、または人間）が差分をレビューする。
8. レビュアーが PR にレビューを投稿し、**レビューした head SHA** に `review-gate` status を付ける。

   ```sh
   REPO=haruki0420/8bitdo-ultimate2-suite
   PR=123
   REVIEWED_SHA=<レビューした head SHA>

   current=$(gh pr view "$PR" --repo "$REPO" --json headRefOid --jq .headRefOid)
   if [ "$current" != "$REVIEWED_SHA" ]; then
     echo "head moved: reviewed=$REVIEWED_SHA current=$current" >&2
     exit 1
   fi

   gh api --method POST "repos/$REPO/statuses/$REVIEWED_SHA" \
     -f state=success \
     -f context=review-gate \
     -f description="Independent review of ${REVIEWED_SHA:0:7}" \
     -f target_url="https://github.com/$REPO/pull/$PR"
   ```

   修正が必要な場合は `state=failure` で同じ context に status を付け、修正後に新しい SHA へ `success` を付ける。`docs/BRANCH_PROTECTION.md` に確認コマンドと監査方法があります。

9. すべてのレビューコメントを解決し、squash merge する。

> `reviewed` ラベルは任意の目視用マーカーであり、マージ条件ではありません。必須なのは SHA に紐づく `review-gate` status です。

## 3. 独立レビューの観点

レビュアーは「作者の意図」ではなく、**仕様書と実装の一致** を確認します。

- [ ] `docs/PROTOCOL_SPEC.md` のコマンド / フレーミング / CRC と一致しているか
- [ ] `docs/STRUCT_BT2_2768B.md` のオフセット・サイズ・エンディアンと一致しているか
- [ ] 予約領域 `0x1F4..0x68B` / `0x914..0x92B` を破壊していないか
- [ ] 名前が UTF-16**BE**、数値が Little-Endian になっているか
- [ ] マクロの `align32`、4KB セクタ、Erase → Write → Header → Save の順序
- [ ] BLE 受信にプレフィックスを要求していないか（`0x600F`）
- [ ] USB HID の送信 `[0x81, 0x04]` / 受信 `[0x02, 0x04]` を混同していないか
- [ ] 失敗系（長さ不足、未知の値、予約バイト）のテストがあるか
- [ ] ドキュメントと実装の記述が矛盾していないか

## 4. レビュー status の限界

`review-gate` status は **SHA への紐づき** を強制しますが、**投稿者の身元** や **レビュー内容** までは強制しません。write 権限を持つ者は誰でも status を付けられます。同じ SHA への最新 status が有効なので、`failure` を `success` で上書きすることも可能です。

- 現状はソロ運用のため承認数 0 とし、独立レビューを手続きとして運用する。
- 同じリポジトリのブランチから実行される PR ワークフローは `statuses: write` を要求して status を偽装できる。PR 内で `ci.yml` を書き換えることもできる。**CI と status は悪意ある write 権限者に対する security boundary ではない。**
- 身元まで強制する場合は、2 人目のレビュアーアカウントまたはマシンユーザーを追加し、required approvals を 1 に上げる。
- または、信頼できる GitHub App の `integration_id` で `review-gate` の expected source を固定する。
- GitHub Actions の bot 承認は required approvals にカウントされない。

## 5. コミットメッセージ

Conventional Commits に近い形式を使います。

```text
feat(protocol): add macro write packet builder
fix(protocol): correct config write subcmd
docs: update review process
chore(ci): add review gate
```

## 6. 禁止事項

- 公式の `.apk` / `.exe` / `.dll` / `.so` / ファームウェア本体をコミットしない（`.gitignore` で除外済み）。
- 実機の 2768B 設定やマクロを、バックアップなしで上書きしない。
- `main` へ直接 push しない。
- レビューを受けずに `review-gate` status を自分で付けない。

## 7. AI エージェント向け

- 変更は必ずブランチ → PR で行う。
- レビューは **別セッションのエージェント** に依頼し、著者自身のセッションでレビューを完結させない。
- レビュー結果は PR のレビューコメントとして残す。
- レビューで blocker / major が見つかった場合は、修正して CI を再実行し、再度レビューを受ける。
- status を付ける前に、レビュー対象の `headRefOid` と自分のレビューした SHA が一致していることを確認する。
