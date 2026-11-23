import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import App from "./App";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

const NAV_LABELS = [
  "デバイス",
  "入力モニター",
  "現在値",
  "設定",
  "マクロ",
  "ログ",
] as const;

const EMOJI_PATTERN =
  /[\u{1f000}-\u{1faff}\u{2600}-\u{27bf}\u{fe0f}\u{1f1e6}-\u{1f1ff}]/u;

function nav() {
  return screen.getByRole("navigation", { name: "画面" });
}

function openView(label: string) {
  fireEvent.click(within(nav()).getByRole("button", { name: label }));
}

describe("App", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue({
      name: "Padbench",
      version: "0.1.0",
      tauriVersion: "2.0.0",
    });
  });

  it("上部バー・左ナビ・右レールの骨格を描画する", async () => {
    render(<App />);

    expect(screen.getByRole("banner")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "本文へスキップ" })).toHaveAttribute(
      "href",
      "#main-content",
    );
    expect(nav()).toBeInTheDocument();
    for (const label of NAV_LABELS) {
      expect(
        within(nav()).getByRole("button", { name: label }),
      ).toBeInTheDocument();
    }

    const version = await screen.findByTestId("app-version");
    expect(version).toHaveTextContent("v0.1.0");
    expect(invokeMock).toHaveBeenCalledWith("app_info");

    expect(
      screen.getByRole("complementary", { name: "書き込み前の確認" }),
    ).toBeInTheDocument();
    expect(
      within(nav()).getByRole("button", { name: "デバイス" }),
    ).toHaveAttribute("aria-current", "page");

    expect(document.body.textContent ?? "").not.toMatch(EMOJI_PATTERN);
  });

  it("デバイス画面に空状態・操作・接続情報の表を出す", () => {
    render(<App />);

    expect(
      screen.getByRole("heading", { level: 1, name: "デバイス" }),
    ).toBeInTheDocument();
    expect(screen.getByText("まだ接続していません。")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "スキャン" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "接続する" })).toBeDisabled();
    expect(screen.getByText("先にデバイスをスキャンして選びます。")).toBeInTheDocument();

    const table = screen.getByRole("table", { name: "接続情報" });
    expect(
      within(table).getByRole("columnheader", { name: "項目" }),
    ).toBeInTheDocument();
    expect(
      within(table).getByRole("cell", { name: "デバイス" }),
    ).toBeInTheDocument();
    expect(
      within(table).getByRole("cell", { name: "バッテリー生値" }),
    ).toBeInTheDocument();
  });

  it("入力モニター画面にダークステージの見本を出す", () => {
    render(<App />);
    openView("入力モニター");

    expect(
      screen.getByRole("heading", { level: 1, name: "入力モニター" }),
    ).toBeInTheDocument();

    const stage = document.querySelector('[data-surface="stage"]');
    expect(stage).not.toBeNull();
    expect(stage).toHaveAttribute("data-surface", "stage");

    expect(screen.getByText("見本の値です")).toBeInTheDocument();
    expect(screen.getByText("0.62")).toBeInTheDocument();
    expect(screen.getByText("0.12")).toBeInTheDocument();
    expect(screen.getByText(/−0\.31/)).toBeInTheDocument();
    expect(screen.getByText("Y 軸は上が正")).toBeInTheDocument();
  });

  it("スキャンは未実装を伝えるトーストを出し、閉じられる", () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "スキャン" }));

    const toast = screen.getByRole("status");
    expect(toast).toHaveTextContent("スキャンはまだ実装していません");

    fireEvent.click(
      within(toast).getByRole("button", { name: "通知を閉じる" }),
    );
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("準備中の画面からデバイスへ戻れる", () => {
    render(<App />);
    openView("現在値");

    expect(
      screen.getByRole("heading", { level: 1, name: "現在値" }),
    ).toBeInTheDocument();
    expect(screen.getByText("この画面はまだ準備中です。")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "デバイスを開く" }));
    expect(
      screen.getByRole("heading", { level: 1, name: "デバイス" }),
    ).toBeInTheDocument();
  });

  it("ライセンスのダイアログを開いて閉じられる", () => {
    render(<App />);
    fireEvent.click(within(nav()).getByRole("button", { name: "ライセンス" }));

    const dialog = screen.getByRole("dialog", { name: "ライセンス" });
    expect(dialog).toBeInTheDocument();
    expect(within(dialog).getByText("MIT OR Apache-2.0")).toBeInTheDocument();

    const closeButtons = within(dialog).getAllByRole("button", {
      name: "閉じる",
    });
    const footerClose = closeButtons.at(-1);
    if (footerClose === undefined) {
      throw new Error("閉じるボタンが見つかりません");
    }
    fireEvent.click(footerClose);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("app_info が失敗したら警告を出す", async () => {
    invokeMock.mockRejectedValue(new Error("ipc unavailable"));

    render(<App />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Tauri ランタイム外",
    );
  });
});
