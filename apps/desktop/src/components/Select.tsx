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
 * ネイティブ `<select>`。透明地 + 1px graphite 枠 + 8px 角丸、focus で 1px chalk。
 * 開いた一覧は OS のポップアップに任せ、色は明示して文字が消えないようにする。
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
      <label
        htmlFor={id}
        className="font-mono text-micro tracking-meta text-smoke uppercase"
      >
        {label}
      </label>
      <div className="relative">
        <select
          id={id}
          aria-describedby={hint === undefined ? undefined : `${id}-hint`}
          className={cn(
            "h-control w-full appearance-none rounded-md border border-graphite bg-carbon pr-9 pl-4 text-ui text-chalk transition-colors duration-90 ease-standard hover:border-iron focus:border-chalk focus:outline-hidden disabled:cursor-not-allowed disabled:text-smoke",
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
        <ChevronDownIcon className="pointer-events-none absolute top-1/2 right-3 -translate-y-1/2 text-chalk" />
      </div>
      {hint !== undefined && (
        <p id={`${id}-hint`} className="text-meta text-smoke">
          {hint}
        </p>
      )}
    </div>
  );
}
