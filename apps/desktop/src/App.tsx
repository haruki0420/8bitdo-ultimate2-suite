import { useEffect, useState } from "react";

import { getAppInfo, type AppInfo } from "./lib/app-info";

interface NavItem {
  id: string;
  label: string;
  /** Views beyond the device placeholder are implemented by later phases. */
  available: boolean;
}

const NAV_ITEMS: readonly NavItem[] = [
  { id: "devices", label: "デバイス", available: true },
  { id: "input", label: "ライブ入力", available: false },
  { id: "config", label: "設定", available: false },
  { id: "macros", label: "マクロ", available: false },
  { id: "logs", label: "ログ", available: false },
];

const DEVICE_FACTS: readonly { label: string; value: string }[] = [
  { label: "トランスポート", value: "BLE / USB（未実装）" },
  { label: "ファームウェア", value: "—" },
  { label: "プロファイル", value: "—" },
];

function EmptyStateIcon() {
  return (
    <svg
      aria-hidden="true"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      className="h-6 w-6"
    >
      <path d="M6 8h12a3 3 0 0 1 3 3v5a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3v-5a3 3 0 0 1 3-3Z" />
      <path d="M8 12v3M6.5 13.5h3M16 13h.01M18 15h.01" />
    </svg>
  );
}

export default function App() {
  const [activeView, setActiveView] = useState<string>("devices");
  const [appInfo, setAppInfo] = useState<AppInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;

    getAppInfo()
      .then((info) => {
        if (!cancelled) {
          setAppInfo(info);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setError(
            "Rust バックエンド（app_info）に接続できませんでした。Tauri ランタイム外で実行している可能性があります。",
          );
        }
      });

    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <div className="flex h-screen flex-col overflow-hidden bg-slate-950 text-slate-200">
      <header className="flex h-14 shrink-0 items-center justify-between border-b border-slate-800 bg-slate-900/70 px-5">
        <div className="flex items-center gap-3">
          <div className="flex h-8 w-8 items-center justify-center rounded-md bg-emerald-500/15 text-sm font-bold text-emerald-300 ring-1 ring-emerald-500/30">
            P
          </div>
          <div className="leading-tight">
            <p className="text-sm font-semibold tracking-wide text-slate-100">
              Padbench
            </p>
            <p className="text-[11px] text-slate-500">Desktop</p>
          </div>
        </div>
        <div className="flex items-center gap-4 text-xs text-slate-400">
          <span className="inline-flex items-center gap-2 rounded-full border border-slate-700 px-3 py-1">
            <span
              aria-hidden="true"
              className="h-2 w-2 rounded-full bg-slate-500"
            />
            未接続
          </span>
          <span
            data-testid="app-version"
            className="font-mono text-slate-500"
          >
            {appInfo ? `v${appInfo.version}` : "v—"}
          </span>
        </div>
      </header>

      <div className="flex min-h-0 flex-1">
        <nav
          aria-label="メインナビゲーション"
          className="flex w-56 shrink-0 flex-col border-r border-slate-800 bg-slate-900/40 p-3"
        >
          <ul className="space-y-1">
            {NAV_ITEMS.map((item) => {
              const isActive = activeView === item.id;

              return (
                <li key={item.id}>
                  <button
                    type="button"
                    disabled={!item.available}
                    onClick={() => setActiveView(item.id)}
                    aria-current={isActive ? "page" : undefined}
                    className={[
                      "flex w-full items-center justify-between rounded-md px-3 py-2 text-left text-sm transition-colors",
                      isActive
                        ? "bg-slate-800 text-slate-100"
                        : item.available
                          ? "text-slate-300 hover:bg-slate-800/60"
                          : "cursor-not-allowed text-slate-600",
                    ].join(" ")}
                  >
                    <span>{item.label}</span>
                    {!item.available && (
                      <span className="text-[10px] uppercase tracking-wider">
                        準備中
                      </span>
                    )}
                  </button>
                </li>
              );
            })}
          </ul>

          <div className="mt-auto space-y-1 border-t border-slate-800 pt-3 text-[11px] text-slate-500">
            <p className="truncate">{appInfo?.name ?? "Padbench"}</p>
            <p className="font-mono">Tauri {appInfo?.tauriVersion ?? "—"}</p>
          </div>
        </nav>

        <main className="min-w-0 flex-1 overflow-y-auto p-8">
          <div className="mx-auto max-w-3xl">
            <h2 className="text-xl font-semibold text-slate-100">デバイス</h2>
            <p className="mt-1 text-sm text-slate-400">
              コントローラの接続状態をここに表示します。BLE
              接続と読み取りは次のフェーズで実装予定です。
            </p>

            {error !== null && (
              <div
                role="alert"
                className="mt-5 rounded-lg border border-amber-500/30 bg-amber-500/10 px-4 py-3 text-xs leading-relaxed text-amber-200"
              >
                {error}
              </div>
            )}

            <section className="mt-6 rounded-xl border border-dashed border-slate-800 bg-slate-900/30 px-6 py-12 text-center">
              <div className="mx-auto flex h-12 w-12 items-center justify-center rounded-full border border-slate-700 text-slate-500">
                <EmptyStateIcon />
              </div>
              <p className="mt-4 text-sm font-medium text-slate-300">
                デバイスは未接続です
              </p>
              <p className="mt-1 text-xs text-slate-500">
                8BitDo Ultimate 2（PID 0x600F / 0x6011）を検出すると、ここに状態が表示されます。
              </p>
            </section>

            <dl className="mt-6 grid gap-3 sm:grid-cols-3">
              {DEVICE_FACTS.map((fact) => (
                <div
                  key={fact.label}
                  className="rounded-lg border border-slate-800 bg-slate-900/40 px-4 py-3"
                >
                  <dt className="text-[11px] uppercase tracking-wider text-slate-500">
                    {fact.label}
                  </dt>
                  <dd className="mt-1 text-sm text-slate-300">{fact.value}</dd>
                </div>
              ))}
            </dl>
          </div>
        </main>
      </div>
    </div>
  );
}
