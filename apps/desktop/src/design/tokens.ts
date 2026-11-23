/**
 * Padbench design tokens — TypeScript 側の写し。
 *
 * 正本は docs/DESIGN_TAURI.md §5、実装上の値は `src/styles/tokens.css` と
 * `src/index.css` の `@theme` にあります。このファイルはテストとコンポーネントから
 * 同じ値を参照するための写しで、`tokens.test.ts` が CSS との一致を検査します。
 * 値を変えるときは CSS とこのファイルとテストを同時に直してください。
 */

/** CSS 変数名 → HEX。tokens.css の `:root` と一致していることをテストで確認する。 */
export const COLORS = {
  "--surface-bg": "#F4F6F7",
  "--surface-panel": "#FFFFFF",
  "--surface-sunken": "#EDF0F2",
  "--surface-hover": "#F1F4F5",
  "--surface-active": "#E7EBEE",
  "--line": "#D6DCE0",
  "--line-strong": "#7C868F",
  "--text": "#15191C",
  "--text-2": "#3A434B",
  "--text-muted": "#5B656E",
  "--text-disabled": "#646E78",
  "--accent": "#0B6A63",
  "--accent-soft": "#E3F1EF",
  "--ink-fill": "#15191C",
  "--ink-hover": "#2A3138",
  "--ink-active": "#0B0E10",
  "--on-ink": "#FFFFFF",
  "--warning": "#8A5200",
  "--warning-soft": "#FDF4E3",
  "--danger": "#AE2418",
  "--danger-soft": "#FCEFEE",
  "--danger-fill": "#A3211A",
  "--danger-fill-hover": "#8E1C13",
  "--success": "#15653C",
  "--success-soft": "#E7F4EC",
} as const;

/** `[data-surface="stage"]` のスコープでだけ有効な色（§5.1.4）。 */
export const STAGE_COLORS = {
  "--stage-bg": "#0E1115",
  "--stage-panel": "#141A1F",
  "--stage-line": "#2A3239",
  "--stage-line-strong": "#3C4752",
  "--stage-text": "#E7EAEE",
  "--stage-text-2": "#A9B2BC",
  "--stage-text-muted": "#7E8893",
  "--stage-led": "#6FE7D8",
  "--stage-led-dim": "#3A6F69",
  "--stage-amber": "#E9B65C",
  "--stage-danger": "#FF8F8F",
} as const;

/**
 * アクセントは 1 色だけ（§5.1.1 ルール 2）。
 * 成功・注意・異常は状態色で、アクセントとして数えない。
 */
export const ACCENT = "#0B6A63";

/** 本文・UI 文字に求める最小コントラスト（WCAG AA）。 */
export const CONTRAST_TEXT_MIN = 4.5;
/** 操作要素の輪郭とフォーカスに求める最小コントラスト（SC 1.4.11）。 */
export const CONTRAST_UI_MIN = 3;

export interface TypeStep {
  /** `text-*` ユーティリティ名（index.css の `--text-*`） */
  token: "display" | "title" | "heading" | "body" | "ui" | "meta" | "data" | "caption";
  size: number;
  lineHeight: number;
  weight: 400 | 500 | 700;
  usage: string;
}

/** 8 段のタイポグラフィスケール（§5.2.2）。11px 未満は作らない。 */
export const TYPE_SCALE: readonly TypeStep[] = [
  { token: "display", size: 24, lineHeight: 34, weight: 700, usage: "画面タイトル（h1）" },
  { token: "title", size: 18, lineHeight: 28, weight: 700, usage: "ダイアログの見出し" },
  { token: "heading", size: 15, lineHeight: 24, weight: 500, usage: "セクション見出し（h2 / h3）" },
  { token: "body", size: 14, lineHeight: 24, weight: 400, usage: "本文・説明・空状態" },
  { token: "ui", size: 13, lineHeight: 20, weight: 500, usage: "ボタン・タブ・入力・表セル・ナビ" },
  { token: "meta", size: 12, lineHeight: 18, weight: 400, usage: "補助・単位・タイムスタンプ" },
  { token: "data", size: 12, lineHeight: 18, weight: 400, usage: "数値・hex・アドレス（Hack）" },
  { token: "caption", size: 11, lineHeight: 16, weight: 500, usage: "バッジ・注記" },
] as const;

/** 4px グリッドの余白スケール（§5.3.1）。Tailwind 既定の spacing と同じ値。 */
export const SPACE = [4, 8, 12, 16, 20, 24, 32, 40, 48, 64] as const;

/** 角丸（§5.3.3）。 */
export const RADIUS = { xs: 4, sm: 6, md: 8, lg: 12, full: 9999 } as const;

/** 影はポップアップ用の 1 種だけ（§5.3.4）。 */
export const SHADOW_POP =
  "0 1px 2px rgba(16, 20, 24, 0.06), 0 8px 24px rgba(16, 20, 24, 0.1)";
export const SHADOW_FOCUS =
  "0 0 0 2px var(--surface-panel), 0 0 0 4px var(--accent)";

/** モーション（§5.7.1）。曲線は 1 本、時間は 3 段。 */
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
  "--content-max": "45rem",
  "--control-h": "2rem",
  "--control-h-sm": "1.75rem",
  "--row-h": "2.25rem",
  "--row-header-h": "2rem",
  "--tab-h": "2.5rem",
  "--nav-row-h": "2.125rem",
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
  contentMaxWidth: 720,
  contentPadding: 24,
  controlHeight: 32,
  controlHeightCompact: 28,
  tableHeaderHeight: 32,
  tableRowHeight: 36,
  tabHeight: 40,
  navRowHeight: 34,
  stageHeight: 240,
  stageHeightShort: 200,
  dialogWidth: 480,
  dialogWidthConfirm: 560,
  breakpointRail: 1180,
  breakpointTableColumns: 1100,
  windowMinWidth: 900,
  windowMinHeight: 620,
} as const;

/** 用語集（§6.3）。UI 文言はこの表の採用表記に合わせる。 */
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
