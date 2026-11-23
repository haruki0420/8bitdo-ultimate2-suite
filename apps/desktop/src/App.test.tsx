import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import App from "./App";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

describe("App", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue({
      name: "Padbench",
      version: "0.1.0",
      tauriVersion: "2.0.0",
    });
  });

  it("app_info のバージョンを表示し、デバイスのプレースホルダを描画する", async () => {
    render(<App />);

    expect(
      screen.getByRole("heading", { name: "デバイス" }),
    ).toBeInTheDocument();
    expect(screen.getAllByText("Padbench").length).toBeGreaterThan(0);
    const version = await screen.findByTestId("app-version");
    expect(version).toHaveTextContent("v0.1.0");
    expect(invokeMock).toHaveBeenCalledWith("app_info");
    expect(
      screen.getByText("デバイスは未接続です"),
    ).toBeInTheDocument();
  });

  it("app_info が失敗した場合は警告を表示する", async () => {
    invokeMock.mockRejectedValue(new Error("ipc unavailable"));

    render(<App />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Rust バックエンド",
    );
  });
});
