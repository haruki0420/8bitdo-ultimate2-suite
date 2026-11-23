import { useEffect, useState } from "react";

import { cn } from "../lib/cn";
import { CloseIcon } from "./icons";

export type ToastTone = "success" | "warning" | "danger";

export interface ToastProps {
  tone: ToastTone;
  title: string;
  meta?: string;
  onClose: () => void;
  /** 表示時間。既定は 4 秒、危険は 8 秒 */
  duration?: number;
}

const TONE_LABEL: Record<ToastTone, string> = {
  success: "完了",
  warning: "注意",
  danger: "失敗",
};

/**
 * トースト（Imported style）: carbon 地 + 1px graphite 枠 + 8px 角丸。影なし。
 * 状態は mono のラベルと文言で表し、色付きの塗りは使わない。
 */
export function Toast({
  tone,
  title,
  meta,
  onClose,
  duration,
}: ToastProps) {
  const [paused, setPaused] = useState(false);
  const lifetime = duration ?? (tone === "danger" ? 8000 : 4000);

  useEffect(() => {
    if (paused) {
      return;
    }
    const timer = window.setTimeout(onClose, lifetime);
    return () => window.clearTimeout(timer);
  }, [paused, lifetime, onClose]);

  return (
    <div
      role="status"
      aria-live="polite"
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
      onFocus={() => setPaused(true)}
      onBlur={() => setPaused(false)}
      className="toast-enter pointer-events-auto flex w-80 items-start gap-3 rounded-md border border-graphite bg-carbon px-4 py-3"
    >
      <div className="min-w-0 flex-1">
        <p className="font-mono text-micro tracking-meta text-smoke uppercase">
          {TONE_LABEL[tone]}
        </p>
        <p className="mt-1 text-ui text-chalk">{title}</p>
        {meta !== undefined && (
          <p className="mt-0.5 font-mono text-micro text-smoke">{meta}</p>
        )}
      </div>
      <button
        type="button"
        onClick={onClose}
        aria-label="通知を閉じる"
        className={cn(
          "inline-flex size-8 shrink-0 items-center justify-center rounded-icon text-chalk transition-colors duration-90 ease-standard hover:bg-paper-ink",
        )}
      >
        <CloseIcon />
      </button>
    </div>
  );
}
