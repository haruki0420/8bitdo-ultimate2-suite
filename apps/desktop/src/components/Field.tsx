import type { InputHTMLAttributes } from "react";

import { cn } from "../lib/cn";

export interface FieldProps
  extends Omit<InputHTMLAttributes<HTMLInputElement>, "id"> {
  id: string;
  label: string;
  /** 範囲や単位など、例の値の代わりにラベルの下へ書く。 */
  hint?: string;
  error?: string;
}

/**
 * 入力 1 つ（Imported style）。透明地 + 1px graphite 枠 + 8px 角丸。
 * focus は 1px chalk の枠線に変える（影は使わない）。
 */
export function Field({
  id,
  label,
  hint,
  error,
  className,
  ...rest
}: FieldProps) {
  const describedBy =
    [
      hint === undefined ? null : `${id}-hint`,
      error === undefined ? null : `${id}-error`,
    ]
      .filter((value): value is string => value !== null)
      .join(" ") || undefined;

  return (
    <div className="flex min-w-0 flex-col gap-1">
      <label
        htmlFor={id}
        className="font-mono text-micro tracking-meta text-smoke uppercase"
      >
        {label}
      </label>
      <input
        id={id}
        aria-invalid={error === undefined ? undefined : true}
        aria-describedby={describedBy}
        className={cn(
          "h-control w-full rounded-md border border-graphite bg-transparent px-4 text-ui text-chalk transition-colors duration-90 ease-standard hover:border-iron focus:border-chalk focus:outline-hidden disabled:cursor-not-allowed disabled:text-smoke aria-invalid:border-danger",
          className,
        )}
        {...rest}
      />
      {hint !== undefined && (
        <p id={`${id}-hint`} className="text-meta text-smoke">
          {hint}
        </p>
      )}
      {error !== undefined && (
        <p id={`${id}-error`} role="alert" className="text-meta text-chalk">
          {error}
        </p>
      )}
    </div>
  );
}
