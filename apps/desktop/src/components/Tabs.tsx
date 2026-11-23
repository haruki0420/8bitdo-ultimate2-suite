import type { KeyboardEvent, ReactNode } from "react";

import { cn } from "../lib/cn";

export interface TabItem {
  id: string;
  label: string;
  /** 対応するパネルの id（`aria-controls`） */
  panelId: string;
}

export interface TabsProps {
  ariaLabel: string;
  items: readonly TabItem[];
  selectedId: string;
  onChange: (id: string) => void;
}

/**
 * タブ（Imported style）: 下線 1px だけ。選択時 chalk、非選択 transparent。塗りなし。
 * ← → Home End で移動する（roving tabindex）。
 */
export function Tabs({ ariaLabel, items, selectedId, onChange }: TabsProps) {
  const move = (event: KeyboardEvent<HTMLDivElement>) => {
    const index = items.findIndex((item) => item.id === selectedId);
    let nextIndex: number;

    switch (event.key) {
      case "ArrowRight":
        nextIndex = (index + 1) % items.length;
        break;
      case "ArrowLeft":
        nextIndex = (index - 1 + items.length) % items.length;
        break;
      case "Home":
        nextIndex = 0;
        break;
      case "End":
        nextIndex = items.length - 1;
        break;
      default:
        return;
    }

    event.preventDefault();
    const next = items[nextIndex];
    if (next === undefined) {
      return;
    }
    onChange(next.id);
    event.currentTarget
      .querySelector<HTMLElement>(`[data-tab-id="${next.id}"]`)
      ?.focus();
  };

  return (
    <div
      role="tablist"
      aria-label={ariaLabel}
      onKeyDown={move}
      className="flex gap-1 border-b border-graphite"
    >
      {items.map((item) => {
        const selected = item.id === selectedId;

        return (
          <button
            key={item.id}
            id={`tab-${item.id}`}
            data-tab-id={item.id}
            type="button"
            role="tab"
            aria-selected={selected}
            aria-controls={item.panelId}
            tabIndex={selected ? 0 : -1}
            onClick={() => onChange(item.id)}
            className={cn(
              "relative h-tab px-4 text-ui transition-colors duration-90 ease-standard",
              selected
                ? "text-chalk after:absolute after:inset-x-0 after:-bottom-px after:h-px after:bg-chalk"
                : "text-smoke hover:text-chalk",
            )}
          >
            {item.label}
          </button>
        );
      })}
    </div>
  );
}

export interface TabPanelProps {
  id: string;
  labelledBy: string;
  children: ReactNode;
}

export function TabPanel({ id, labelledBy, children }: TabPanelProps) {
  return (
    <div
      id={id}
      role="tabpanel"
      aria-labelledby={labelledBy}
      tabIndex={0}
      className="pt-6"
    >
      {children}
    </div>
  );
}
