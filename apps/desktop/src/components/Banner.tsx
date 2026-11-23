import type { ReactNode } from "react";

import { cn } from "../lib/cn";

export type BannerTone = "warning" | "danger";

export interface BannerProps {
  tone: BannerTone;
  title: string;
  children?: ReactNode;
  /** 行動ボタンは 1 つだけ（§5.5.9） */
  action?: ReactNode;
}

const TONE_CLASS: Record<BannerTone, { box: string; bar: string }> = {
  warning: { box: "bg-warning-soft text-warning", bar: "bg-warning" },
  danger: { box: "bg-danger-soft text-danger", bar: "bg-danger" },
};

/** バナー（§5.5.9）。左 3px の状態バー + soft の地 + 状態色の文字。 */
export function Banner({ tone, title, children, action }: BannerProps) {
  const toneClass = TONE_CLASS[tone];

  return (
    <div
      role={tone === "danger" ? "alert" : "status"}
      className={cn(
        "flex items-stretch gap-2 rounded-sm px-3 py-2.5",
        toneClass.box,
      )}
    >
      <span
        aria-hidden="true"
        className={cn("w-bar shrink-0 rounded-full", toneClass.bar)}
      />
      <div className="min-w-0 flex-1">
        <p className="text-ui font-medium">{title}</p>
        {children !== undefined && (
          <div className="mt-0.5 text-meta">{children}</div>
        )}
      </div>
      {action}
    </div>
  );
}
