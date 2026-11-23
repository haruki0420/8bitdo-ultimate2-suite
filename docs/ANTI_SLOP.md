# AI スロップ（AI slop）を避ける運用ガイド

- 位置づけ: **推奨（非規範）**。レビュー checklist・Tier 判定の正本は [REVIEW_TIERS.md](REVIEW_TIERS.md)、デスクトップ GUI の技術スタック正本は [AGENTS.md](../AGENTS.md) §3 と [TAURI_PLAN.md](TAURI_PLAN.md)、デザイン正本は [DESIGN_TAURI.md](DESIGN_TAURI.md) と [design/IMPORTED_STYLE_HYPERSTUDIO.md](design/IMPORTED_STYLE_HYPERSTUDIO.md)。
- 対象: 正式 GUI の Tauri v2 版（`apps/desktop`）。egui 版（`crates/eightbitdo-gui`）は凍結済みで対象外。モバイルネイティブ（Kotlin / Swift）は色・フォント定数の静的検査と目視で代替する。

## 1. 原因とトリアージ

AI 生成 UI が量産的に見える主因は「判断の不在（統計的平均への回帰）」と「装飾のクローム漏れ」。適用量は 3 段階で判断する。

- **heavy（兆候が多い）**: 方向 5 行を決め、9 パスを順に通し、正本の検証サイズと 200% ズームで確認する（描画確認は最大 3 回）。
- **light（概ね設計済み）**: 監査で見つかった兆候だけを直し、色・タイポ・レイアウトの大きな変更は提案に留める。
- **no-slop（大きな兆候なし）**: 問題ないと明言し、小さな改善を 2〜3 件のみ具体値付きで提案する。
- 正本の決定を勝手に変えず、変える場合は [DESIGN_TAURI.md](DESIGN_TAURI.md) も同時に更新する。

## 2. 検出チェックリスト（推奨基準）

### 機械検出できる兆候（`apps/desktop` の `tokens.test.ts` / `style-audit.test.ts` / `terminology.test.ts`）

| 兆候 | Tauri v2 での検出 | 推奨基準 |
| --- | --- | --- |
| グラデーション・紫・ネオン | CSS / トークン走査 | 0 件（使用しない） |
| アクセント乱立・色付き塗り | トークン一致テスト・style-audit | クロームのアクセントを絞る |
| 絵文字アイコン | `terminology.test.ts` | 0 件（16px・1.5 stroke のインライン SVG） |
| 過剰な影・グロー | `box-shadow` / `blur` 走査 | 現行 Hyperstudio では影 0 件 |
| 任意値ユーティリティ・角丸乱用 | style-audit・トークン検査 | 4px グリッド、規定トークン外を使わない |
| 小文字・コントラスト不足 | `tokens.test.ts`（`contrastRatio`） | 11px 未満不可、本文 4.5:1・操作輪郭 3:1 以上 |
| 用語不統一・誇張コピー | `terminology.test.ts`・レビュー | 正本用語集に照らし旧語 0 件、未検証の断定なし |

### 目視レビュー観点

- **テンプレ置換チェック**: 製品名を他社に置き換えても成立するなら方向が弱い。
- 見せ場（Moments）と署名（Signature）が正本の範囲に絞られ、クロームが静かか。
- 8 状態（default / hover / focus-visible / active / disabled / loading / error / empty）と数値の桁揃えがあるか。
- モーションが意味を説明しているか（全プロパティ遷移・不要ループを避ける）。
- 日本語が直訳調でなく、助詞抜けや用語揺れがないか。

## 3. 回避プロセス

1. **方向 5 行（heavy のみ）**: Product / Character（`clean`/`modern` 等を避け具体語 2〜3 語）/ World / Moments（1〜2 か所）/ Signature（全体で 1 つ）を正本に固定する。
2. **9 パス（light は該当のみ）**: ①余白 → ②色 → ③アイコン → ④コピー → ⑤タイポ → ⑥レイアウト → ⑦細部 → ⑧ビジュアル → ⑨モーション。
3. **確認**: 900×620 / 1024×768 / 1280×800 / 1920×1080（幅 1180px 切替点）と 200% ズームで実起動スクリーンショットを確認する。

## 4. 参考・帰属

- [unslop（作者 Damir Sokolovsky、CC BY 4.0）](https://github.com/heydamir/unslop)（原典: https://heydamir.design/unslop ）— 本書の方向 5 行・9 パス・トリアージの翻案元。由来部分は [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) に従う。
- [vibecodekit: AI Slop Design](https://vibecodekit.dev/ai-slop-design) / [design-agent.dev](https://design-agent.dev/blog/2026-10-15-ai-slop-measuring-design-homogenization/) / [AI-Slop Checklist](https://standoutmcp.io/guides/fix-vibe-coded-ui-checklist) / [no-slop-ui](https://github.com/LeoStehlik/no-slop-ui) / [arXiv 2501.19361](https://arxiv.org/abs/2501.19361)
