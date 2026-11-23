import { cn } from "../lib/cn";

export interface StatusPillProps {
  /** 状態名（「接続中」「未接続」）。日本語は大文字化しない。 */
  label: string;
  /**
   * ライブ / 接続中だけ true。pulse-green の 6px ドットは
   * 「ライブ/接続中」専用のため、未接続では出さない。
   */
  live?: boolean;
  className?: string;
}

/**
 * ステータスピル（Imported style）: `#1a1a1a` 地 + 1px graphite + 4px 角丸、
 * 8×14 padding、mono 12 大文字 smoke。live のときだけ pulse-green の 6px ドット。
 */
export function StatusPill({ label, live = false, className }: StatusPillProps) {
  return (
    <span
      data-testid="status-pill"
      data-live={live || undefined}
      className={cn(
        "inline-flex h-8 items-center gap-2 rounded-xs border border-graphite bg-paper-ink px-3.5 font-mono text-micro tracking-meta text-smoke uppercase",
        className,
      )}
    >
      {live && (
        <span
          aria-hidden="true"
          data-testid="status-pill-dot"
          className="size-1.5 shrink-0 rounded-full bg-pulse-green"
        />
      )}
      {label}
    </span>
  );
}
