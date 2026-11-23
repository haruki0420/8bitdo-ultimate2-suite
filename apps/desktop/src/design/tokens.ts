/**
 * Padbench design tokens — TypeScript 側の写し。
 *
 * 正本は docs/design/IMPORTED_STYLE_HYPERSTUDIO.md（Imported style: Hyperstudio）。
 * 実装上の値は `src/styles/tokens.css` と `src/index.css` の `@theme` にあります。
 * このファイルはテストとコンポーネントから同じ値を参照するための写しで、
 * `tokens.test.ts` が CSS との一致を検査します。
 * 値を変えるときは CSS とこのファイルとテストを同時に直してください。
 */

/** CSS 変数名 → HEX。tokens.css の `:root` と一致していることをテストで確認する。 */
export const COLORS = {
  "--obsidian": "#101010",
  "--carbon": "#080808",
  "--graphite": "#212121",
  "--card-slate": "#3b3d45",
  "--iron": "#474747",
  "--smoke": "#9c9c9c",
  "--ash": "#c1c1c1",
  "--chalk": "#f3f3f3",
  "--signal-white": "#ffffff",
  "--compass-gold": "#6f6759",
  "--pulse-green": "#98ff38",
  "--paper-ink": "#1a1a1a",
  "--danger": "#b42318",
} as const;

/**
 * 色付きの塗りは 2 つだけ（白ピルと pulse-green の 6px ドット）。
 * `--paper-ink` と `--graphite` は無彩色の面・罫線で、色付きの塗りには数えない。
 */
export const FILL_ACCENTS = ["--signal-white", "--pulse-green"] as const;

/** 本文・UI 文字に求める最小コントラスト（WCAG AA）。 */
export const CONTRAST_TEXT_MIN = 4.5;
/** 操作要素の輪郭・アイコンに求める最小コントラスト（SC 1.4.11）。 */
export const CONTRAST_UI_MIN = 3;

export interface TypeStep {
  /** `text-*` ユーティリティ名（index.css の `--text-*`） */
  token:
    | "hero"
    | "display"
    | "title"
    | "heading"
    | "subheading"
    | "lead"
    | "dialog"
    | "body"
    | "ui"
    | "meta"
    | "micro";
  size: number;
  lineHeight: number;
  /** 見出しも本文も 400 固定（Imported style のタイポ方針） */
  weight: 400;
  /** 字間（em）。負のトラッキングは大きい見出しだけ。 */
  tracking: number;
  usage: string;
}

/**
 * タイポグラフィスケール。Imported style の 10 段
 * （63 / 44 / 34 / 23 / 21 / 18 / 17 / 16 / 14 / 13）に、
 * 部品仕様が要求する mono 12（ステータスピル・表ヘッダー）を加えた 11 段。
 */
export const TYPE_SCALE: readonly TypeStep[] = [
  {
    token: "hero",
    size: 63,
    lineHeight: 66,
    weight: 400,
    tracking: -0.011,
    usage: "予約（アプリでは全面使用しない）",
  },
  {
    token: "display",
    size: 44,
    lineHeight: 46,
    weight: 400,
    tracking: -0.007,
    usage: "画面タイトル（h1）",
  },
  {
    token: "title",
    size: 34,
    lineHeight: 36,
    weight: 400,
    tracking: -0.004,
    usage: "大きな見出し（将来の空状態・ヒーロー帯）",
  },
  {
    token: "heading",
    size: 23,
    lineHeight: 28,
    weight: 400,
    tracking: 0,
    usage: "空状態の見出し・ダイアログの見出し",
  },
  {
    token: "subheading",
    size: 21,
    lineHeight: 26,
    weight: 400,
    tracking: 0,
    usage: "セクション見出し（h2 / h3）",
  },
  {
    token: "lead",
    size: 18,
    lineHeight: 24,
    weight: 400,
    tracking: 0,
    usage: "導入文（アプリでは使わない）",
  },
  {
    token: "dialog",
    size: 17,
    lineHeight: 22,
    weight: 400,
    tracking: 0,
    usage: "ダイアログ・パネルの副見出し",
  },
  {
    token: "body",
    size: 16,
    lineHeight: 22,
    weight: 400,
    tracking: 0,
    usage: "本文・説明（smoke）",
  },
  {
    token: "ui",
    size: 14,
    lineHeight: 20,
    weight: 400,
    tracking: 0,
    usage: "ボタン・タブ・入力・表セル・ナビ",
  },
  {
    token: "meta",
    size: 13,
    lineHeight: 18,
    weight: 400,
    tracking: 0,
    usage: "補助・単位・数値（mono）",
  },
  {
    token: "micro",
    size: 12,
    lineHeight: 16,
    weight: 400,
    tracking: 0.02,
    usage: "ステータスピル・表ヘッダー（mono 大文字）",
  },
] as const;

/** 4px グリッドの余白スケール。 */
export const SPACE = [4, 8, 12, 16, 20, 24, 40] as const;

/** 角丸（tags 4 / cards 8 / icons 99 / 白ピル 9999）。 */
export const RADIUS = { xs: 4, md: 8, icon: 99, full: 9999 } as const;

/** モーション。曲線は 1 本、時間は 3 段。 */
export const MOTION = {
  durations: [90, 160, 240],
  reducedDuration: 0,
  easeStandard: "cubic-bezier(0.2, 0, 0, 1)",
} as const;

/** tokens.css に置く寸法トークン（rem）。200% ズームでも崩れないため rem で持つ。 */
export const DIMENSIONS = {
  "--topbar-h": "3.25rem",
  "--nav-w": "13.5rem",
  "--rail-w": "21.5rem",
  "--rail-w-min": "20rem",
  "--content-max": "75rem",
  "--control-h": "2.5rem",
  "--control-h-sm": "2.25rem",
  "--row-h": "2.75rem",
  "--row-header-h": "2.25rem",
  "--tab-h": "2.5rem",
  "--nav-row-h": "2.25rem",
  "--stage-h": "15rem",
  "--stage-h-short": "12.5rem",
  "--dialog-w": "30rem",
  "--dialog-w-lg": "35rem",
} as const;

/** 寸法の実測値（px）。DIMENSIONS の rem 値と一致することをテストで確認する。 */
export const LAYOUT = {
  topbarHeight: 52,
  navWidth: 216,
  railWidth: 344,
  railWidthMin: 320,
  contentMaxWidth: 1200,
  contentPadding: 24,
  controlHeight: 40,
  controlHeightCompact: 36,
  tableHeaderHeight: 36,
  tableRowHeight: 44,
  tabHeight: 40,
  navRowHeight: 36,
  stageHeight: 240,
  stageHeightShort: 200,
  dialogWidth: 480,
  dialogWidthConfirm: 560,
  breakpointRail: 1180,
  breakpointWide: 1200,
  breakpointTableColumns: 1100,
  windowMinWidth: 900,
  windowMinHeight: 620,
} as const;

/** 用語集（docs/DESIGN_TAURI.md §6.3）。UI 文言はこの表の採用表記に合わせる。 */
export const VOCAB = {
  inputMonitor: "入力モニター",
  currentValues: "現在値",
  record: "記録",
  writeCapable: "書き込み対応版",
  nextAction: "次にすること",
  write: "書き込み",
  read: "読み込み",
  connect: "接続",
  disconnect: "切断",
} as const;
