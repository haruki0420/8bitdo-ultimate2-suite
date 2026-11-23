import type { ReactNode } from "react";

import { cn } from "../lib/cn";

export type BannerTone = "warning" | "danger";

export interface BannerProps {
  tone: BannerTone;
  title: string;
  children?: ReactNode;
  /** 行動ボタンは 1 つだけ */
  action?: ReactNode;
}

const TONE_CLASS: Record<BannerTone, string> = {
  warning: "border-graphite",
  danger: "border-danger",
};

/**
 * バナー（Imported style）: 透明地 + 1px 枠。soft の塗りは使わない。
 * 文字は chalk / smoke で AA を満たし、危険の区別は 1px danger 枠と文言で行う。
 */
export function Banner({ tone, title, children, action }: BannerProps) {
  return (
    <div
      role={tone === "danger" ? "alert" : "status"}
      className={cn(
        "flex items-start justify-between gap-4 rounded-md border px-5 py-4",
        TONE_CLASS[tone],
      )}
    >
      <div className="min-w-0 flex-1">
        <p className="text-ui text-chalk">{title}</p>
        {children !== undefined && (
          <div className="mt-1 text-meta text-smoke">{children}</div>
        )}
      </div>
      {action}
    </div>
  );
}
