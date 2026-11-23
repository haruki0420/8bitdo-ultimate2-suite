import { cn } from "../lib/cn";

export interface SwitchProps {
  id: string;
  label: string;
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  disabled?: boolean;
}

/**
 * スイッチ 36×20・つまみ 16。塗りは使わず、1px 枠とつまみの位置で状態を表す。
 * 色だけに頼らないよう、右に「オン」「オフ」を mono で出す。
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
    <div className="flex min-h-control items-center gap-3">
      <button
        id={id}
        type="button"
        role="switch"
        aria-checked={checked}
        aria-labelledby={labelId}
        disabled={disabled}
        onClick={() => onCheckedChange(!checked)}
        className={cn(
          "relative h-5 w-9 shrink-0 rounded-full border transition-colors duration-160 ease-standard disabled:cursor-not-allowed",
          checked ? "border-chalk" : "border-iron",
        )}
      >
        <span
          aria-hidden="true"
          className={cn(
            "absolute top-0.5 left-0.5 size-4 rounded-full transition-transform duration-160 ease-standard",
            checked ? "translate-x-4 bg-chalk" : "bg-iron",
          )}
        />
      </button>
      <span id={labelId} className="text-ui text-smoke">
        {label}
      </span>
      <span className="font-mono text-micro tracking-meta text-smoke uppercase">
        {checked ? "オン" : "オフ"}
      </span>
    </div>
  );
}
