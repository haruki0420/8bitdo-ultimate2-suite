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

const NAV_ROW =
  "relative flex min-h-nav-row w-full items-center px-3 text-left text-ui transition-colors duration-90 ease-standard";

/**
 * 左ナビ 216px（比率は現行維持）。背景は obsidian 一色、右境界は 1px graphite。
 * 現在の画面は文字色 chalk + 左 1px chalk のヘアラインで表す（塗りは使わない）。
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
      className="flex w-nav shrink-0 flex-col overflow-y-auto border-r border-graphite bg-obsidian px-3 py-4"
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
                  NAV_ROW,
                  active
                    ? "text-chalk after:absolute after:top-2 after:bottom-2 after:left-0 after:w-px after:bg-chalk"
                    : "text-smoke hover:text-chalk",
                )}
              >
                {view.label}
              </button>
            </li>
          );
        })}
      </ul>

      <div aria-hidden="true" className="mx-1.5 my-4 h-px bg-graphite" />

      {/* 接続状態は上部バーに 1 つだけ置く。ここは接続先の要約だけ。
       * 未接続ではデバイス名・アドレス・MTU を出さない。接続後は Rust が返す値だけを出す。 */}
      <p className="mb-1 px-3 font-mono text-micro tracking-meta text-smoke uppercase">
        接続先
      </p>
      {connection === null ? (
        <p className="px-3 text-ui text-smoke">未接続</p>
      ) : (
        <>
          <p className="truncate px-3 text-ui text-chalk">{connection.name}</p>
          <p className="mt-0.5 truncate px-3 font-mono text-meta text-smoke">
            {connection.address}
          </p>
          <p className="mt-0.5 px-3 font-mono text-meta text-smoke">
            MTU {connection.mtu}
          </p>
        </>
      )}

      <div className="mt-auto pt-4">
        <button
          type="button"
          onClick={onOpenLicenses}
          className={cn(NAV_ROW, "text-smoke hover:text-chalk")}
        >
          ライセンス
        </button>
      </div>
    </nav>
  );
}
