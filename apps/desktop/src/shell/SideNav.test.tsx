import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { SideNav } from "./SideNav";

const BASE_PROPS = {
  activeView: "devices" as const,
  onSelect: vi.fn(),
  onOpenLicenses: vi.fn(),
};

describe("SideNav の接続先", () => {
  it("未接続では「未接続」だけを出し、名前・アドレス・MTU を出さない", () => {
    render(<SideNav {...BASE_PROPS} connection={null} />);

    expect(screen.getByText("接続先")).toBeInTheDocument();
    expect(screen.getByText("未接続")).toBeInTheDocument();
    expect(screen.queryByText(/MTU/)).toBeNull();
    expect(screen.queryByText(/:/)).toBeNull();
  });

  it("接続後は Rust が返した名前・アドレス・MTU を出す", () => {
    render(
      <SideNav
        {...BASE_PROPS}
        connection={{
          name: "Test Pad",
          address: "AA:BB:CC:DD:EE:FF",
          mtu: 200,
        }}
      />,
    );

    expect(screen.getByText("Test Pad")).toBeInTheDocument();
    expect(screen.getByText("AA:BB:CC:DD:EE:FF")).toBeInTheDocument();
    expect(screen.getByText("MTU 200")).toBeInTheDocument();
    expect(screen.queryByText("未接続")).toBeNull();
  });
});
