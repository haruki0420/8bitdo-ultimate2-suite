import { describe, expect, it } from "vitest";

import {
  Y_SIGN,
  clampUnit,
  formatSigned,
  formatTrigger,
  normalizeAxis,
  normalizeStick,
  normalizeTrigger,
  screenOffset,
} from "./input-report";

describe("スティックの正規化（§5.6.1）", () => {
  it("中心は 0、端は ±1、範囲外は丸める", () => {
    expect(normalizeAxis(127)).toBe(0);
    expect(normalizeAxis(0)).toBe(-1);
    expect(normalizeAxis(254)).toBeCloseTo(1, 5);
    expect(normalizeAxis(300)).toBe(1);
    expect(normalizeAxis(-100)).toBe(-1);
    expect(clampUnit(2)).toBe(1);
    expect(clampUnit(-2)).toBe(-1);
  });

  it("Y は上が正の座標系で、Y_SIGN の適用は 1 回だけ", () => {
    // 実機で生値の向きを確認するまで符号は固定しない（TODO(input) 参照）。
    // 値そのものではなく「Y_SIGN を 1 回だけ掛ける」ことだけを固定する。
    const rawY = 140;
    const vector = normalizeStick(127, rawY);
    expect(vector.y).toBeCloseTo(normalizeAxis(rawY) * Y_SIGN, 10);
    expect(Math.sign(vector.y)).toBe(Math.sign(normalizeAxis(rawY)) * Y_SIGN);
  });

  it("ダミー値が表示用の数値になる（+0.42 / +0.10 など）", () => {
    const left = normalizeStick(180, 140);
    expect(left.x).toBeCloseTo(0.417, 3);
    expect(left.y).toBeCloseTo(0.102, 3);
    expect(formatSigned(left.x)).toBe("+0.42");
    expect(formatSigned(left.y)).toBe("+0.10");

    const right = normalizeStick(117, 88);
    expect(formatSigned(right.x)).toBe("−0.08");
    expect(formatSigned(right.y)).toBe("−0.31");
  });

  it("画面座標では Y を 1 回だけ反転する（上が画面の上）", () => {
    expect(screenOffset({ x: 0, y: 1 }, 12)).toEqual({ x: 0, y: -12 });
    expect(screenOffset({ x: 0, y: -1 }, 12)).toEqual({ x: 0, y: 12 });
    expect(screenOffset({ x: 1, y: 0 }, 12)).toEqual({ x: 12, y: 0 });
  });
});

describe("トリガーの正規化", () => {
  it("0〜255 を 0〜1 に写す", () => {
    expect(normalizeTrigger(0)).toBe(0);
    expect(normalizeTrigger(255)).toBe(1);
    expect(normalizeTrigger(158)).toBeCloseTo(0.62, 2);
    expect(formatTrigger(normalizeTrigger(158))).toBe("0.62");
    expect(formatTrigger(normalizeTrigger(31))).toBe("0.12");
  });

  it("範囲外は丸める", () => {
    expect(normalizeTrigger(-10)).toBe(0);
    expect(normalizeTrigger(999)).toBe(1);
  });
});
