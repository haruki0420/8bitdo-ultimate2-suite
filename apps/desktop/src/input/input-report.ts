/**
 * 入力レポート（cmd=0x0003、12 バイト）の正規化。
 *
 * 描画は SVG / 数値の両方がこの 1 か所の正規化値を使う。符号の反転もここで 1 回だけ
 * 行い、コンポーネントごとに反転しない（二重反転を防ぐ）。仕様は
 * docs/DESIGN_TAURI.md §5.6.1 / docs/PROTOCOL_SPEC.md §7.2。
 */

/**
 * 生値 Y の向き。中心値と `val − 127` だけが仕様で定義され、上方向が + か − かは
 * 実機で未確認のため、アダプタ層に定数として置く。
 *
 * TODO(input): 実機で 1 回だけスティックを上へ倒して確認し、この値を固定する
 * （docs/DESIGN_TAURI.md §5.6.1 / §9 の未決事項）。
 */
export const Y_SIGN: 1 | -1 = 1;

/** スティックの中心生値。 */
export const STICK_CENTER_RAW = 127;
/** スティックの可動範囲（中心から端まで）。 */
export const STICK_RANGE_RAW = 127;
/** トリガーの生値の最大。 */
export const TRIGGER_MAX_RAW = 255;

export interface StickVector {
  /** 右が +1（画面座標と同じ向き） */
  x: number;
  /** 上が +1（画面座標とは逆向き。描画時に反転する） */
  y: number;
}

/** -1〜+1 に丸める。 */
export function clampUnit(value: number): number {
  return Math.min(1, Math.max(-1, value));
}

/** スティック 1 軸の生値を -1〜+1 に正規化する。 */
export function normalizeAxis(raw: number): number {
  return clampUnit((raw - STICK_CENTER_RAW) / STICK_RANGE_RAW);
}

/** スティック 2 軸。Y は `Y_SIGN` を掛けた「上が正」の座標系で返す。 */
export function normalizeStick(rawX: number, rawY: number): StickVector {
  return { x: normalizeAxis(rawX), y: normalizeAxis(rawY) * Y_SIGN };
}

/** トリガー生値（0〜255）を 0〜1 に正規化する。 */
export function normalizeTrigger(raw: number): number {
  return Math.min(1, Math.max(0, raw / TRIGGER_MAX_RAW));
}

/**
 * 正規化したスティック値を、中心からの SVG オフセットに変換する。
 * SVG も画面と同じく下が +Y なので、ここで 1 回だけ符号を反転する。
 */
export function screenOffset(vector: StickVector, radius: number): StickVector {
  // `-0` を残さないよう、0 はそのまま 0 にする。
  const y = vector.y === 0 ? 0 : -vector.y * radius;
  return { x: vector.x * radius, y };
}

/** 符号付きの比率を `+0.42` / `−0.31` の形にする（マイナスは U+2212）。 */
export function formatSigned(value: number): string {
  const fixed = value.toFixed(2);
  return fixed.startsWith("-") ? `−${fixed.slice(1)}` : `+${fixed}`;
}

/** トリガーを `0.62` の形にする。 */
export function formatTrigger(value: number): string {
  return value.toFixed(2);
}
