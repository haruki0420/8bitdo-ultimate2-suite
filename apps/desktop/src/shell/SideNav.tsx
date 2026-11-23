import { cn } from "../lib/cn";
import type { ConnectionInfo } from "../lib/connection";
import { VIEWS, type ViewId } from "./views";

export interface SideNavProps {
  activeView: ViewId;
  onSelect: (view: ViewId) => void;
  onOpenLicenses: () => void;
  /** 接続中のデバイス。未接続は `null`（Rust 側の接続状態が正） */
  connection: ConnectionInfo | null;
}

/**
 * 左ナビ 216px（§5.4.1）。現在の画面は `--accent-soft` の地 + 左 2px のアクセント。
 * ナビ行は 34px、行間 2px。
 */
export function SideNav({
  activeView,
  onSelect,
  onOpenLicenses,
  connection,
}: SideNavProps) {
  return (
    <nav
      aria-label="画面"
      className="flex w-nav shrink-0 flex-col overflow-y-auto border-r border-line bg-sunken px-3 py-4"
    >
      <ul className="flex flex-col gap-0.5">
        {VIEWS.map((view) => {
          const active = view.id === activeView;

          return (
            <li key={view.id}>
              <button
                type="button"
                onClick={() => onSelect(view.id)}
                aria-current={active ? "page" : undefined}
                className={cn(
                  "flex min-h-nav-row w-full items-center rounded-sm px-2.5 text-left text-ui font-medium transition-colors duration-90 ease-standard focus-visible:shadow-focus focus-visible:outline-hidden",
                  active
                    ? "bg-accent-soft text-ink shadow-nav-current"
                    : "text-ink-2 hover:bg-hover hover:text-ink",
                )}
              >
                {view.label}
              </button>
            </li>
          );
        })}
      </ul>

      <div aria-hidden="true" className="mx-1.5 my-4 h-px bg-line" />

      {/* 接続状態は上部バーに 1 つだけ置く（§5.4.1）。ここは接続先の要約だけ。
       * 未接続ではデバイス名・アドレス・MTU を出さない。接続後は Rust が返す値だけを出す。 */}
      <p className="mb-1 px-2.5 text-meta font-medium text-muted">接続先</p>
      {connection === null ? (
        <p className="px-2.5 text-ui font-medium text-muted">未接続</p>
      ) : (
        <>
          <p className="truncate px-2.5 text-ui font-medium text-ink">
            {connection.name}
          </p>
          <p className="mt-0.5 truncate px-2.5 font-mono text-data tracking-data text-muted">
            {connection.address}
          </p>
          <p className="mt-0.5 px-2.5 text-meta text-muted">
            MTU {connection.mtu}
          </p>
        </>
      )}

      <div className="mt-auto pt-4">
        <button
          type="button"
          onClick={onOpenLicenses}
          className="flex min-h-nav-row w-full items-center rounded-sm px-2.5 text-left text-ui font-medium text-muted transition-colors duration-90 ease-standard hover:bg-hover hover:text-ink-2 focus-visible:shadow-focus focus-visible:outline-hidden"
        >
          ライセンス
        </button>
      </div>
    </nav>
  );
}
