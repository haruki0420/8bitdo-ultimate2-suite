import { cn } from "../lib/cn";

export interface SwitchProps {
  id: string;
  label: string;
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  disabled?: boolean;
}

/**
 * スイッチ 36×20・つまみ 16（§5.5.3）。
 * 色だけで状態を伝えないよう、右に「オン」「オフ」を文字で出す。
 */
export function Switch({
  id,
  label,
  checked,
  onCheckedChange,
  disabled = false,
}: SwitchProps) {
  const labelId = `${id}-label`;

  return (
    <div className="flex min-h-control items-center gap-2">
      <button
        id={id}
        type="button"
        role="switch"
        aria-checked={checked}
        aria-labelledby={labelId}
        disabled={disabled}
        onClick={() => onCheckedChange(!checked)}
        className={cn(
          "relative h-5 w-9 shrink-0 rounded-full border transition-colors duration-160 ease-standard focus-visible:shadow-focus focus-visible:outline-hidden disabled:cursor-not-allowed",
          checked
            ? "border-accent bg-accent"
            : "border-line-strong bg-sunken",
        )}
      >
        <span
          aria-hidden="true"
          className={cn(
            "absolute top-px left-px size-4 rounded-full border border-line bg-panel transition-transform duration-160 ease-standard",
            checked && "translate-x-4",
          )}
        />
      </button>
      <span id={labelId} className="text-ui text-ink-2">
        {label}
      </span>
      <span className="text-meta text-muted">{checked ? "オン" : "オフ"}</span>
    </div>
  );
}
