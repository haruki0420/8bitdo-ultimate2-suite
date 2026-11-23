import { useId, type ButtonHTMLAttributes, type ReactNode } from "react";

import { cn } from "../lib/cn";
import { Spinner } from "./icons";

/**
 * ボタンは 3 種だけ（Imported style）。
 * - primary: signal-white の白ピル（9999px）。1 画面に 1 つまで。
 * - secondary: 透明地 + 1px signal-white のゴースト。8px 角丸。
 * - danger: ゴースト + 1px danger の枠。文字は AA を満たす chalk
 *   （danger #B42318 は obsidian 上で 2.89:1 のため文字には使わない）。
 */
export type ButtonVariant = "primary" | "secondary" | "danger";
export type ButtonSize = "md" | "sm";

const VARIANT_CLASS: Record<ButtonVariant, string> = {
  primary:
    "border-transparent rounded-full bg-signal-white px-6 text-obsidian hover:bg-chalk active:bg-ash disabled:border-graphite disabled:bg-signal-white/10 disabled:text-smoke",
  secondary:
    "rounded-md border-signal-white bg-transparent px-5 text-signal-white hover:border-chalk hover:text-chalk active:bg-paper-ink disabled:border-graphite disabled:text-smoke",
  danger:
    "rounded-md border-danger bg-transparent px-5 text-chalk hover:border-chalk active:bg-paper-ink disabled:border-graphite disabled:text-smoke",
};

const SIZE_CLASS: Record<ButtonSize, string> = {
  md: "h-control",
  sm: "h-control-sm text-meta",
};

export interface ButtonProps
  extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children"> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  /** 処理中はラベルを変えず、スピナーと aria-busy だけを足す。 */
  loading?: boolean;
  /** 無効の理由をボタンの直下に 1 行で出す。押せると誤読させない。 */
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
        "inline-flex items-center justify-center gap-2 border text-ui font-normal whitespace-nowrap transition-colors duration-90 ease-standard disabled:cursor-not-allowed",
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
      <span id={reasonId} className="text-meta text-smoke">
        {disabledReason}
      </span>
    </span>
  );
}
