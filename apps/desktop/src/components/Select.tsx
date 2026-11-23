import type { SelectHTMLAttributes } from "react";

import { cn } from "../lib/cn";
import { ChevronDownIcon } from "./icons";

export interface SelectOption {
  value: string;
  label: string;
}

export interface SelectProps
  extends Omit<SelectHTMLAttributes<HTMLSelectElement>, "id" | "children"> {
  id: string;
  label: string;
  options: readonly SelectOption[];
  hint?: string;
}

/**
 * ネイティブ `<select>` を使い、矢印だけをインライン SVG にする（§5.5.4）。
 * 開いた一覧は OS のポップアップに任せ、自作リストボックスは作らない。
 */
export function Select({
  id,
  label,
  options,
  hint,
  className,
  ...rest
}: SelectProps) {
  return (
    <div className="flex min-w-0 flex-col gap-1">
      <label htmlFor={id} className="text-meta text-muted">
        {label}
      </label>
      <div className="relative">
        <select
          id={id}
          aria-describedby={hint === undefined ? undefined : `${id}-hint`}
          className={cn(
            "h-control w-full appearance-none rounded-sm border border-line-strong bg-panel pr-7 pl-2.5 text-ui text-ink transition-colors duration-90 ease-standard hover:bg-hover focus-visible:border-accent focus-visible:shadow-focus focus-visible:outline-hidden disabled:bg-sunken disabled:text-disabled",
            className,
          )}
          {...rest}
        >
          {options.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
        <ChevronDownIcon className="pointer-events-none absolute top-1/2 right-2 -translate-y-1/2 text-muted" />
      </div>
      {hint !== undefined && (
        <p id={`${id}-hint`} className="text-meta text-muted">
          {hint}
        </p>
      )}
    </div>
  );
}
