import { describe, expect, it } from "vitest";

// 実装ファイルをそのまま読み、Imported style の自己監査項目をテストで固定する。
// Node の fs を使わず、Vite の ?raw で読む（tsc に @types/node を足さないため）。
const RAW_SOURCES = import.meta.glob(["../**/*.{ts,tsx,css}"], {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

function rawSource(key: string): string {
  const value: unknown = RAW_SOURCES[key];
  if (typeof value === "string") {
    return value;
  }
  if (value !== null && typeof value === "object" && "default" in value) {
    const nested = (value as { default: unknown }).default;
    if (typeof nested === "string") {
      return nested;
    }
  }
  throw new Error(`unreadable source: ${key}`);
}

/**
 * コメントを除いた本文。コメント内の例示（旧クラス名・色名など）で
 * 偽陽性を出さないため、すべての検査をこの本文に対して行う。
 */
function stripComments(text: string): string {
  return text
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/^\s*\/\/.*$/gm, "");
}

/** `src/` 配下の実装ファイル（テスト自身は除く）。コメントは除いて検査する。 */
const SOURCES = Object.keys(RAW_SOURCES)
  .filter((path) => !/\.test\.tsx?$/.test(path))
  .map((path) => ({ path, text: stripComments(rawSource(path)) }));

const ALL_TEXT = SOURCES.map(({ path, text }) => `/* ${path} */\n${text}`).join(
  "\n",
);

function filesMatching(pattern: RegExp): string[] {
  return SOURCES.filter(({ text }) => pattern.test(text)).map(
    ({ path }) => path,
  );
}

describe("自己監査: 色・影・グラデーション", () => {
  it("実装ファイルを 1 件以上走査している", () => {
    // glob の解決に失敗すると以下の監査がすべて空振りで通るため、先に固定する。
    expect(SOURCES.length).toBeGreaterThan(0);
  });

  it("旧ライト面のユーティリティを残さない", () => {
    for (const banned of [
      "bg-panel",
      "bg-sunken",
      "bg-bg",
      "bg-hover",
      "bg-active",
      "bg-accent",
      "text-accent",
      "border-accent",
      "text-ink",
      "text-muted",
      "border-line",
      "accent-soft",
      "ink-fill",
      "on-ink",
      "warning-soft",
      "danger-soft",
      "danger-fill",
      "success-soft",
      "bg-stage",
      "text-stage",
      "border-stage",
      "stroke-stage",
      "fill-stage",
    ]) {
      expect(ALL_TEXT, `banned class: ${banned}`).not.toContain(banned);
    }
  });

  it("影を使わない（shadow ユーティリティ 0 件）", () => {
    expect(ALL_TEXT).not.toMatch(/\bshadow-|\bshadow\b/);
  });

  it("グラデーションを使わない", () => {
    expect(ALL_TEXT).not.toMatch(/gradient/i);
  });

  it("色付きの塗りは白ピルと pulse-green ドットだけ", () => {
    const whiteFill = filesMatching(/bg-signal-white/);
    expect(whiteFill.sort()).toEqual(["../components/Button.tsx"]);

    const greenFill = filesMatching(/bg-pulse-green|fill-pulse-green/);
    expect(greenFill.sort()).toEqual([
      "../components/StatusPill.tsx",
      "../screens/InputMonitorScreen.tsx",
    ]);
  });

  it("影の代替である focus ring は outline で描く", () => {
    expect(ALL_TEXT).toContain(":focus-visible");
    expect(ALL_TEXT).toContain("outline: 1px solid var(--chalk)");
  });
});

describe("自己監査: 値の許可リスト", () => {
  /** glob のキーは `./tokens.ts` や `../styles/tokens.css` の形で返る。 */
  function isAllowedPath(path: string, allowed: readonly string[]): boolean {
    const normalized = path.replace(/^\.\//, "");
    return allowed.some(
      (name) => normalized === name || normalized.endsWith(`/${name}`),
    );
  }

  it("6/8 桁の生の色は tokens.css / tokens.ts だけ", () => {
    const allowed = ["styles/tokens.css", "tokens.ts"] as const;
    const offenders = SOURCES.filter(
      ({ path, text }) =>
        !isAllowedPath(path, allowed) &&
        // 3 桁は使わない。`#1234` のようなチケット番号を色と誤認しない。
        /#[0-9a-fA-F]{6}(?:[0-9a-fA-F]{2})?\b/.test(text),
    ).map(({ path }) => path);
    expect(offenders).toEqual([]);
  });

  it("rgb() / hsl() は tokens.css と tokens.ts のスクリムだけ", () => {
    const allowed = ["styles/tokens.css", "tokens.ts"] as const;
    const offenders = SOURCES.filter(
      ({ path, text }) =>
        !isAllowedPath(path, allowed) &&
        /\b(?:rgba?|hsla?)\(/i.test(text),
    ).map(({ path }) => path);
    expect(offenders).toEqual([]);
  });

  it("スケール外の任意値ユーティリティ（p-[13px] / bg-[#...] / text-[13px]）を書かない", () => {
    // Tailwind のバリアント（data-[state=open] など）はデザイントークンの
    // 任意値ではないため許可する（任意値の混入検査から除外する）。
    const allowedVariant = /^(?:data|aria|group|peer|has|not|supports|nth)-\[/;
    const matches = [
      ...ALL_TEXT.matchAll(/(?:^|[\s"'`])([a-z][a-z0-9]*(?:-[a-z0-9]+)*)-\[/g),
    ]
      .map((match) => `${match[1] ?? ""}-[`)
      .filter((token) => !allowedVariant.test(token));
    expect(matches).toEqual([]);
  });

  it("インライン style はトリガーバーの幅指定だけ", () => {
    const inline = [...ALL_TEXT.matchAll(/style=\{\{([\s\S]*?)\}\}/g)].map(
      (match) => match[1] ?? "",
    );
    // 許容しているのは 1 か所だけ。増やすときはこのリストに追記する。
    expect(inline).toHaveLength(1);
    for (const body of inline) {
      const keys = [...body.matchAll(/([A-Za-z][A-Za-z-]*)\s*:/g)].map(
        (match) => match[1],
      );
      expect(keys).toEqual(["width"]);
    }
  });

  it("font-weight は 400 だけ（ユーティリティ以外の直書きも検出する）", () => {
    const weights = [...ALL_TEXT.matchAll(/font-weight\s*:\s*([^;}\n]+)/gi)].map(
      (match) => (match[1] ?? "").trim(),
    );
    for (const weight of weights) {
      expect(["400", "normal"]).toContain(weight);
    }
    expect(ALL_TEXT).not.toMatch(
      /fontWeight\s*:\s*["']?(?:[5-9]\d\d|bold|semibold|medium|extrabold|black)\b/,
    );
  });
});

describe("自己監査: タイポグラフィ", () => {
  it("weight 500 / 600 / 700 のユーティリティを使わない", () => {
    expect(ALL_TEXT).not.toMatch(
      /font-(medium|semibold|bold|extrabold|black|thin|light)\b/,
    );
  });

  it("字間は 63px / 44px / 34px の見出しと meta だけ", () => {
    const matches = ALL_TEXT.match(/tracking-[a-z]+/g) ?? [];
    const allowed = new Set([
      "tracking-hero",
      "tracking-display",
      "tracking-title",
      "tracking-none",
      "tracking-meta",
    ]);
    for (const match of matches) {
      expect(allowed.has(match), `unexpected ${match}`).toBe(true);
    }
  });
});

describe("自己監査: 形・アイコン", () => {
  it("角丸は 4 / 8 / 99 / 9999 のトークンだけ", () => {
    const matches = ALL_TEXT.match(/rounded-[a-z]+/g) ?? [];
    const allowed = new Set([
      "rounded-xs",
      "rounded-md",
      "rounded-icon",
      "rounded-full",
    ]);
    for (const match of matches) {
      expect(allowed.has(match), `unexpected ${match}`).toBe(true);
    }
  });

  it("アイコンの色は chalk か compass-gold だけ", () => {
    const icons = SOURCES.find(({ path }) => path === "../components/icons.tsx");
    expect(icons).toBeDefined();
    expect(icons?.text).not.toMatch(
      /text-(smoke|ash|iron|graphite|danger)\b|stroke-(smoke|ash|iron|graphite|danger)\b/,
    );
    // 色は currentColor を継承し、呼び出し側が chalk / compass-gold を指定する。
    expect(icons?.text).toContain('stroke="currentColor"');
  });

  it("絵文字を使わない", () => {
    const emoji =
      /[\u{1f000}-\u{1faff}\u{2600}-\u{27bf}\u{fe0f}\u{1f1e6}-\u{1f1ff}]/u;
    expect(emoji.test(ALL_TEXT)).toBe(false);
  });

  it("見出し・本文の色は chalk / smoke / ash に限る", () => {
    for (const banned of [
      "text-pulse-green",
      "text-danger",
      "text-iron",
      "text-graphite",
      "text-card-slate",
    ]) {
      expect(ALL_TEXT, `banned text color: ${banned}`).not.toContain(banned);
    }
  });
});
