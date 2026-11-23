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
 * タブ（§5.5.5）。選択下線は 2px の要素で表現し、レイアウトを動かさない。
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
      className="flex gap-1 border-b border-line"
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
              "relative h-tab rounded-t-sm px-3 text-ui font-medium transition-colors duration-90 ease-standard hover:bg-hover hover:text-ink-2 focus-visible:shadow-focus focus-visible:outline-hidden",
              selected
                ? "text-ink after:absolute after:inset-x-0 after:-bottom-px after:h-0.5 after:bg-accent"
                : "text-muted",
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
      className="pt-4"
    >
      {children}
    </div>
  );
}
