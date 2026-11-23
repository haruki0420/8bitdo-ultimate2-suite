import { describe, expect, it } from "vitest";

// CSS は Vite の ?raw でそのまま読む（vitest の `test.css: true` が必要）。
import themeCss from "../index.css?raw";
import tokensCss from "../styles/tokens.css?raw";
import { contrastRatio } from "./contrast";
import {
  COLORS,
  CONTRAST_TEXT_MIN,
  CONTRAST_UI_MIN,
  DIMENSIONS,
  FILL_ACCENTS,
  LAYOUT,
  MOTION,
  OVERLAY,
  RADIUS,
  SPACE,
  TYPE_SCALE,
} from "./tokens";

function cssVar(css: string, name: string): string {
  const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = new RegExp(`${escaped}\\s*:\\s*([^;]+);`).exec(css);
  const value = match?.[1];
  if (value === undefined) {
    throw new Error(`missing CSS variable: ${name}`);
  }
  return value.trim();
}

function px(value: string): number {
  const match = /^(-?[\d.]+)px$/.exec(value);
  const number = match?.[1];
  if (number === undefined) {
    throw new Error(`px expected: ${value}`);
  }
  return Number.parseFloat(number);
}

function remToPx(value: string): number {
  const match = /^(-?[\d.]+)rem$/.exec(value);
  const number = match?.[1];
  if (number === undefined) {
    throw new Error(`rem expected: ${value}`);
  }
  return Math.round(Number.parseFloat(number) * 16);
}

describe("色トークン（Imported style: Hyperstudio）", () => {
  it("tokens.ts の色が tokens.css と一致する", () => {
    for (const [name, hex] of Object.entries(COLORS)) {
      expect(cssVar(tokensCss, name).toLowerCase()).toBe(hex.toLowerCase());
    }
  });

  it("ライト面（design C）のトークンとアクセントを残さない", () => {
    for (const banned of [
      "--surface-bg",
      "--surface-panel",
      "--surface-sunken",
      "--surface-hover",
      "--surface-active",
      "--line",
      "--line-strong",
      "--text",
      "--text-2",
      "--text-muted",
      "--text-disabled",
      "--accent",
      "--accent-soft",
      "--ink-fill",
      "--on-ink",
      "--warning",
      "--warning-soft",
      "--danger-soft",
      "--danger-fill",
      "--success",
      "--success-soft",
      "--stage-bg",
      "--stage-panel",
      "--stage-line",
      "--stage-text",
      "--stage-led",
      "--stage-amber",
      "--stage-danger",
    ]) {
      expect(tokensCss, `banned token: ${banned}`).not.toContain(banned);
    }
  });

  it("色付きの塗りは白ピルと pulse-green のドットだけ", () => {
    expect(FILL_ACCENTS).toEqual(["--signal-white", "--pulse-green"]);
    // 白ピル以外に signal-white を面の塗りとして使わない運用は style-audit.test.ts で検査する。
    expect(COLORS["--signal-white"]).toBe("#ffffff");
    expect(COLORS["--pulse-green"]).toBe("#98ff38");
  });

  it("スクリム --overlay は tokens.css / tokens.ts で一致する", () => {
    expect(cssVar(tokensCss, "--overlay")).toBe(OVERLAY);
  });

  it("グラデーションを使わない", () => {
    expect(`${tokensCss}\n${themeCss}`).not.toMatch(
      /gradient|linear-gradient|radial-gradient/i,
    );
  });

  it("影を一切使わない（--shadow-pop を削除済み）", () => {
    expect(`${tokensCss}\n${themeCss}`).not.toMatch(
      /box-shadow|text-shadow|drop-shadow|--shadow/,
    );
  });
});

describe("コントラスト（実測値をテストで固定する）", () => {
  it("主テキスト chalk は obsidian / carbon で AA を満たす", () => {
    for (const surface of ["--obsidian", "--carbon"] as const) {
      expect(
        contrastRatio(COLORS["--chalk"], COLORS[surface]),
        `chalk on ${surface}`,
      ).toBeGreaterThanOrEqual(CONTRAST_TEXT_MIN);
    }
  });

  it("本文・副次テキスト smoke は obsidian / carbon / paper-ink で AA を満たす", () => {
    for (const surface of ["--obsidian", "--carbon", "--paper-ink"] as const) {
      expect(
        contrastRatio(COLORS["--smoke"], COLORS[surface]),
        `smoke on ${surface}`,
      ).toBeGreaterThanOrEqual(CONTRAST_TEXT_MIN);
    }
  });

  it("三次テキスト ash は obsidian で AA を満たす", () => {
    expect(
      contrastRatio(COLORS["--ash"], COLORS["--obsidian"]),
    ).toBeGreaterThanOrEqual(CONTRAST_TEXT_MIN);
  });

  it("白ピルの文字 obsidian は signal-white で AA を満たす", () => {
    expect(
      contrastRatio(COLORS["--obsidian"], COLORS["--signal-white"]),
    ).toBeGreaterThanOrEqual(CONTRAST_TEXT_MIN);
  });

  it("compass-gold のアイコン線は obsidian / carbon で 3:1 以上", () => {
    for (const surface of ["--obsidian", "--carbon"] as const) {
      expect(
        contrastRatio(COLORS["--compass-gold"], COLORS[surface]),
        `compass-gold on ${surface}`,
      ).toBeGreaterThanOrEqual(CONTRAST_UI_MIN);
    }
  });

  it("pulse-green のドットは obsidian / paper-ink で 3:1 以上", () => {
    for (const surface of ["--obsidian", "--paper-ink"] as const) {
      expect(
        contrastRatio(COLORS["--pulse-green"], COLORS[surface]),
        `pulse-green on ${surface}`,
      ).toBeGreaterThanOrEqual(CONTRAST_UI_MIN);
    }
  });

  it("danger は 1px 枠の carbon 上で 3:1 以上（文字には使わない）", () => {
    // danger #B42318 は obsidian 上で 2.89:1 のため text では使わず、
    // 1px 枠と chalk の文字を併用する（docs/design/IMPORTED_STYLE_HYPERSTUDIO.md）。
    expect(
      contrastRatio(COLORS["--danger"], COLORS["--carbon"]),
    ).toBeGreaterThanOrEqual(CONTRAST_UI_MIN);
    expect(
      contrastRatio(COLORS["--danger"], COLORS["--obsidian"]),
    ).toBeLessThan(CONTRAST_UI_MIN);
  });
});

describe("タイポグラフィ（Imported style）", () => {
  it("スケールは 63 / 44 / 34 / 23 / 21 / 18 / 17 / 16 / 14 / 13 + mono 12", () => {
    expect(TYPE_SCALE.map((step) => step.size)).toEqual([
      63, 44, 34, 23, 21, 18, 17, 16, 14, 13, 12,
    ]);
  });

  it("すべて weight 400（700 / 500 を使わない）", () => {
    for (const step of TYPE_SCALE) {
      expect(step.weight, step.token).toBe(400);
    }
  });

  it("行間は 0.95〜1.43、display は 1.05", () => {
    for (const step of TYPE_SCALE) {
      const ratio = step.lineHeight / step.size;
      expect(ratio, `${step.token} ${ratio.toFixed(3)}`).toBeGreaterThanOrEqual(
        0.95,
      );
      expect(ratio, `${step.token} ${ratio.toFixed(3)}`).toBeLessThanOrEqual(
        1.43,
      );
    }
    expect(TYPE_SCALE[0]?.lineHeight).toBe(66);
    expect(TYPE_SCALE[1]?.lineHeight).toBe(46);
  });

  it("負のトラッキングは大きい見出しだけ（小さい文字は 0 以上）", () => {
    for (const step of TYPE_SCALE) {
      if (step.size >= 34) {
        expect(step.tracking, step.token).toBeLessThan(0);
      } else {
        expect(step.tracking, step.token).toBeGreaterThanOrEqual(0);
      }
    }
    expect(TYPE_SCALE.find((step) => step.token === "hero")?.tracking).toBe(
      -0.011,
    );
    expect(TYPE_SCALE.find((step) => step.token === "display")?.tracking).toBe(
      -0.007,
    );
  });

  it("index.css の --text-* / --text-*--line-height が tokens.ts と一致する", () => {
    for (const step of TYPE_SCALE) {
      const size = cssVar(themeCss, `--text-${step.token}`);
      const lineHeight = cssVar(themeCss, `--text-${step.token}--line-height`);
      expect(remToPx(size), `size ${step.token}`).toBe(step.size);
      expect(remToPx(lineHeight), `line-height ${step.token}`).toBe(
        step.lineHeight,
      );
    }
  });

  it("字間トークン: 63px -0.011em / 44px -0.007em / 小さい文字は 0", () => {
    expect(cssVar(themeCss, "--tracking-hero")).toBe("-0.011em");
    expect(cssVar(themeCss, "--tracking-display")).toBe("-0.007em");
    expect(cssVar(themeCss, "--tracking-title")).toBe("-0.004em");
    expect(cssVar(themeCss, "--tracking-none")).toBe("0em");
    expect(cssVar(themeCss, "--tracking-meta")).toBe("0.02em");
  });

  it("書体は Inter → Noto Sans JP、mono は JetBrains Mono / IBM Plex Mono", () => {
    const ui = cssVar(tokensCss, "--font-ui").replace(/\s+/g, " ");
    const mono = cssVar(tokensCss, "--font-data").replace(/\s+/g, " ");
    expect(ui.indexOf("Inter")).toBeLessThan(ui.indexOf("Noto Sans JP"));
    expect(mono).toContain("JetBrains Mono");
    expect(mono).toContain("IBM Plex Mono");
  });
});

describe("余白・角丸・影", () => {
  it("余白は 4px グリッド（4 / 8 / 12 / 16 / 20 / 24 / 40）", () => {
    expect(SPACE).toEqual([4, 8, 12, 16, 20, 24, 40]);
    for (const space of SPACE) {
      expect(space % 4).toBe(0);
    }
  });

  it("角丸は 4 / 8 / 99 / 9999（白ピルだけ 9999）", () => {
    expect(px(cssVar(themeCss, "--radius-xs"))).toBe(RADIUS.xs);
    expect(px(cssVar(themeCss, "--radius-md"))).toBe(RADIUS.md);
    expect(px(cssVar(themeCss, "--radius-icon"))).toBe(RADIUS.icon);
    expect(px(cssVar(themeCss, "--radius-full"))).toBe(RADIUS.full);
    expect(themeCss).not.toMatch(/--radius-(sm|lg|xl|2xl|3xl)/);
  });

  it("影のトークンを持たない", () => {
    expect(themeCss).not.toMatch(/--shadow/);
    expect(tokensCss).not.toMatch(/--shadow/);
  });
});

describe("寸法・レイアウト", () => {
  it("tokens.css の寸法トークンが DIMENSIONS と一致する", () => {
    for (const [name, value] of Object.entries(DIMENSIONS)) {
      expect(cssVar(tokensCss, name)).toBe(value);
    }
  });

  it("rem 換算値が実測 px と一致する（200% ズーム対応）", () => {
    const expected = [
      ["--topbar-h", LAYOUT.topbarHeight],
      ["--nav-w", LAYOUT.navWidth],
      ["--rail-w", LAYOUT.railWidth],
      ["--rail-w-min", LAYOUT.railWidthMin],
      ["--content-max", LAYOUT.contentMaxWidth],
      ["--control-h", LAYOUT.controlHeight],
      ["--control-h-sm", LAYOUT.controlHeightCompact],
      ["--row-h", LAYOUT.tableRowHeight],
      ["--row-header-h", LAYOUT.tableHeaderHeight],
      ["--tab-h", LAYOUT.tabHeight],
      ["--nav-row-h", LAYOUT.navRowHeight],
      ["--stage-h", LAYOUT.stageHeight],
      ["--stage-h-short", LAYOUT.stageHeightShort],
      ["--dialog-w", LAYOUT.dialogWidth],
      ["--dialog-w-lg", LAYOUT.dialogWidthConfirm],
    ] as const;

    for (const [name, value] of expected) {
      expect(remToPx(cssVar(tokensCss, name)), name).toBe(value);
    }
  });

  it("コンテンツ最大幅は 1200px", () => {
    expect(LAYOUT.contentMaxWidth).toBe(1200);
    expect(remToPx(cssVar(tokensCss, "--content-max"))).toBe(1200);
  });

  it("1180px / 1100px / 1200px のブレークポイントが一致する", () => {
    expect(remToPx(cssVar(themeCss, "--breakpoint-rail"))).toBe(
      LAYOUT.breakpointRail,
    );
    expect(remToPx(cssVar(themeCss, "--breakpoint-cols"))).toBe(
      LAYOUT.breakpointTableColumns,
    );
    expect(remToPx(cssVar(themeCss, "--breakpoint-wide"))).toBe(
      LAYOUT.breakpointWide,
    );
  });

  it("最小ウィンドウは 900×620", () => {
    expect(LAYOUT.windowMinWidth).toBe(900);
    expect(LAYOUT.windowMinHeight).toBe(620);
  });
});

describe("モーション", () => {
  it("90 / 160 / 240ms の 3 段", () => {
    expect(ms(cssVar(tokensCss, "--dur-1"))).toBe(MOTION.durations[0]);
    expect(ms(cssVar(tokensCss, "--dur-2"))).toBe(MOTION.durations[1]);
    expect(ms(cssVar(tokensCss, "--dur-3"))).toBe(MOTION.durations[2]);
  });

  it("曲線は 1 本だけ（index.css の @theme にだけ置く）", () => {
    expect(cssVar(themeCss, "--ease-standard").replace(/\s+/g, " ")).toBe(
      MOTION.easeStandard,
    );
    // 値の重複定義を作らない: tokens.css 側では宣言しない（コメントでの言及は可）。
    expect(tokensCss).not.toMatch(/--ease-standard\s*:/);
    expect(themeCss.match(/--ease-standard\s*:/g)).toHaveLength(1);
  });

  it("prefers-reduced-motion で 0ms にする", () => {
    expect(tokensCss).toMatch(
      /@media\s*\(prefers-reduced-motion:\s*reduce\)[\s\S]*?--dur-1:\s*0ms/,
    );
  });
});

describe("写しの完全性", () => {
  it("tokens.css の CSS 変数はすべて tokens.ts の写しにある", () => {
    const known = new Set<string>([
      ...Object.keys(COLORS),
      ...Object.keys(DIMENSIONS),
      "--overlay",
      "--font-ui",
      "--font-data",
      "--dur-1",
      "--dur-2",
      "--dur-3",
    ]);
    const declared = [
      ...tokensCss.matchAll(/^\s*(--[a-z0-9-]+)\s*:/gm),
    ].map((match) => match[1] ?? "");
    const unknown = [...new Set(declared)].filter((name) => !known.has(name));
    expect(unknown).toEqual([]);
  });
});

function ms(value: string): number {
  if (value.endsWith("ms")) {
    return Number.parseFloat(value);
  }
  throw new Error(`ms expected: ${value}`);
}
