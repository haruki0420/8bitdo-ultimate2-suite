import { describe, expect, it } from "vitest";

import { VOCAB } from "./design/tokens";
import { VIEWS } from "./shell/views";

// Node の fs を使わず、Vite の ?raw で実ファイルをそのまま読む
// （tsc の型チェックに @types/node を足さないため）。
const RAW_SOURCES = import.meta.glob(["./**/*.{ts,tsx,css}", "../index.html"], {
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

const INDEX_HTML = rawSource("../index.html");

/** `src/` 配下の実装ファイル（テスト自身は除く）。 */
const SOURCE_TEXT = Object.keys(RAW_SOURCES)
  .filter(
    (path) => path !== "../index.html" && !/\.test\.tsx?$/.test(path),
  )
  .map((path) => `/* ${path} */\n${rawSource(path)}`)
  .join("\n");

const BANNED_TERMS = [
  "ライブ入力",
  "設定値",
  "録音",
  "スナップショット",
  "次の一手",
  "書き込み対応ビルド",
  "書込み",
  "リトライ",
] as const;

/** 絵文字・記号フォントのアイコンを使わない（インライン SVG の線画だけ）。 */
const EMOJI_PATTERN =
  /[\u{1f000}-\u{1faff}\u{2600}-\u{27bf}\u{fe0f}\u{1f1e6}-\u{1f1ff}]/u;

describe("日本語の用語（§6.3）", () => {
  it("使わない表記が実装に残っていない", () => {
    for (const term of BANNED_TERMS) {
      expect(SOURCE_TEXT, `banned term: ${term}`).not.toContain(term);
      expect(INDEX_HTML, `banned term in index.html: ${term}`).not.toContain(
        term,
      );
    }
  });

  it("採用表記が使われている", () => {
    expect(SOURCE_TEXT).toContain(VOCAB.inputMonitor);
    expect(SOURCE_TEXT).toContain(VOCAB.currentValues);
    expect(SOURCE_TEXT).toContain(VOCAB.record);
    expect(SOURCE_TEXT).toContain(VOCAB.writeCapable);
    expect(SOURCE_TEXT).toContain(VOCAB.nextAction);
  });

  it("左ナビの並びが仕様どおり", () => {
    expect(VIEWS.map((view) => view.label)).toEqual([
      "デバイス",
      VOCAB.inputMonitor,
      VOCAB.currentValues,
      "設定",
      "マクロ",
      "ログ",
    ]);
  });
});

describe("絵文字禁止", () => {
  it("実装ファイルに絵文字が無い", () => {
    const match = EMOJI_PATTERN.exec(SOURCE_TEXT);
    expect(match?.[0] ?? null, `emoji found: ${match?.[0]}`).toBeNull();
  });

  it("index.html に絵文字が無い", () => {
    expect(EMOJI_PATTERN.test(INDEX_HTML)).toBe(false);
  });
});
