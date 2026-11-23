# AI スロップ（AI slop）を避ける方法（調査と運用ルール）

- 作成: 2026-11-23
- 目的: 「量産的」「AI が作った感じ」を**主観で争わず**、判断と検証の手順に落とす
- 位置づけ: 本文書は**推奨（非規範）**。レビュー checklist・Tier 判定の正本は [docs/REVIEW_TIERS.md](REVIEW_TIERS.md) です。本文書はレビューの義務や差し戻し条件を課しません（§4 の 3 項）。以下の「推奨基準」は判断の目安で、満たさない画面を自動的に不合格にはしません。
- 適用対象: 本リポジトリのデスクトップ GUI。**main の GUI は egui 版（`crates/eightbitdo-gui`）が正本**で、**Tauri v2 版は `feat/tauri-gui` ブランチで再構築中**です。**Tauri 版がマージされた時点で、本文書の Tauri 記述が有効**になります。モバイル（Kotlin / Swift）はネイティブ UI のため、§2.1 の CSS / DOM 前提の検出手段はそのままは使えません（色定数・フォントサイズ定数の静的検査と目視で代替します）。
- main との関係: main の `AGENTS.md` §3・`README.md` の GUI 節・`crates/eightbitdo-gui/README.md`・`docs/IMPLEMENTATION_PLAN.md` は **egui 前提**（「Tauri を使わない」と明記）です。Tauri v2 の許可・GUI 再構築・表示名 **Padbench** への改名（予定）は `feat/tauri-gui` ブランチで行い、**本 PR は main の技術スタック記述を変更しません**。
- 正本の関係: 方向 5 行・見せ場（Moments）・署名・トークン・用語集・8 状態の正本は、**マージ後の `feat/tauri-gui` の `docs/DESIGN_TAURI.md`**（§4 / §6.3 / §8.2）です。本文書は方法論だけを扱い、その内容を要約・再定義しません（§5）。egui 版（main）は Tauri 版の方向・用語集の対象外です。
- `feat/tauri-gui` は未マージのため、本文書ではパス名の言及にとどめ、リンクしません（マージ後に参照してください）。

## 適用のトリアージ（どこまでやるか）

unslop の判断に倣い、適用量を 3 段階で決めます。

- **heavy（兆候が多い）**: 方向 5 行を決め、9 パスを順に通し、正本の検証サイズと 200% ズームで確認します。描画して確認するのは**最大 3 回**です。
- **light（既に概ね設計済み）**: 磨き（polish）。監査で見つかった兆候だけを直し、方向は既存のブランドから取ります。色・タイポ・レイアウトの大きな変更や新しい見せ場は、勝手に入れず**提案に留めます**。
- **no-slop（大きな兆候が無い）**: 問題ないと明言し、小さな改善を**2〜3 件だけ**、具体的な値付きで提案します。適用は同意がある場合だけにし、それ以上は変えません。
- どの段階でも、正本の決定を勝手に変えません。変える場合は正本（`docs/DESIGN_TAURI.md`）も同時に更新します。

---

## 0. 結論（推奨の要点）

1. **AI スロップの原因は「判断の不在」**。モデルは指示が無いと学習分布の平均（無難な白＋青、角丸カード、Inter 風見出し）へ回帰する。能力の問題ではなく、**決めていないこと**が原因。
2. 対策は「非推奨リスト」だけでは足りない。**方向を 1 回決めて（5 行）、全パスをその方向に従わせ、最後にテンプレ置換チェックで検証する**。
3. 良し悪しは属人的なので、**機械で検出できる兆候**（グラデ・絵文字・アクセント数・余白スケール・コントラスト・禁止語）を検査にし、残りをレビュー観点（推奨）にする。
4. 見せ場（光る一点）は**正本の Moments の範囲にだけ**置く。署名（この製品だけの細部）は**アプリ全体で 1 つ**に絞り、画面ごとに必須とはしない（unslop は、属する場所にだけ現れ、それ以外では現れないとする）。
5. 日本語 UI は別軸の AI スロップ（直訳・用語の不統一）が出る。用語集（正本）に従い、日本語の推敲を独立に通す。

---

## 1. なぜ AI 生成 UI は量産的に見えるのか

外部の分析でも原因は一致している。

- **統計的平均への回帰**: 指示が無いとモデルは「よくある見た目」の確率質量へ寄る。白いカード＋グレー背景＋単一の青、角丸 8〜12px、Inter 風フォント、紫→青グラデがその代表。[vibecodekit](https://vibecodekit.dev/ai-slop-design) は、AI スロップの本当の原因は判断の不在だと述べている。
- **自己評価が苦手**: 「良い UI か」を自分で採点させても、分布が縮んでもスコアが動かない。生成的均質性はモデルの能力向上だけでは解けない。[design-agent.dev の分析](https://design-agent.dev/blog/2026-10-15-ai-slop-measuring-design-homogenization/) はこの構造を指摘している。
- **装飾がクロームに漏れる**: 色・光・影・絵文字が、内容ではなくナビ・ボタン・枠・ラベルに散る。結果、どの製品にも見える。[unslop](https://github.com/heydamir/unslop)（作者 Damir Sokolovsky、[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/)）は、richness は 1〜2 の内容の瞬間に置き、クロームは静かにする、と定式化している。本文書は同記事の考え方を**翻案して作成**した（原典: https://heydamir.design/unslop 。§6）。
- **検証が主観**: 「なんか安っぽい」で終わると直らない。兆候を列挙し、修正し、再描画して突き合わせる手順が必要。[AI-Slop Checklist](https://standoutmcp.io/guides/fix-vibe-coded-ui-checklist)、[no-slop-ui](https://github.com/LeoStehlik/no-slop-ui)。

---

## 2. 検出チェックリスト（兆候と検出手段）

以下は**機械検出できるもの**と**目視で見るもの**に分ける。検出手段は、Tauri v2 / Web UI（`feat/tauri-gui` で追加中）と main の egui 版の両方で書く。**推奨基準**は目安であり、判定を機械的に強制しない。

### 2.1 機械で検出できる兆候（推奨）

| 兆候 | 検出方法（Tauri v2 / Web UI。`feat/tauri-gui` で追加中） | 検出方法（egui 版・main） | 推奨基準 |
| --- | --- | --- | --- |
| 色相グラデーション（面・文字・光彩） | `gradient` の grep、`tokens.ts` と `tokens.css` の変数一致テスト | 【grep/目視】`Painter` のグラデ呼び出しと多段色定数の grep | 0 件（同一色相内の微小な明度差のみ許容） |
| アクセント色の乱立 | 色トークンの列挙と CSS 変数の一致テスト | 【grep】色定数（`pub const ... Color32`）の列挙（トークン単一性テストは無い） | クロームのアクセントは 1 色のみ |
| 絵文字をアイコン代わりに使用 | 絵文字コードポイントのテスト（`terminology.test.ts`） | 【grep】`i18n.rs` と画面コードの絵文字スキャン | 0 件（アイコンは 16px または 18px・1.5 stroke の線画。egui 版は SVG を使わず同梱のアイコンフォント） |
| 紫・ネオンの使用 | 色相の検査 | 【grep/目視】色定数の色相検査 | 使用しない |
| 過剰な影・グロー・カラーブラー | `box-shadow` / `filter: blur` の検査 | 【grep】`Shadow` / `Stroke` の使用箇所 | 影は `--shadow-pop` の 1 種のみ（ポップオーバー・ダイアログ・トースト） |
| 4px グリッド外の余白 | 任意値（`[...px]`）の grep | 【grep】`SPACE_*` と直書きの余白値 | 原則 4px 倍数（2px の微調整・中央寄せの奇数値は許容） |
| 角丸の量産（`rounded-full` の多用） | クラス集計と `RADIUS` トークン外の値の grep | 【テスト】`geometry_fits_the_minimum_window_with_one_content_column` が `RADIUS_*` の 4 種を検査 | 角丸は限られた段（4 種程度）に限定し、ピル化しない |
| 11px 未満の文字 | サイズトークン検査 | 【grep】`FONT_*` 定数と 11px 未満の検出 | 11px 未満を使わない（データ補助を除く） |
| コントラスト不足 | 実測計算（`contrastRatio`） | 【テスト】`theme.rs` の `contrast_ratio` テスト | AA: 本文 4.5:1、大文字 3:1、操作輪郭 3:1 |
| 用語の不統一・禁止語 | 用語集テスト（`VOCAB` / `terminology.test.ts`） | 【grep/目視】`i18n.rs` の文字列を**対象 UI の用語集**と突き合わせ（egui 用の用語集テストは無い） | 対象 UI の用語集に照らして旧語 0 件 |
| 未検証の主張 | コピーのレビュー + grep | 【レビュー/grep】コピーのレビュー + grep | 「完全」「絶対」「10x」等の断定なし |

> **Tauri 側は導入予定**: main には Tauri 版のコードもテストもありません。`feat/tauri-gui` で `apps/desktop` に `tokens.test.ts` / `terminology.test.ts` 等を追加中で、実装後は `apps/desktop/src/**/*.test.ts(x)` の同じテスト群へ画面追加分を足します。`apps/desktop` のテストは root の CI では走りません（手動 `pnpm test`）。

> **egui 版（main）のテスト実体**: `theme.rs` 8 件・`i18n.rs` 9 件。既存テストで担保されるのは文字スケール・コントラスト・幾何（角丸 4 種）・フォント同梱で、4px 倍数・トークン単一性・禁止語突き合わせのテストはありません（表で【grep/目視】とした項目）。egui 版は Tauri 版のマージ後に凍結予定です。

> **用語**: egui 版の現行語（`i18n.rs` の「ライブ入力」「設定値」等）は、Tauri 版の用語集を遡って適用しません。推奨基準は常に**対象 UI の用語集**に対して判定します。

モバイル（Kotlin / Swift）のネイティブ UI は CSS・DOM 検査ができないため、同じ兆候を色定数・フォントサイズ定数の静的検査、絵文字スキャン、コントラスト計算で確認します（検査基盤は実装時に用意する。冒頭の「適用対象」のとおり、本文書が扱うのはデスクトップ GUI）。

### 2.2 目視・レビューで見る（推奨）

- **テンプレ置換チェック**: 製品名を他社製品に置き換えても成立する画面は、まだ方向が弱い（unslop の最終チェック）。
- **見せ場（光る一点）**が正本の Moments の範囲に収まっているか。
- **署名**がアプリ全体で 1 つに絞られ、必要な場所にだけ現れているか（画面ごとに足さない）。
- クローム（ナビ・ボタン・枠）が静かか。内容より目立っていないか。
- **8 状態**（default / hover / focus / active / disabled / loading / error / empty）が設計されているか（出典: Tauri 版の `docs/DESIGN_TAURI.md` §8.2。egui 版は対象外）。
- 数値の桁揃え（Tauri 版は `tabular-nums`、egui 版は等幅フォント）と書式の統一。
- 見出しが「何の画面か」を言っているか（`ようこそ！` 等の挨拶は避ける）。
- モーションが意味を説明しているか（ループ・跳ね・全プロパティ遷移は避ける）。
- 日本語: 直訳調でないか、助詞が抜けていないか、用語が統一されているか。

---

## 3. 回避のプロセス（順序が重要）

### 3.1 方向を 5 行で決める（heavy のみ）

1. **Product**: 何を、誰のために。
2. **Character**: 具体的な 2〜3 語。「clean」「modern」「sleek」「minimal」「premium」は避ける（どのテンプレにも当てはまるため）。
3. **World**: その製品が自然に属する場所・素材。
4. **Moments**: 色・材料・光を集中させる 1〜2 か所（内容側）。
5. **Signature**: この製品だけの小さな細部 1 つ。

方向が決まるまで実装しない。決めた方向は**正本**（Tauri 版の `docs/DESIGN_TAURI.md` §4）に固定し、変更には理由を書く。**同じ 5 行を複数の文書に書かない**（正本を 1 つに保つ）。ブランド（ロゴ・色・書体）が既にある場合は、それを活かして方向を組み立てる。

### 3.2 9 パスで直す（light は該当パスだけ）

1. 余白（4px グリッド、同種同サイズ、グルーピング）
2. 色（1 つのニュートラルランプ、3 つの文字役割、1 アクセント）
3. アイコン（1 セット・1 サイズ・1 ストローク、絵文字排除）
4. コピー（具体的・短い・誇張なし・未検証の主張なし）
5. タイポ（キャラクタに合う 1〜2 ファミリ、400〜600、階層はサイズと色で）
6. レイアウト（アプリの比率、中央寄せ縦積みの回避、非対称の活用）
7. 細部（入れ子角丸、状態、フォーカスリング、数値書式）
8. ビジュアル（製品自身の内容で見せる。装飾の詰め物をしない）
9. モーション（意味のある動きのみ、1 イージング、ループ禁止）

### 3.3 確認（推奨）

- heavy は正本の検証サイズ（900×620 / 1024×768 / 1280×800 / 1920×1080、幅 1180 px の切替点）と **200% ズーム**で**実際に起動してスクリーンショット**を撮る（描画は最大 3 回）。
- 兆候リストと突き合わせ、残っていれば直す。
- 独立レビューは [docs/REVIEW_TIERS.md](REVIEW_TIERS.md) の Tier に従う（本文書はレビューの義務を課さない）。

---

## 4. このリポジトリでの推奨運用

1. **画面追加・変更時（推奨）**: 方向のどこに属するか、見せ場、署名の当てはまりを PR 本文に書く（**義務ではない**）。
2. **検査の追加（導入予定）**: 画面ごとに、検出できる兆候のいずれかを自動テストに含める（main では未実装。`feat/tauri-gui` で追加中）。
3. **レビュー**: [docs/REVIEW_TIERS.md](REVIEW_TIERS.md) が正本。本文書は推奨で、レビューの義務や差し戻し条件を課さない。REVIEW_TIERS に本文書への参照を足す場合は別 PR で行う。
4. **日本語**: 用語集（正本: Tauri 版の `docs/DESIGN_TAURI.md` §6.3）に従い、推敲は別途行う。`nihongo` は選択肢の 1 つ（スキル名）であり、規範根拠にはしない。
5. **非推奨リスト（推奨）**: 色相グラデーション、紫・ネオン、絵文字アイコン、ガラスモーフィズム、過剰な角丸カード、意味のないバッジ（`PRO` `New`）、挨拶見出し、`Welcome back!`、`seamless` 等の誇張、未検証の数値主張、全プロパティ遷移、ループアニメーション。
6. **本文書の変更**: レビュー観点に触れるため **Tier 3**（[docs/REVIEW_TIERS.md](REVIEW_TIERS.md) §2）。外部資料の引用は帰属を確認する。
7. **正本を 1 つに**: 方向・値・用語を変えるときは、先に正本（Tauri 版の `docs/DESIGN_TAURI.md`）を更新する。

---

## 5. GUI への適用（正本を参照）

main の GUI は **egui 版が正本**です。**Tauri v2 版は `feat/tauri-gui` ブランチで再構築中**で、**マージ後に本文書の Tauri 記述が有効**になります。表示名 **Padbench** への改名も同ブランチで予定されており、main には反映されていません。

方向 5 行・見せ場・署名・不採用案・トークン・用語集・8 状態は `docs/DESIGN_TAURI.md`（§4 / §6.3 / §8.2）を参照し、**本文書では要約しない**（正本の二重化を避ける）。egui 版（main）は Tauri 版の方向・用語集の対象外です。main の egui 前提の記述の更新も同ブランチで行います。

---

## 6. 参考

- [unslop（作者 Damir Sokolovsky、CC BY 4.0）](https://github.com/heydamir/unslop) — 方向 5 行・9 パス・トリアージ・チェックリスト。本文書の骨格。原典は https://heydamir.design/unslop 、ライセンスは [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/)（GitHub の skill リポジトリ `heydamir/unslop` の LICENSE）。本文書のうち原典を**翻案して作成**した部分も CC BY 4.0 で提供し、作者名・ライセンス・出典を表示する。
- [AI Slop Design: Why AI-Generated UI Looks Generic](https://vibecodekit.dev/ai-slop-design) — 原因は判断の不在。
- [AI Slop and UI Homogenization: Measuring Design Convergence](https://design-agent.dev/blog/2026-10-15-ai-slop-measuring-design-homogenization/) — 生成的均質性の測定。
- [How to Fix a Vibe-Coded UI: The AI-Slop Checklist](https://standoutmcp.io/guides/fix-vibe-coded-ui-checklist) — 兆候別チェックリスト。
- [no-slop-ui](https://github.com/LeoStehlik/no-slop-ui) — エージェント向けガードレールとレビュー面。
- [Anti-Slop UI: A Deterministic State-Machine Architecture](https://www.researchgate.net/publication/411021905_Anti-Slop_UI_A_Deterministic_State-Machine_Architecture_for_Eliminating_Design_Hallucinations_in_LLM-Generated_Interfaces) — 制約による状態機械的アプローチ。
- [We're Different, We're the Same (arXiv 2501.19361)](https://arxiv.org/abs/2501.19361) — LLM 出力の同質性。
