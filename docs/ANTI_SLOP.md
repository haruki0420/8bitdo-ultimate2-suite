# AI スロップ（AI slop）を避ける方法（調査と運用ルール）

- 作成: 2026-11-23
- 目的: 「量産的」「AI が作った感じ」を**主観で争わず**、判断と検証の手順に落とす
- 位置づけ: 本ドキュメントは**方法論の推奨（非規範）**。レビュー checklist・Tier 判定の正本は [docs/REVIEW_TIERS.md](REVIEW_TIERS.md) です。本文書は差し戻し条件を定義しません（§4 の 3 項）。
- 適用対象: 本リポジトリのデスクトップ GUI（**Tauri v2 / Web UI で再構築中（決定済み）**）と、今後追加する画面。モバイル（Kotlin / Swift）はネイティブ UI のため、§2.1 の CSS / DOM 前提の検出手段はそのままは使えません（色定数・フォントサイズ定数の静的検査と目視で代替します）。
- GUI の現状: **GUI は Tauri v2 で再構築中（決定済み）。egui 版は凍結保存。** Tauri 版の計画・デザイン決定・実装は `feat/tauri-gui` ブランチ（`docs/TAURI_PLAN.md` / `docs/DESIGN_TAURI.md` / `docs/design/mockup.html` / `apps/desktop`）で進行中で、**main にはまだありません**。したがって本文書ではこれらをリンクせず、パス名の言及にとどめます（マージ後に参照してください）。GUI の技術スタックは Tauri v2 が決定事項であり、「egui を明示的に不使用」という意味ではありません。
- 正本の関係: 色・タイポ・レイアウト・方向（Direction 5 行）・用語集の正本は `docs/DESIGN_TAURI.md`（`feat/tauri-gui`）。本文書は方法論のみを扱い、具体値や方向を再定義しません（§5）。

---

## 0. 結論（先に要点）

1. **AI スロップの原因は「判断の不在」**。モデルは指示が無いと学習分布の平均（無難な白＋青、角丸カード、Inter 風見出し）へ回帰する。能力の問題ではなく、**決めていないこと**が原因。
2. 対策は「禁止リスト」だけでは足りない。**方向を 1 回決めて（5 行）、全パスをその方向に従わせ、最後にテンプレ置換テストで検証する**。
3. 良し悪しは属人的なので、**機械で検出できる兆候**（グラデ・絵文字・アクセント数・余白スケール・コントラスト・禁止語）をテストにし、残りをレビュー観点にする。
4. 見せ場（光る一点）は**正本の Moments にだけ**置き、**明るい面には光を足さない**。署名は**アプリ全体で 1 つ**に絞る（画面ごとに必須ではない。unslop の「属する場所にだけ現れ、それ以外では現れない」）。
5. 日本語 UI は別軸の AI スロップ（直訳・用語の不統一）が出る。**用語集と `nihongo` 基準**を独立に通す。

---

## 1. なぜ AI 生成 UI は量産的に見えるのか

外部の分析でも原因は一致している。

- **統計的平均への回帰**: 指示が無いとモデルは「よくある見た目」の確率質量へ寄る。白いカード＋グレー背景＋単一の青、角丸 8〜12px、Inter 風フォント、紫→青グラデがその代表。[vibecodekit](https://vibecodekit.dev/ai-slop-design) は「AI スロップの本当の原因は判断の不在」と表現している。
- **自己評価が苦手**: 「良い UI か」を自分で採点させても、分布が縮んでもスコアが動かない。生成的均質性はモデルの能力向上だけでは解けない。[design-agent.dev の分析](https://design-agent.dev/blog/2026-10-15-ai-slop-measuring-design-homogenization/) はこの構造を指摘している。
- **装飾がクロームに漏れる**: 色・光・影・絵文字が、内容ではなくナビ・ボタン・枠・ラベルに散る。結果、どの製品にも見える。[unslop](https://github.com/heydamir/unslop)（作者 Damir Sokolovsky、CC BY 4.0）は「richness は 1〜2 の内容の瞬間に置き、クロームは静かにする」と定式化している。本文書は同記事の考え方を**翻案して作成**した（原典: https://heydamir.design/unslop 、ライセンス: CC BY 4.0。§6）。
- **検証が主観**: 「なんか安っぽい」で終わると直らない。兆候を列挙し、修正し、再描画して突き合わせる手順が必要。[AI-Slop Checklist](https://standoutmcp.io/guides/fix-vibe-coded-ui-checklist)、[no-slop-ui](https://github.com/LeoStehlik/no-slop-ui)。

---

## 2. 検出チェックリスト（兆候 → 修正）

以下は**機械検出できるもの**と**目視・レビューで見るもの**に分ける。検出手段は、Tauri v2 / Web UI（現在の実装方針）と、凍結保存した egui 版の両方で書く。

### 2.1 機械で検出する（テスト / grep / トークン検査）

| 兆候 | 検出方法（Tauri v2 / Web UI） | 検出方法（egui 版・凍結コード） | 合格条件 |
| --- | --- | --- | --- |
| 色相グラデーション（面・文字・光彩） | `gradient` の grep、`tokens.ts` と `tokens.css` の変数検査 | `Painter` のグラデ呼び出しと多段色定数の grep（`theme.rs`） | 0 件（同一色相内の微小な明度差のみ許可） |
| アクセント色の乱立 | 色トークンの列挙（`COLORS` / `ACCENT`）と CSS 変数の一致テスト | 色定数（`pub const ... Color32`）の列挙と、テーマトークン単一性テスト（`theme.rs` 以外に色を作らない） | クロームのアクセントは 1 色のみ |
| 絵文字をアイコン代わりに使用 | 絵文字コードポイントの検査（`terminology.test.ts`） | ソース文字列の絵文字スキャン（`i18n.rs` と画面コード） | 0 件（16px・1.5 stroke の線画 SVG のみ） |
| 紫・ネオンの使用 | 色相の検査 | 色定数の色相検査 | 使用しない |
| 過剰な影・グロー・カラーブラー | `box-shadow` / `filter: blur` の検査 | `Shadow` / `Stroke` の使用箇所の検査 | 影は `--shadow-pop` の 1 種のみ（ポップオーバー・ダイアログ・トースト） |
| 4px グリッド外の余白 | 任意値（`[...px]`）の grep | `SPACE_*` の 4px 倍数テストと直書きの grep | 0 件 |
| 角丸の量産（`rounded-full` の多用） | クラス集計（**Tauri 版では**）と `RADIUS` トークン外の値の grep | `RADIUS_*` の種類数テスト（4 種まで） | スイッチ等の形状に限定 |
| 12px 未満の文字 | サイズトークン検査 | フォントサイズ定数（`FONT_*`）の grep と 11px 未満の検出 | 最小 11px（データ補助のみ） |
| コントラスト不足 | 実測計算（`contrastRatio`） | 実測計算（`theme.rs` の `contrast_ratio` テスト） | AA: 本文 4.5:1、大文字 3:1、操作輪郭 3:1 |
| 用語の不統一・禁止語 | 用語集テスト（`VOCAB` / `terminology.test.ts`） | 用語集テスト（`i18n.rs` の文字列と用語集の突き合わせ） | 録音/ライブ入力/設定値 等の旧語 0 件 |
| 未検証の主張 | コピーのレビュー + grep | コピーのレビュー + grep | 「完全」「絶対」「10x」等の断定なし |

> **導入予定**: Tauri 版の実装時に、`apps/desktop/src/**/*.test.ts` へ、グラデ禁止・絵文字禁止・単一アクセント・コントラスト・用語集の同じテスト群を追加する（現在は未実装）。`apps/desktop` のテストは root の CI では走らない（フロントエンドは手動で `pnpm test`）。**画面を追加したら同じテスト群に項目を足す**。egui 版は凍結済みのため、既存の `cargo test -p eightbitdo-gui` を回帰確認に使う。

モバイル（Kotlin / Swift）のネイティブ UI は CSS・DOM 検査ができないため、同じ兆候を**色定数・フォントサイズ定数の静的検査、絵文字スキャン、コントラストの計算、テーマトークンの単一性テスト**で検出する（検査基盤はモバイル実装時に用意する。§5 の対象外）。

### 2.2 目視・レビューで見る（機械化が難しい）

- **テンプレ置換テスト**: 製品名を他社製品に置き換えても成立する画面は、まだ方向が弱い（unslop の最終チェック）。
- **見せ場（光る一点）**が**正本の Moments にだけ**置かれているか。明るい面に光を足していないか（Moments 以外では光らせない）。
- **署名**が**アプリ全体で 1 つ**だけ、必要な場所にだけ現れているか（画面ごとに足さない。2 つ以上は散らかる）。
- クローム（ナビ・ボタン・枠）が静かか。内容より目立っていないか。
- **8 状態**（default / hover / focus / active / disabled / loading / error / empty）が設計されているか。
- 数値の桁揃え（Tauri 版は `tabular-nums`、egui 版は等幅フォント）と書式の統一。
- 見出しが「何の画面か」を言っているか（`ようこそ！` 等の挨拶は禁止）。
- モーションが意味を説明しているか（ループ・跳ね・`transition-all`（Tauri 版）は禁止）。
- 日本語: 直訳調でないか、助詞が抜けていないか、用語が統一されているか。

---

## 3. 回避のプロセス（順序が重要）

### 3.1 方向を 5 行で決める（実装前に）

1. **Product**: 何を、誰のために。
2. **Character**: 具体的な 2〜3 語（「静か」「正確」「必要な所だけ灯る」）。「モダン」「ミニマル」「プレミアム」は禁止（どのテンプレにも当てはまるため）。
3. **World**: その製品が自然に属する場所・素材（昼の作業台と暗いステージなど）。
4. **Moments**: 色・材料・光を集中させる 1〜2 か所（内容側）。
5. **Signature**: この製品だけの小さな細部 1 つ。

方向が決まるまで実装しない。決めた方向は**正本（本リポジトリでは `docs/DESIGN_TAURI.md` §4）**に固定し、変更には理由を書く。**同じ 5 行を複数の文書に書かない**（正本を 1 つに保つ）。

### 3.2 9 パスで直す（unslop の手順）

1. 余白（4px グリッド、同種同サイズ、グルーピング）
2. 色（1 つのニュートラルランプ、3 つの文字役割、1 アクセント）
3. アイコン（1 セット・1 サイズ・1 ストローク、絵文字排除）
4. コピー（具体的・短い・誇張なし・未検証の主張なし）
5. タイポ（キャラクタに合う 1〜2 ファミリ、400〜600、階層はサイズと色で）
6. レイアウト（アプリの比率、中央寄せ縦積みの回避、非対称の活用）
7. 細部（入れ子角丸、状態、フォーカスリング、数値書式）
8. ビジュアル（製品自身の内容で見せる。装飾の詰め物をしない）
9. モーション（意味のある動きのみ、1 イージング、ループ禁止）

### 3.3 検証してから完了

- 正本の検証サイズ（900×620 / 1024×768 / 1280×800 / 1920×1080、幅 1180 px の切替点）と **200% ズーム**で**実際に起動してスクリーンショット**を撮る。
- 兆候リストと突き合わせ、残っていれば直す。
- 独立レビュー（Tier は [docs/REVIEW_TIERS.md](REVIEW_TIERS.md) に従う）で「テンプレ置換テスト」と「見せ場・署名」を確認する。

---

## 4. このリポジトリでの運用ルール

1. **画面追加時**: 方向 5 行（正本 `docs/DESIGN_TAURI.md` §4 を引用）+ 見せ場（Moments のどれか）+ 署名の当てはまりを PR 本文に書く（**推奨**。欠けていればレビューで質問する）。
2. **テスト追加**: 画面ごとに、グラデ 0・絵文字 0・アクセント 1・コントラスト AA・用語集一致・4px グリッドのいずれかを自動テストに含める（**導入予定**。Tauri 版の実装時に `apps/desktop` のテスト群へ追加する）。
3. **レビュー**: [docs/REVIEW_TIERS.md](REVIEW_TIERS.md) の checklist に従い、兆候リストとテンプレ置換テストを確認する。**本文書は推奨であり、差し戻し条件は REVIEW_TIERS が正本**。
4. **日本語**: `nihongo` 基準と GUI の用語集（Tauri 版の用語集は `docs/DESIGN_TAURI.md` §6.3 — `feat/tauri-gui` ブランチで進行中、マージ後に参照）を独立に通す。機械テストは禁止語のみ。
5. **禁止リスト（この製品で使わない）**: 色相グラデーション、紫・ネオン、絵文字アイコン、ガラスモーフィズム、過剰な角丸カード、意味のないバッジ（`PRO` `New`）、挨拶見出し、`Welcome back!`、`seamless` 等の誇張、未検証の数値主張、`transition-all`、ループアニメーション。
6. **本文書の変更**: レビュー観点・コントリビューター手順に触れるため **Tier 3**（[docs/REVIEW_TIERS.md](REVIEW_TIERS.md) §2）。外部資料の引用（unslop は CC BY 4.0）は帰属を確認する。

---

## 5. 本リポジトリの GUI への適用（正本を参照）

本リポジトリのデスクトップ GUI は **Tauri v2 で再構築中（決定済み）**。表示名 **Padbench**（2026-11-23 決定）と実装は `feat/tauri-gui` ブランチで進行中で、**main にはまだ無い**（main の GUI は egui 版で、凍結保存）。方向（Direction 5 行）・トークン・コンポーネント・用語集の決定記録は `docs/DESIGN_TAURI.md` が正本。

- **方向 5 行**: 正本 `docs/DESIGN_TAURI.md` §4 をそのまま参照する（Product / Character / World / Moments / Signature）。**本文書では再定義しない**（二重定義を避ける）。
- **見せ場**: 正本の Moments だけに置く（入力モニターの暗いステージ、書き込み前の確認）。明るい作業面には光を足さない。
- **署名**: 正本の Signature（面の二重性 = 読む・書くは明るい面、見るは暗い面）を**アプリ全体で 1 つ**使う。
- **不採用案を持ち込まない**: 正本で不採用の案（A: 影も光も使わない校正台そのもの、B: 刻印・紙のテクスチャ・影）を再導入しない。採用は C（昼の作業台と夜のステージ）で、A の規律（罫線・余白・装飾の削除）と B の等幅・桁揃えだけを内部作法として継承する。

方向を変える場合は、`docs/DESIGN_TAURI.md`（と `docs/TAURI_PLAN.md`）の決定記録も同時に更新し、正本を 1 つに保つ。

---

## 6. 参考

- [unslop（作者 Damir Sokolovsky、CC BY 4.0）](https://github.com/heydamir/unslop) — 方向 5 行・9 パス・チェックリスト。本ドキュメントの骨格。原典は https://heydamir.design/unslop 。本文書は原典の考え方を**翻案して作成**した（CC BY 4.0 の条件に従い、作者名・ライセンス・出典を表示）。
- [AI Slop Design: Why AI-Generated UI Looks Generic](https://vibecodekit.dev/ai-slop-design) — 原因は判断の不在。
- [AI Slop and UI Homogenization: Measuring Design Convergence](https://design-agent.dev/blog/2026-10-15-ai-slop-measuring-design-homogenization/) — 生成的均質性の測定。
- [How to Fix a Vibe-Coded UI: The AI-Slop Checklist](https://standoutmcp.io/guides/fix-vibe-coded-ui-checklist) — 兆候別チェックリスト。
- [no-slop-ui](https://github.com/LeoStehlik/no-slop-ui) — エージェント向けガードレールとレビュー面。
- [Anti-Slop UI: A Deterministic State-Machine Architecture](https://www.researchgate.net/publication/411021905_Anti-Slop_UI_A_Deterministic_State-Machine_Architecture_for_Eliminating_Design_Hallucinations_in_LLM-Generated_Interfaces) — 制約による状態機械的アプローチ。
- [We're Different, We're the Same (arXiv 2501.19361)](https://arxiv.org/abs/2501.19361) — LLM 出力の同質性。
