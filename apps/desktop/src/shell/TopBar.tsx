import { BrandRing } from "../components";
import type { AppInfo } from "../lib/app-info";

export interface TopBarProps {
  appInfo: AppInfo | null;
}

/**
 * 上部バー 52px（§5.4.1）。状態はここに 1 つだけ置き、ナビや本文で繰り返さない。
 */
export function TopBar({ appInfo }: TopBarProps) {
  return (
    <header className="flex h-topbar shrink-0 items-center justify-between gap-4 border-b border-line bg-panel px-5">
      <div className="flex min-w-0 items-baseline gap-2.5">
        <BrandRing className="self-center text-accent" />
        <span className="text-ui font-medium whitespace-nowrap text-ink">
          Padbench
        </span>
        <span className="text-meta whitespace-nowrap text-muted">
          書き込み対応版
        </span>
      </div>

      <div className="flex min-w-0 items-center gap-4">
        <span className="inline-flex items-center gap-1.5 text-ui font-medium whitespace-nowrap text-ink">
          <span
            aria-hidden="true"
            className="size-2 rounded-full border border-muted"
          />
          未接続
        </span>
        <span
          data-testid="app-version"
          className="font-mono text-data tracking-data whitespace-nowrap text-muted"
        >
          {appInfo === null ? "v—" : `v${appInfo.version}`}
        </span>
      </div>
    </header>
  );
}
