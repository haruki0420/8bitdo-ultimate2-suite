import { describe, expect, it } from "vitest";

// CSS は Vite の ?raw でそのまま読む（vitest の `test.css: true` が必要）。
import themeCss from "../index.css?raw";
import tokensCss from "../styles/tokens.css?raw";
import { contrastRatio } from "./contrast";
import {
  ACCENT,
  COLORS,
  CONTRAST_TEXT_MIN,
  CONTRAST_UI_MIN,
  DIMENSIONS,
  LAYOUT,
  MOTION,
  RADIUS,
  SHADOW_POP,
  SPACE,
  STAGE_COLORS,
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

describe("色トークン", () => {
  it("tokens.ts のライト面の色が tokens.css と一致する", () => {
    for (const [name, hex] of Object.entries(COLORS)) {
      expect(cssVar(tokensCss, name).toLowerCase()).toBe(hex.toLowerCase());
    }
  });

  it("tokens.ts のステージの色が tokens.css と一致する", () => {
    for (const [name, hex] of Object.entries(STAGE_COLORS)) {
      expect(cssVar(tokensCss, name).toLowerCase()).toBe(hex.toLowerCase());
    }
  });

  it("アクセントは 1 色だけ（§5.1.1 ルール 2）", () => {
    expect(COLORS["--accent"]).toBe(ACCENT);

    const sameAsAccent = Object.entries(COLORS).filter(
      ([name, hex]) => hex === ACCENT && name !== "--accent",
    );
    expect(sameAsAccent).toHaveLength(0);

    const accentTokens = Object.keys(COLORS).filter((name) =>
      name.includes("accent"),
    );
    expect(accentTokens.sort()).toEqual(["--accent", "--accent-soft"]);
  });

  it("ソフト地には状態色だけを使う（アクセントを状態表示に使わない）", () => {
    expect(COLORS["--accent-soft"]).not.toBe(COLORS["--warning-soft"]);
    expect(COLORS["--accent-soft"]).not.toBe(COLORS["--danger-soft"]);
  });

  it("グラデーションを使わない（§5.1.1 ルール 5）", () => {
    expect(`${tokensCss}\n${themeCss}`).not.toMatch(/gradient/i);
  });
});

describe("コントラスト（§5.1.5 の実測値をテストで固定する）", () => {
  const lightSurfaces = [
    "--surface-panel",
    "--surface-bg",
    "--surface-sunken",
  ] as const;

  const textTokens = [
    "--text",
    "--text-2",
    "--text-muted",
    "--text-disabled",
    "--accent",
    "--success",
    "--warning",
    "--danger",
  ] as const;

  it.each(textTokens)("%s はライト面で AA を満たす", (token) => {
    for (const surface of lightSurfaces) {
      expect(
        contrastRatio(COLORS[token], COLORS[surface]),
        `${token} on ${surface}`,
      ).toBeGreaterThanOrEqual(CONTRAST_TEXT_MIN);
    }
  });

  it("操作要素の輪郭 --line-strong はライト面で 3:1 以上", () => {
    for (const surface of lightSurfaces) {
      expect(
        contrastRatio(
          COLORS["--line-strong"],
          COLORS[surface],
        ),
        `line-strong on ${surface}`,
      ).toBeGreaterThanOrEqual(CONTRAST_UI_MIN);
    }
  });

  it("状態色は soft 地で AA を満たす", () => {
    const pairs = [
      ["--warning", "--warning-soft"],
      ["--danger", "--danger-soft"],
      ["--success", "--success-soft"],
    ] as const;

    for (const [fg, bg] of pairs) {
      expect(
        contrastRatio(COLORS[fg], COLORS[bg]),
        `${fg} on ${bg}`,
      ).toBeGreaterThanOrEqual(CONTRAST_TEXT_MIN);
    }
  });

  it("塗りボタンの文字は AA を満たす", () => {
    expect(
      contrastRatio(COLORS["--on-ink"], COLORS["--ink-fill"]),
    ).toBeGreaterThanOrEqual(CONTRAST_TEXT_MIN);
    expect(
      contrastRatio(COLORS["--on-ink"], COLORS["--danger-fill"]),
    ).toBeGreaterThanOrEqual(CONTRAST_TEXT_MIN);
  });

  it("ステージの文字は暗い面で AA を満たす", () => {
    const stageSurfaces = ["--stage-bg", "--stage-panel"] as const;
    const stageText = [
      "--stage-text",
      "--stage-text-2",
      "--stage-text-muted",
      "--stage-led",
      "--stage-amber",
      "--stage-danger",
    ] as const;

    for (const token of stageText) {
      for (const surface of stageSurfaces) {
        expect(
          contrastRatio(STAGE_COLORS[token], STAGE_COLORS[surface]),
          `${token} on ${surface}`,
        ).toBeGreaterThanOrEqual(CONTRAST_TEXT_MIN);
      }
    }
  });

  it("ステージの LED は図形の輪郭に対して 3:1 以上", () => {
    expect(
      contrastRatio(
        STAGE_COLORS["--stage-led"],
        STAGE_COLORS["--stage-line-strong"],
      ),
    ).toBeGreaterThanOrEqual(CONTRAST_UI_MIN);
  });
});

describe("タイポグラフィ（§5.2.2）", () => {
  it("8 段のスケールで、11px 未満を含まない", () => {
    expect(TYPE_SCALE).toHaveLength(8);
    for (const step of TYPE_SCALE) {
      expect(step.size).toBeGreaterThanOrEqual(11);
      expect([400, 500, 700]).toContain(step.weight);
      if (step.token === "display") {
        expect(step.weight).toBe(700);
      }
      if (step.token === "heading" || step.token === "ui") {
        expect(step.weight).toBe(500);
      }
    }
  });

  it("スケールが降順で、重複は meta と data の 12px だけ", () => {
    const sizes = TYPE_SCALE.map((step) => step.size);
    expect(sizes).toEqual([...sizes].sort((a, b) => b - a));
    // 12/18 は meta（Noto）と data（Hack）の 2 役で共有する（§5.2.2）。
    const duplicates = sizes.filter(
      (size, index) => sizes.indexOf(size) !== index,
    );
    expect(duplicates).toEqual([12]);
  });

  it("index.css の --text-* と --text-*--line-height が tokens.ts と一致する", () => {
    for (const step of TYPE_SCALE) {
      const size = cssVar(themeCss, `--text-${step.token}`);
      const lineHeight = cssVar(
        themeCss,
        `--text-${step.token}--line-height`,
      );
      expect(remToPx(size), `size ${step.token}`).toBe(step.size);
      expect(remToPx(lineHeight), `line-height ${step.token}`).toBe(
        step.lineHeight,
      );
    }
  });

  it("字間はデータと注記だけ開く", () => {
    expect(cssVar(themeCss, "--tracking-data")).toBe("0.01em");
    expect(cssVar(themeCss, "--tracking-caption")).toBe("0.02em");
  });
});

describe("余白・角丸・影", () => {
  it("余白は 4px グリッド", () => {
    for (const space of SPACE) {
      expect(space % 4).toBe(0);
    }
    expect(SPACE[0]).toBe(4);
    expect(SPACE.includes(24)).toBe(true);
  });

  it("角丸は 4 / 6 / 8 / 12 の 4 段", () => {
    expect(px(cssVar(themeCss, "--radius-xs"))).toBe(RADIUS.xs);
    expect(px(cssVar(themeCss, "--radius-sm"))).toBe(RADIUS.sm);
    expect(px(cssVar(themeCss, "--radius-md"))).toBe(RADIUS.md);
    expect(px(cssVar(themeCss, "--radius-lg"))).toBe(RADIUS.lg);
  });

  it("影はポップアップ用の 1 種だけ", () => {
    expect(cssVar(themeCss, "--shadow-pop").replace(/\s+/g, " ")).toBe(
      SHADOW_POP,
    );
    expect(themeCss.match(/--shadow-pop\s*:/g)).toHaveLength(1);
  });
});

describe("寸法（§5.3.2）", () => {
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

  it("1180px と 1100px のブレークポイントが一致する", () => {
    expect(remToPx(cssVar(themeCss, "--breakpoint-rail"))).toBe(
      LAYOUT.breakpointRail,
    );
    expect(remToPx(cssVar(themeCss, "--breakpoint-cols"))).toBe(
      LAYOUT.breakpointTableColumns,
    );
  });

  it("最小ウィンドウは 900×620", () => {
    expect(LAYOUT.windowMinWidth).toBe(900);
    expect(LAYOUT.windowMinHeight).toBe(620);
  });
});

describe("モーション（§5.7.1）", () => {
  it("90 / 160 / 240ms の 3 段", () => {
    expect(pxOrMs(cssVar(tokensCss, "--dur-1"))).toBe(MOTION.durations[0]);
    expect(pxOrMs(cssVar(tokensCss, "--dur-2"))).toBe(MOTION.durations[1]);
    expect(pxOrMs(cssVar(tokensCss, "--dur-3"))).toBe(MOTION.durations[2]);
  });

  it("曲線は 1 本だけ", () => {
    const ease = cssVar(themeCss, "--ease-standard").replace(/\s+/g, " ");
    expect(ease).toBe(MOTION.easeStandard);
  });

  it("prefers-reduced-motion で 0ms にする", () => {
    expect(tokensCss).toMatch(
      /@media\s*\(prefers-reduced-motion:\s*reduce\)[\s\S]*?--dur-1:\s*0ms/,
    );
  });
});

function pxOrMs(value: string): number {
  if (value.endsWith("ms")) {
    return Number.parseFloat(value);
  }
  throw new Error(`ms expected: ${value}`);
}
