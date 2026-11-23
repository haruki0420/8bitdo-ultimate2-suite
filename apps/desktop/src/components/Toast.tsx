import { useEffect, useState } from "react";

import { cn } from "../lib/cn";
import { CloseIcon } from "./icons";

export type ToastTone = "success" | "warning" | "danger";

export interface ToastProps {
  tone: ToastTone;
  title: string;
  meta?: string;
  onClose: () => void;
  /** 表示時間。既定は 4 秒、危険は 8 秒（§5.5.8） */
  duration?: number;
}

const BAR_CLASS: Record<ToastTone, string> = {
  success: "bg-success",
  warning: "bg-warning",
  danger: "bg-danger",
};

/**
 * トースト（§5.5.8）。句点は付けない 1 文の通知。
 * hover / focus 中は自動で閉じない。
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
      className="toast-enter pointer-events-auto flex w-80 items-stretch gap-2 rounded-md border border-line bg-panel px-3 py-2.5 shadow-pop"
    >
      <span
        aria-hidden="true"
        className={cn("w-bar shrink-0 rounded-full", BAR_CLASS[tone])}
      />
      <div className="min-w-0 flex-1">
        <p className="text-ui font-medium text-ink">{title}</p>
        {meta !== undefined && (
          <p className="mt-0.5 text-meta text-muted">{meta}</p>
        )}
      </div>
      <button
        type="button"
        onClick={onClose}
        aria-label="通知を閉じる"
        className="inline-flex size-8 shrink-0 items-center justify-center rounded-sm text-muted transition-colors duration-90 ease-standard hover:bg-hover hover:text-ink focus-visible:shadow-focus focus-visible:outline-hidden"
      >
        <CloseIcon />
      </button>
    </div>
  );
}
