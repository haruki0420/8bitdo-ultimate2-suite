import { useId, type ButtonHTMLAttributes, type ReactNode } from "react";

import { cn } from "../lib/cn";
import { Spinner } from "./icons";

export type ButtonVariant = "primary" | "secondary" | "danger" | "danger-solid";
export type ButtonSize = "md" | "sm";

/** 主キーは墨の塗り。danger-solid は確認ダイアログの最終ボタンだけに使う（§5.5.1）。 */
const VARIANT_CLASS: Record<ButtonVariant, string> = {
  primary:
    "border-transparent bg-ink-fill text-on-ink hover:bg-ink-hover active:bg-ink-active",
  secondary:
    "border-line-strong bg-panel text-ink hover:bg-hover active:bg-active",
  danger:
    "border-danger bg-panel text-danger hover:bg-danger-soft active:bg-danger-soft",
  "danger-solid":
    "border-danger-fill bg-danger-fill text-on-ink hover:bg-danger-fill-hover",
};

const SIZE_CLASS: Record<ButtonSize, string> = {
  md: "h-control px-3.5",
  sm: "h-control-sm px-2.5 text-meta",
};

export interface ButtonProps
  extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children"> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  /** 処理中はラベルを変えず、スピナーと aria-busy だけを足す。 */
  loading?: boolean;
  /** 無効の理由をボタンの直下に 1 行で出す。押せると誤読させない（§5.5.1）。 */
  disabledReason?: string;
  children: ReactNode;
}

export function Button({
  variant = "primary",
  size = "md",
  loading = false,
  disabledReason,
  className,
  children,
  disabled = false,
  type = "button",
  ...rest
}: ButtonProps) {
  const reasonId = useId();
  const isDisabled = disabled || loading;

  const button = (
    <button
      type={type}
      disabled={isDisabled}
      aria-busy={loading || undefined}
      aria-describedby={disabledReason === undefined ? undefined : reasonId}
      className={cn(
        "inline-flex items-center justify-center gap-1.5 rounded-sm border font-medium whitespace-nowrap transition-colors duration-90 ease-standard focus-visible:shadow-focus focus-visible:outline-hidden disabled:cursor-not-allowed disabled:border-transparent disabled:bg-sunken disabled:text-disabled",
        VARIANT_CLASS[variant],
        SIZE_CLASS[size],
        className,
      )}
      {...rest}
    >
      {loading && <Spinner className="size-3" />}
      {children}
    </button>
  );

  if (disabledReason === undefined) {
    return button;
  }

  return (
    <span className="inline-flex flex-col items-start gap-1">
      {button}
      <span id={reasonId} className="text-meta text-muted">
        {disabledReason}
      </span>
    </span>
  );
}
