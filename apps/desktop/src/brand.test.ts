import { describe, expect, it } from "vitest";

/**
 * アプリ名は 3 か所に書かれている（Rust の `APP_NAME`、`tauri.conf.json` の
 * `productName` とウィンドウタイトル、上部バーのワードマーク）。コードから
 * 単一ソースにできないため、少なくとも一致をテストで固定する。
 */
const RAW = import.meta.glob(
  [
    "../src-tauri/tauri.conf.json",
    "../src-tauri/src/lib.rs",
    "./shell/TopBar.tsx",
  ],
  { query: "?raw", import: "default", eager: true },
) as Record<string, string>;

function rawSource(key: string): string {
  const value = RAW[key];
  if (typeof value !== "string") {
    throw new Error(`unreadable source: ${key}`);
  }
  return value;
}

const CONF = JSON.parse(rawSource("../src-tauri/tauri.conf.json")) as {
  productName: string;
  app: { windows: { title: string }[] };
};

const RUST_APP_NAME =
  /const APP_NAME: &str = "([^"]+)";/.exec(rawSource("../src-tauri/src/lib.rs"))?.[1];
const TOP_BAR = rawSource("./shell/TopBar.tsx");

describe("アプリ名の三重管理", () => {
  it("productName / APP_NAME / 上部バーが同じ", () => {
    expect(RUST_APP_NAME).toBeDefined();
    expect(CONF.productName).toBe(RUST_APP_NAME);
    // ワードマークは `{APP_NAME}</span>` の形で置く。
    expect(TOP_BAR).toContain(`>${CONF.productName}</span>`);
  });

  it("ウィンドウタイトルも productName と一致する", () => {
    expect(CONF.app.windows).toHaveLength(1);
    expect(CONF.app.windows[0]?.title).toBe(CONF.productName);
  });

  it("上部バーは未実装の能力を名乗らない", () => {
    // 書き込み対応版の副題は、能力フラグが入るまで表示しない（TODO コメントのみ）。
    expect(TOP_BAR).not.toMatch(/>\s*書き込み対応版\s*</);
  });
});
