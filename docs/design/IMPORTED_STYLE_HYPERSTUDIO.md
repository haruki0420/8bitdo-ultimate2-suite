# Imported style: Hyperstudio

Padbench Tauri デスクトップ UI に適用したスタイルの仕様です。**色・タイポグラフィ・コンポーネントの正本**はこの文書で、実装上の値は `apps/desktop/src/styles/tokens.css` と `apps/desktop/src/index.css` の `@theme` にあります。`docs/DESIGN_TAURI.md` §5（案 C「昼の作業台と夜のステージ」）のライト面トークンは、このスタイルで置換済みです（§12 参照）。

- 出典: [Hyperstudio — Refero Styles](https://styles.refero.design/style/8eb9c53e-d69c-497a-b640-610856cf3a60)
- 適用日: 2026-11-23
- 適用範囲: `apps/desktop`（Tauri v2 + React + Tailwind CSS v4）

> Refero / Hyperstudio はデザインの**参考**であり、Padbench は両者と提携・推奨・後援の関係にありません。名称・ロゴ・固有のアートワークは使用していません。トークン名と数値だけを、出典を明記して参考にしています。

---

## 1. コンセプト

**「黒曜石に刻んだ青写真」。**

- near-black のキャンバス（`obsidian`）に、1px のヘアライン（`graphite`）だけで構造を描く
- 文字が主役。weight 400 の大きな見出しと、強い負のトラッキングで階層を作る
- 主アクションは **白 1 色のピル**だけ。ほかに色付きの塗りを持たない
- アイコンの線だけが `compass-gold`、ライブ状態のドットと入力モニターの軌跡線だけが `pulse-green`
- 影・グラデーション・写真・イラストを使わない。浮きは境界線とコントラストだけで表す

Padbench では「静か、正確、必要な所だけ灯る」という既存の方向性を保ちつつ、面の二重性（明るい面 / 暗いステージ）を廃止し、**アプリ全体を 1 つの暗色キャンバス**に統一しました。

---

## 2. 色トークン

CSS 変数名はそのまま `tokens.css` にあります。Tailwind では `bg-obsidian`、`text-chalk`、`border-graphite` のように使います。

| token | hex | 用途 |
| --- | --- | --- |
| `obsidian` | `#101010` | ページ canvas。アプリ全体の地 |
| `carbon` | `#080808` | 最深部。ダイアログ・トースト・入力モニターのステージ |
| `graphite` | `#212121` | 1px の境界線・カード・区切り。構造はこれ 1 色で描く |
| `card-slate` | `#3b3d45` | カード / パネルの境界アクセント（予約） |
| `iron` | `#474747` | 二次境界・ストローク。スティック円、トリガー枠 |
| `smoke` | `#9c9c9c` | 副次テキスト・キャプション・本文 |
| `ash` | `#c1c1c1` | 中間境界・三次テキスト。表の値など smoke より一段強い文字 |
| `chalk` | `#f3f3f3` | 主テキスト・見出し・本文の強調 |
| `signal-white` | `#ffffff` | **唯一の主アクション** = 白ピルの塗り / 反転文字 / ゴーストの 1px 枠 |
| `compass-gold` | `#6f6759` | **アイコンのストローク専用**。文字・背景には使わない |
| `pulse-green` | `#98ff38` | ライブ / 接続中のステータスドット（6px 円）と、入力モニターの軌跡線（1px・不透明度のみ変化）専用 |

派生 2 色（出典の 11 色には含まれないが、部品仕様のために必要）:

| token | hex | 用途 |
| --- | --- | --- |
| `paper-ink` | `#1a1a1a` | ステータスピルの地、表の選択行 |
| `danger` | `#b42318` | 危険ボタンの 1px 枠。**文字には使わない**（後述のコントラスト理由） |

色のルール:

1. 色付きの塗りは **`signal-white` の白ピル**と **`pulse-green` の 6px ドット**だけ。色付きの線は `pulse-green` の軌跡線だけ。ほかは無彩色（`paper-ink` / `graphite` / `carbon`）の面と罫線
2. `compass-gold` はアイコンの線だけ。文字・背景・枠線には使わない
3. `pulse-green` はライブ / 接続中のドットと入力モニターの軌跡線だけ。注意・成功・エラーの色には使わない
4. `danger` は枠線だけ。文字は `chalk`（`danger` on `obsidian` は 2.89:1 で AA を満たさないため）
5. グラデーション・影・発光・写真は使わない

---

## 3. タイポグラフィ

### 3.1 書体

| 役割 | 書体 | 備考 |
| --- | --- | --- |
| UI・本文・見出し | **Inter**（Aeonik の代替） | 日本語は **Noto Sans JP** を Inter の後ろに置く |
| 数値・メタ・ステータス | **JetBrains Mono** / **IBM Plex Mono**（Input の代替） | 桁揃え。`font-variant-numeric: tabular-nums` |
| アイコン | インライン SVG 1.5px ストローク | 16 / 18 / 24px。絵文字は使わない |

`tokens.css` の `--font-ui` / `--font-data` は OS フォールバックを並べています。

> **TODO(fonts)**: Inter / JetBrains Mono / IBM Plex Mono の woff2 サブセットは未同梱です。`apps/desktop/public/fonts/` に置いたら `@font-face`（`font-display: swap`）と preload を追加し、スタックの先頭に実在する状態にしてください。現状は OS フォントで代替されます。

### 3.2 スケール

見出しも本文も **weight 400 固定**（500 / 600 / 700 は使いません）。階層はサイズとトラッキングだけで作ります。

| token | size / line-height | tracking | 用途 |
| --- | --- | --- | --- |
| `--text-hero` | 63 / 66 px（1.05） | -0.011em | 予約。アプリでは全面使用しない |
| `--text-display` | 44 / 46 px（1.05） | -0.007em | 画面タイトル（`h1`） |
| `--text-title` | 34 / 36 px（1.06） | -0.004em | 大きな見出し（将来のヒーロー帯） |
| `--text-heading` | 23 / 28 px（1.22） | 0 | 空状態の見出し・ダイアログの見出し |
| `--text-subheading` | 21 / 26 px（1.24） | 0 | セクション見出し（`h2` / `h3`） |
| `--text-lead` | 18 / 24 px（1.33） | 0 | 導入文（アプリでは未使用） |
| `--text-dialog` | 17 / 22 px（1.29） | 0 | ダイアログ・パネルの副見出し |
| `--text-body` | 16 / 22 px（1.38） | 0 | 本文（`smoke`） |
| `--text-ui` | 14 / 20 px（1.43） | 0 | ボタン・タブ・入力・表セル・ナビ |
| `--text-meta` | 13 / 18 px（1.38） | 0 | 補助・数値（mono） |
| `--text-micro` | 12 / 16 px（1.33） | +0.02em | ステータスピル・表ヘッダー（mono 大文字） |

- 負のトラッキングは 34px 以上だけ。**小さい文字に負のトラッキングを使わない**（12px のメタは +0.02em で僅かに開く）
- 大文字化はステータス / メタだけ。日本語には適用しない（`uppercase` が効かないので実害はない）
- 本文は 16px・`smoke`。アプリ UI では 14〜16 の範囲で調整してよい
- 行間は 0.95〜1.43。display は 1.05

---

## 4. 余白・形・レイアウト

### 4.1 余白（4px グリッド）

`4 / 8 / 12 / 16 / 20 / 24 / 40` px。カード内は 20〜24（アプリでは 20〜32 に調整）、セクション間は 40。

### 4.2 角丸

| token | 値 | 用途 |
| --- | --- | --- |
| `--radius-xs` | 4 px | タグ、ステータスピル、押下チップ |
| `--radius-md` | 8 px | カード、ダイアログ、トースト、入力、secondary / danger ボタン |
| `--radius-icon` | 99 px | アイコンだけのボタン（閉じる） |
| `--radius-full` | 9999 px | **白ピル（primary）だけ** |

カードを丸くしない（8px まで）。入れ子の角丸は外 − 内側の余白で計算し、4px を下回らせない。

### 4.3 影

**一切禁止。** ダイアログ・トーストも `carbon` + 1px `graphite` + 8px 角丸で浮きを表し、影を付けません。フォーカスも影ではなく `outline: 1px solid var(--chalk)`（offset 2px）で示します。

### 4.4 レイアウト

- 上部バー / 左ナビ / 右レールの比率は現行を維持（上部 52px、左 216px、右 344px）
- 背景は `obsidian` 一色。面の色替えをしない（`bg-panel` / `bg-sunken` のような塗り分けをしない）
- 境界はすべて 1px `graphite`。セクション区切りも 1px の水平線
- コンテンツ最大幅は 1200px（`--content-max: 75rem`）。アプリでは 1280×800 で実幅 720px 程度
- 1180px 未満で右レールを下部へ回す（現行どおり）。最小 900×620

---

## 5. コンポーネント

すべての部品に **default / hover / active / focus-visible / disabled / loading / error / empty** の状態を定義します。

### 5.1 Primary（白ピル）

- `signal-white` 地 + `obsidian` 文字、9999px、14px、padding 12×24、高さ 40px
- **1 画面に 1 つまで**
- hover は `chalk`、active は `ash` の地
- disabled は白ピルの輪郭を保ち、`signal-white / 10%` の地 + `graphite` 枠 + `smoke` 文字。直下に無効の理由を 1 行出す

### 5.2 Secondary（ゴースト）

- 透明地 + 1px `signal-white` 枠、8px 角丸、padding 10×20、`signal-white` 文字
- 白ピルにしない。hover は枠と文字を `chalk`、active は `paper-ink` の地

### 5.3 Danger（ゴースト）

- 透明地 + 1px `danger`（`#b42318`）枠、8px 角丸
- 文字は `chalk`。`danger` を文字色にしない（コントラスト 2.89:1 のため）
- 破壊的操作の最終確認にだけ使う

### 5.4 ステータスピル

- `paper-ink` 地 + 1px `graphite` + 4px 角丸 + padding 8×14
- mono 12px 大文字 `smoke`
- ライブ / 接続中のときだけ `pulse-green` の 6px ドット。未接続ではドットを出さない

### 5.5 表

- 罫線のみ（1px `graphite`）。行の塗りなし
- ヘッダーは mono 12px 大文字 `smoke`
- 選択行だけ `paper-ink`
- 外枠は 1px `graphite` + 8px 角丸。行高 44px

### 5.6 タブ

- 下線 1px。選択時 `chalk`、非選択 `transparent`。塗りなし
- ホバーは文字色だけ `chalk` に変える

### 5.7 ダイアログ / トースト

- `carbon` 地 + 1px `graphite` 枠 + 8px 角丸。**影なし**
- ダイアログの見出しは 23px `chalk`。閉じるは 99px 角丸のアイコンボタン
- トーストは mono の状態ラベル（完了 / 注意 / 失敗）+ 14px `chalk` の本文 + mono の補足

### 5.8 入力

- 透明地 + 1px `graphite` 枠 + 8px 角丸、14px `chalk`
- focus で 1px `chalk` 枠。影は使わない
- ラベルは mono 12px `smoke`。hint は 13px `smoke`、error は `aria-invalid` + `danger` 枠 + 直下の文言

### 5.9 アイコン

- インライン SVG の 1.5px ストローク、16 / 18 / 24px
- 色は `chalk` か `compass-gold` **だけ**。`smoke` などの副次色をアイコンに使わない
- 絵文字禁止。アイコンだけのボタンには `aria-label`

### 5.10 空状態

- 左寄せ。見出し 23px `chalk`、本文 16px `smoke`、主アクション 1 つ
- 中央揃えの縦積みにしない。アイコンは置かない

---

## 6. Padbench への適用マッピング

| UI | 適用 |
| --- | --- |
| 上部バー | `obsidian` 地 + 下 1px `graphite`。`BrandRing` は `compass-gold`、ワードマークは 16px `chalk`、副題とバージョンは mono 12px `smoke`。状態はステータスピル 1 つ |
| 左ナビ | `obsidian` 地 + 右 1px `graphite`。現在の画面は `chalk` 文字 + 左 1px `chalk` のヘアライン（塗りなし）。行高 36px。接続先ラベルは mono 12px |
| コンテンツ見出し | `h1` 44/46 `chalk`（-0.007em）。説明は 16/22 `smoke`。セクション見出しは 21/26 `chalk` |
| 右レール「書き込み前の確認」 | 透明地 + 1px `graphite` + 8px 角丸 + padding 24。件数は mono 12px `smoke`。行動は secondary |
| デバイス画面 | 空状態（見出し 23px）+ スキャン（primary・1 つだけ）+ 接続情報の表（罫線のみ） |
| 入力モニター | `carbon` 地 + 1px `graphite` のパネル。スティック円 1px `iron`、現在位置は `pulse-green` の 6px ドット、軌跡は `pulse-green` の 1px 線（不透明度だけ変化）。トリガーは 1px `iron` 枠 + `chalk` の 4px バー。数値は mono `chalk`、ラベルは mono 12px `smoke`。Y 軸は上が正（`Y_SIGN` 維持） |
| 設定 / マクロ / 現在値 / ログ（今後） | タブ下線、入力（透明 + `graphite`、focus `chalk`）、スイッチ（枠とつまみの位置）、表（罫線のみ） |
| ダイアログ（書き込み確認・ライセンス） | `carbon` + 1px `graphite` + 8px。影なし。フッターは secondary → primary の順 |
| トースト | `carbon` + 1px `graphite`。状態は mono ラベルで表す（色付きバーは使わない） |
| バナー | 透明地 + 1px 枠（warning は `graphite`、danger は `danger`）。soft の塗りは使わない |
| 無効状態 | 理由をボタン直下に 1 行。無効を押せる見た目にしない |

### 8 状態の担保

| 状態 | 実装 |
| --- | --- |
| default | 全コンポーネント |
| hover | 色・枠線のみ変化（transform を使わない） |
| active | 地を無彩色で 1 段変化（primary は `ash`） |
| focus-visible | `outline: 1px solid var(--chalk)` offset 2px（入力は 1px `chalk` 枠） |
| disabled | 理由を直下に表示。primary は白ピル 10% の輪郭を残す |
| loading | Button はスピナー + `aria-busy`（ラベルは変えない）、Table は骨組み 3 行 |
| error | Field は `aria-invalid` + `danger` 枠 + 直下の文言、Banner は danger |
| empty | EmptyState（左寄せ・見出し 23px・主アクション 1 つ）、Table は 1 行の空状態 |

---

## 7. 採用しなかった要素

- **dot-map の巨大ヒーロー**: マーケティング面の表現。Padbench のアプリ画面は Operate（作業）が主で、巨大な装飾は操作の密度を下げるため採用しない
- **63px 見出しの全面使用**: スケールには残すが、アプリでは `h1` 44px まで。63px は将来のオンボーディング / 空状態のヒーロー用に予約
- **写真・イラスト**: Padbench が扱うのは数値と入力状態で、写真の出番がない。入力モニターの線画が「製品の世界」を担う
- **色付きの塗り・線**: 白ピルと `pulse-green`（ドットと軌跡線）以外は使わない。状態色の soft 地も廃止
- **影**: すべて廃止（ダイアログ・トーストを含む）
- **Refero / Hyperstudio のロゴ・名称・固有アートワーク**: 使用しない（出典はトークンと数値のみ）

---

## 8. コントラスト実測

WCAG 2.x の相対輝度から `(L1 + 0.05) / (L2 + 0.05)` で計算した実測値です。本文・UI 文字は **AA 4.5:1 以上**、大きい文字・アイコン・操作の輪郭は **3:1 以上**を目標にします。

| 前景 | 背景 | 実測 | 判定 |
| --- | --- | --- | --- |
| `chalk` `#f3f3f3` | `obsidian` `#101010` | **17.15:1** | AA 可 |
| `chalk` | `carbon` `#080808` | **18.05:1** | AA 可 |
| `smoke` `#9c9c9c` | `obsidian` | **6.93:1** | AA 可 |
| `smoke` | `carbon` | **7.29:1** | AA 可 |
| `smoke` | `paper-ink` `#1a1a1a` | **6.34:1** | AA 可 |
| `ash` `#c1c1c1` | `obsidian` | **10.57:1** | AA 可 |
| `signal-white` `#ffffff`（ゴースト文字） | `obsidian` | **19.03:1** | AA 可 |
| `obsidian`（白ピルの文字） | `signal-white` | **19.03:1** | AA 可 |
| `compass-gold` `#6f6759`（アイコン線） | `obsidian` | **3.41:1** | アイコン 3:1 可 |
| `compass-gold` | `carbon` | **3.59:1** | アイコン 3:1 可 |
| `pulse-green` `#98ff38`（6px ドット） | `obsidian` | **15.13:1** | 3:1 可 |
| `pulse-green` | `paper-ink` | **13.84:1** | 3:1 可 |
| `danger` `#b42318`（1px 枠） | `carbon` | **3.05:1** | 輪郭 3:1 可（文字には使わない） |
| `danger`（1px 枠） | `obsidian` | 2.89:1 | 3:1 未満。文字にも枠の単独識別にも使わず、枠 + `chalk` のラベルで危険を伝える |
| `graphite` `#212121`（構造線） | `obsidian` | 1.18:1 | 装飾のヘアライン。文字・操作境界には使わない |
| `iron` `#474747`（図形の輪郭） | `carbon` | 2.16:1 | スティック円・トリガー枠。数値ラベルを併記するため単独識別に依存しない |

> `danger` の文字使用を避ける理由: `#b42318` on `#101010` は 2.89:1 で AA（4.5:1）に届きません。危険ボタンは `danger` の 1px 枠 + `chalk` のラベルで表します。これは Imported style の「danger は 1px 枠と文字」に対する最小の逸脱で、AA を優先した結果です（テストで固定）。

---

## 9. フォント同梱の TODO

- Inter / JetBrains Mono / IBM Plex Mono の woff2 サブセットを `apps/desktop/public/fonts/` に追加する
- `@font-face` に `font-display: swap` を付け、`<link rel="preload">` を `index.html` に追加する
- Noto Sans JP は Inter の後ろに置く（現状は OS フォールバック）
- フォントを同梱したら [THIRD_PARTY_LICENSES.md](../../THIRD_PARTY_LICENSES.md) と GUI ライセンス画面を更新する

---

## 10. 参照

- [docs/DESIGN_TAURI.md](../DESIGN_TAURI.md) §12 — 本スタイルの適用と §5 の置換注記
- [apps/desktop/README.md](../../apps/desktop/README.md) — 実装の構成とトークン参照
- [apps/desktop/src/styles/tokens.css](../../apps/desktop/src/styles/tokens.css) — 実装上の値の正本
- [apps/desktop/src/design/tokens.test.ts](../../apps/desktop/src/design/tokens.test.ts) — トークン一致・コントラスト・タイポのテスト
- [apps/desktop/src/design/style-audit.test.ts](../../apps/desktop/src/design/style-audit.test.ts) — 影・グラデ・塗り・角丸・アイコン色の自己監査
