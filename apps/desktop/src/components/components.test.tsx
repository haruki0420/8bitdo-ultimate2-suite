import { fireEvent, render, screen, within } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";

import {
  Banner,
  Button,
  Dialog,
  EmptyState,
  Field,
  Select,
  StatusPill,
  Switch,
  TabPanel,
  Table,
  Tabs,
  Toast,
} from "./index";

describe("Button", () => {
  it("無効の理由をボタンの直下に出し、押せなくする", () => {
    render(
      <Button disabled disabledReason="接続すると操作できます。">
        実機に書き込む
      </Button>,
    );

    const button = screen.getByRole("button", { name: "実機に書き込む" });
    const reason = screen.getByText("接続すると操作できます。");
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute("aria-describedby", reason.id);
    expect(
      button.compareDocumentPosition(reason) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("loading はラベルを変えず aria-busy を付ける", () => {
    render(<Button loading>書き込む</Button>);

    const button = screen.getByRole("button", { name: "書き込む" });
    expect(button).toHaveAttribute("aria-busy", "true");
    expect(button).toBeDisabled();
  });

  it("primary は signal-white の白ピル（9999px）", () => {
    render(<Button>接続する</Button>);
    const button = screen.getByRole("button", { name: "接続する" });
    expect(button.className).toContain("bg-signal-white");
    expect(button.className).toContain("text-obsidian");
    expect(button.className).toContain("rounded-full");
  });

  it("secondary は透明地 + 1px signal-white のゴースト（8px）", () => {
    render(<Button variant="secondary">再試行</Button>);
    const button = screen.getByRole("button", { name: "再試行" });
    expect(button.className).toContain("border-signal-white");
    expect(button.className).toContain("bg-transparent");
    expect(button.className).toContain("rounded-md");
    expect(button.className).not.toContain("rounded-full");
  });

  it("danger は 1px danger のゴーストで、文字は chalk（AA 優先）", () => {
    render(<Button variant="danger">復元する</Button>);
    const button = screen.getByRole("button", { name: "復元する" });
    expect(button.className).toContain("border-danger");
    expect(button.className).toContain("text-chalk");
    expect(button.className).not.toContain("text-danger");
  });

  it("押下で onClick を呼ぶ", () => {
    const onClick = vi.fn();
    render(<Button onClick={onClick}>読み込む</Button>);
    fireEvent.click(screen.getByRole("button", { name: "読み込む" }));
    expect(onClick).toHaveBeenCalledTimes(1);
  });
});

describe("StatusPill", () => {
  it("未接続はドットを出さず、mono のステータスピルで表す", () => {
    const { container } = render(<StatusPill label="未接続" />);
    const pill = screen.getByTestId("status-pill");
    expect(pill).toHaveTextContent("未接続");
    expect(pill.className).toContain("bg-paper-ink");
    expect(pill.className).toContain("border-graphite");
    expect(pill.className).toContain("rounded-xs");
    expect(container.querySelector('[data-testid="status-pill-dot"]')).toBeNull();
  });

  it("ライブ/接続中だけ pulse-green の 6px ドットを出す", () => {
    render(<StatusPill label="接続中" live />);
    const pill = screen.getByTestId("status-pill");
    expect(pill).toHaveAttribute("data-live", "true");
    const dot = screen.getByTestId("status-pill-dot");
    expect(dot.className).toContain("bg-pulse-green");
    expect(dot.className).toContain("size-1.5");
  });
});

describe("Field", () => {
  it("ラベルと結び、hint を aria-describedby で参照する", () => {
    render(
      <Field
        id="deadzone"
        label="内側デッドゾーン（0〜127）"
        hint="0〜127 の範囲で指定します。"
        defaultValue="12"
        inputMode="numeric"
      />,
    );

    const input = screen.getByLabelText("内側デッドゾーン（0〜127）");
    expect(input).toHaveValue("12");
    expect(input).toHaveAttribute("aria-describedby", "deadzone-hint");
    expect(screen.getByText("0〜127 の範囲で指定します。")).toHaveAttribute(
      "id",
      "deadzone-hint",
    );
    expect(input.className).toContain("border-graphite");
    expect(input.className).toContain("rounded-md");
    expect(input.className).toContain("focus:border-chalk");
  });

  it("error は aria-invalid と直下の文言で示す", () => {
    render(
      <Field
        id="trigger"
        label="トリガー（0〜1.0）"
        error="0〜1.0 の範囲で入力します。"
      />,
    );

    const input = screen.getByLabelText("トリガー（0〜1.0）");
    expect(input).toHaveAttribute("aria-invalid", "true");
    expect(input.className).toContain("aria-invalid:border-danger");
    expect(screen.getByRole("alert")).toHaveTextContent(
      "0〜1.0 の範囲で入力します。",
    );
  });
});

describe("Select", () => {
  it("ラベルと結び、選択肢を描画する", () => {
    render(
      <Select
        id="language"
        label="表示言語"
        options={[
          { value: "ja", label: "日本語" },
          { value: "en", label: "English" },
        ]}
      />,
    );

    const select = screen.getByRole("combobox", { name: "表示言語" });
    expect(
      within(select).getByRole("option", { name: "日本語" }),
    ).toBeInTheDocument();
    expect(
      within(select).getByRole("option", { name: "English" }),
    ).toBeInTheDocument();
    expect(select.className).toContain("bg-carbon");
  });
});

function ControlledSwitch() {
  const [checked, setChecked] = useState(false);
  return (
    <Switch
      id="gyro"
      label="ジャイロ"
      checked={checked}
      onCheckedChange={setChecked}
    />
  );
}

describe("Switch", () => {
  it("role=switch と aria-checked を持ち、押下で状態と文字が変わる", () => {
    render(<ControlledSwitch />);

    const toggle = screen.getByRole("switch", { name: "ジャイロ" });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(screen.getByText("オフ")).toBeInTheDocument();

    fireEvent.click(toggle);

    expect(toggle).toHaveAttribute("aria-checked", "true");
    expect(screen.getByText("オン")).toBeInTheDocument();
  });
});

describe("Tabs", () => {
  const items = [
    { id: "basic", label: "基本", panelId: "panel-basic" },
    { id: "buttons", label: "ボタン", panelId: "panel-buttons" },
  ] as const;

  it("role=tab と aria-selected を持ち、← → で移動する", () => {
    const onChange = vi.fn();
    render(
      <Tabs
        ariaLabel="設定の分類"
        items={items}
        selectedId="basic"
        onChange={onChange}
      />,
    );

    const tablist = screen.getByRole("tablist", { name: "設定の分類" });
    const basic = within(tablist).getByRole("tab", { name: "基本" });
    expect(basic).toHaveAttribute("aria-selected", "true");
    expect(basic).toHaveAttribute("tabindex", "0");
    expect(basic.className).toContain("text-chalk");
    expect(basic.className).toContain("after:bg-chalk");

    fireEvent.keyDown(basic, { key: "ArrowRight" });
    expect(onChange).toHaveBeenCalledWith("buttons");

    fireEvent.keyDown(basic, { key: "End" });
    expect(onChange).toHaveBeenLastCalledWith("buttons");
  });

  it("選択パネルを描画する", () => {
    render(
      <TabPanel id="panel-basic" labelledBy="tab-basic">
        パネル本文
      </TabPanel>,
    );

    expect(screen.getByRole("tabpanel")).toHaveTextContent("パネル本文");
  });
});

describe("Table", () => {
  it("ヘッダーと行を描画する（罫線のみ・行の塗りなし）", () => {
    render(
      <Table
        ariaLabel="接続情報"
        columns={[
          { key: "item", label: "項目" },
          { key: "value", label: "値", mono: true },
        ]}
        rows={[{ key: "mtu", cells: { item: "MTU", value: "200" } }]}
        empty={<span>接続すると値が入ります。</span>}
      />,
    );

    const table = screen.getByRole("table", { name: "接続情報" });
    expect(table).toBeInTheDocument();
    expect(
      screen.getByRole("columnheader", { name: "項目" }).className,
    ).toContain("font-mono");
    expect(screen.getByRole("cell", { name: "MTU" })).toBeInTheDocument();
    expect(screen.getByRole("cell", { name: "200" })).toBeInTheDocument();
  });

  it("読み込み中は骨組みを 3 行出し、aria-busy を付ける", () => {
    render(
      <Table
        ariaLabel="接続情報"
        columns={[{ key: "item", label: "項目" }]}
        rows={[]}
        empty={<span>接続すると値が入ります。</span>}
        loading
      />,
    );

    const table = screen.getByRole("table", { name: "接続情報" });
    expect(table).toHaveAttribute("aria-busy", "true");
    expect(screen.getByTestId("table-skeleton").children).toHaveLength(3);
    expect(
      screen.queryByText("接続すると値が入ります。"),
    ).not.toBeInTheDocument();
  });

  it("行が無いときは空状態を 1 行で出す", () => {
    render(
      <Table
        ariaLabel="接続情報"
        columns={[{ key: "item", label: "項目" }]}
        rows={[]}
        empty={<span>接続すると値が入ります。</span>}
      />,
    );

    expect(
      screen.getByText("接続すると値が入ります。"),
    ).toBeInTheDocument();
  });
});

describe("Dialog", () => {
  it("開くと見出しと本文を出し、閉じるボタンで onClose を呼ぶ", () => {
    const onClose = vi.fn();
    render(
      <Dialog
        open
        title="書き込み前の確認"
        onClose={onClose}
        footer={
          <Button variant="secondary" onClick={onClose}>
            キャンセル
          </Button>
        }
      >
        設定 3 件を実機に書き込みます。
      </Dialog>,
    );

    const dialog = screen.getByRole("dialog", { name: "書き込み前の確認" });
    expect(dialog).toHaveTextContent("設定 3 件を実機に書き込みます。");

    fireEvent.click(screen.getAllByRole("button", { name: "閉じる" })[0]);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("Esc（cancel）でも onClose を呼ぶ", () => {
    const onClose = vi.fn();
    render(
      <Dialog open title="確認" onClose={onClose}>
        本文
      </Dialog>,
    );

    fireEvent(screen.getByRole("dialog"), new Event("cancel"));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("閉じているときは描画しない", () => {
    render(
      <Dialog open={false} title="確認" onClose={vi.fn()}>
        本文
      </Dialog>,
    );

    expect(screen.queryByRole("dialog")).toBeNull();
  });
});

describe("Toast", () => {
  it("role=status と aria-live を持ち、閉じられる", () => {
    const onClose = vi.fn();
    render(
      <Toast
        tone="success"
        title="書き込みました"
        meta="読み戻して一致を確認しました"
        onClose={onClose}
      />,
    );

    const status = screen.getByRole("status");
    expect(status).toHaveAttribute("aria-live", "polite");
    expect(status).toHaveTextContent("書き込みました");
    expect(status.className).toContain("bg-carbon");
    expect(status.className).toContain("border-graphite");

    fireEvent.click(
      within(status).getByRole("button", { name: "通知を閉じる" }),
    );
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});

describe("Banner", () => {
  it("注意は status、危険は alert で出す（soft の塗りなし）", () => {
    const { rerender } = render(
      <Banner tone="warning" title="入力が止まっています">
        接続を確認してください。
      </Banner>,
    );

    const warning = screen.getByRole("status");
    expect(warning).toHaveTextContent("入力が止まっています");
    expect(warning.className).toContain("border-graphite");
    expect(warning.className).not.toContain("bg-warning-soft");

    rerender(
      <Banner tone="danger" title="接続が切れました。">
        再接続してください。
      </Banner>,
    );
    const danger = screen.getByRole("alert");
    expect(danger).toHaveTextContent("接続が切れました。");
    expect(danger.className).toContain("border-danger");
  });
});

describe("EmptyState", () => {
  it("状態・次にすること・行動 1 つを出す（左寄せ・見出し 23px）", () => {
    render(
      <EmptyState
        message="まだ接続していません。"
        instruction="コントローラを設定モードにして、「スキャン」を押します。"
        action={<Button>スキャン</Button>}
      />,
    );

    expect(
      screen.getByRole("heading", { level: 2, name: "まだ接続していません。" }),
    ).toHaveClass("text-heading");
    expect(screen.getByText("次にすること")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "スキャン" }),
    ).toBeInTheDocument();
  });
});
