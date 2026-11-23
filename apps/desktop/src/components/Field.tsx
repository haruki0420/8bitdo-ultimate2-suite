import type { InputHTMLAttributes } from "react";

import { cn } from "../lib/cn";

export interface FieldProps
  extends Omit<InputHTMLAttributes<HTMLInputElement>, "id"> {
  id: string;
  label: string;
  /** 範囲や単位など、例の値の代わりにラベルの下へ書く（§5.5.2）。 */
  hint?: string;
  error?: string;
}

/** 入力（テキスト・数値）1 つ。ラベルは必ず `<label for>` で結ぶ。 */
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
      <label htmlFor={id} className="text-meta text-muted">
        {label}
      </label>
      <input
        id={id}
        aria-invalid={error === undefined ? undefined : true}
        aria-describedby={describedBy}
        className={cn(
          "h-control w-full rounded-sm border border-line-strong bg-panel px-2.5 text-ui text-ink transition-colors duration-90 ease-standard hover:bg-hover focus-visible:border-accent focus-visible:shadow-focus focus-visible:outline-hidden disabled:bg-sunken disabled:text-disabled aria-invalid:border-danger",
          className,
        )}
        {...rest}
      />
      {hint !== undefined && (
        <p id={`${id}-hint`} className="text-meta text-muted">
          {hint}
        </p>
      )}
      {error !== undefined && (
        <p id={`${id}-error`} role="alert" className="text-meta text-danger">
          {error}
        </p>
      )}
    </div>
  );
}
