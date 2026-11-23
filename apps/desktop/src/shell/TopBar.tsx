import { BrandRing, StatusPill } from "../components";
import type { AppInfo } from "../lib/app-info";

export interface TopBarProps {
  appInfo: AppInfo | null;
}

/**
 * 上部バー 52px。背景は obsidian 一色、境界は 1px graphite。
 * 状態はここに 1 つだけ置き、ナビや本文で繰り返さない。
 */
export function TopBar({ appInfo }: TopBarProps) {
  return (
    <header className="flex h-topbar shrink-0 items-center justify-between gap-4 border-b border-graphite bg-obsidian px-6">
      <div className="flex min-w-0 items-baseline gap-3">
        <BrandRing className="self-center text-compass-gold" />
        <span className="text-body whitespace-nowrap text-chalk">Padbench</span>
        {/* TODO(p6): 書き込み機能を載せた配布物だけ、副題に
            「書き込み対応版」（VOCAB.writeCapable）を出す。現状は表示しない。 */}
      </div>

      <div className="flex min-w-0 items-center gap-4">
        <StatusPill label="未接続" />
        <span
          data-testid="app-version"
          className="font-mono text-micro tracking-meta whitespace-nowrap text-smoke"
        >
          {appInfo === null ? "v—" : `v${appInfo.version}`}
        </span>
      </div>
    </header>
  );
}
