import { useCallback, useEffect, useState } from "react";

import { Button, Dialog } from "./components";
import { getAppInfo, type AppInfo } from "./lib/app-info";
import { DevicesScreen } from "./screens/DevicesScreen";
import { InputMonitorScreen } from "./screens/InputMonitorScreen";
import { PlaceholderScreen } from "./screens/PlaceholderScreen";
import { SafetyRail } from "./shell/SafetyRail";
import { SideNav } from "./shell/SideNav";
import { TopBar } from "./shell/TopBar";
import type { ViewId } from "./shell/views";

type PlaceholderView = Exclude<ViewId, "devices" | "monitor">;

const PLACEHOLDERS: Record<
  PlaceholderView,
  { title: string; description: string }
> = {
  current: {
    title: "現在値",
    description: "接続したデバイスの現在値を読み込みます。",
  },
  settings: {
    title: "設定",
    description: "プロファイル・スティック・振動などを変更します。",
  },
  macros: {
    title: "マクロ",
    description: "マクロのステップを編集し、記録します。",
  },
  logs: {
    title: "ログ",
    description: "接続や書き込みの結果がここに残ります。",
  },
};

function Screen({
  view,
  ipcError,
  onOpenDevices,
}: {
  view: ViewId;
  ipcError: string | null;
  onOpenDevices: () => void;
}) {
  if (view === "devices") {
    return <DevicesScreen ipcError={ipcError} />;
  }

  if (view === "monitor") {
    return <InputMonitorScreen />;
  }

  const placeholder = PLACEHOLDERS[view];

  return (
    <PlaceholderScreen
      title={placeholder.title}
      description={placeholder.description}
      onOpenDevices={onOpenDevices}
    />
  );
}

export default function App() {
  const [activeView, setActiveView] = useState<ViewId>("devices");
  const [appInfo, setAppInfo] = useState<AppInfo | null>(null);
  const [ipcError, setIpcError] = useState<string | null>(null);
  const [licensesOpen, setLicensesOpen] = useState(false);

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
          setIpcError(
            "Tauri ランタイム外で実行している可能性があります。Tauri から起動すると表示されます。",
          );
        }
      });

    return () => {
      cancelled = true;
    };
  }, []);

  const openDevices = useCallback(() => {
    setActiveView("devices");
  }, []);

  const closeLicenses = useCallback(() => {
    setLicensesOpen(false);
  }, []);

  return (
    <div className="relative flex h-dvh flex-col overflow-hidden bg-obsidian text-smoke">
      <a
        href="#main-content"
        className="sr-only focus:not-sr-only focus:absolute focus:top-2 focus:left-2 focus:z-50 focus:rounded-md focus:border focus:border-chalk focus:bg-carbon focus:px-4 focus:py-2 focus:text-ui focus:text-chalk"
      >
        本文へスキップ
      </a>

      <TopBar appInfo={appInfo} />

      <div className="flex min-h-0 flex-1">
        {/* 接続後は Rust が返す ConnectionInfo を渡す。BLE 接続は未実装なので未接続。 */}
        <SideNav
          activeView={activeView}
          onSelect={setActiveView}
          onOpenLicenses={() => setLicensesOpen(true)}
          connection={null}
        />

        {/* 1180px 未満では右レールをコンテンツの下へ回す */}
        <div className="flex min-w-0 flex-1 flex-col overflow-y-auto rail:flex-row rail:overflow-hidden">
          <main
            id="main-content"
            tabIndex={-1}
            className="min-w-0 flex-1 px-6 py-6 focus-visible:outline-hidden rail:overflow-y-auto"
          >
            <div className="max-w-content wide:mx-auto">
              <Screen
                view={activeView}
                ipcError={ipcError}
                onOpenDevices={openDevices}
              />
            </div>
          </main>

          <aside
            aria-label="書き込み前の確認"
            className="shrink-0 border-t border-graphite px-6 py-6 rail:w-rail rail:border-t-0 rail:border-l rail:overflow-y-auto"
          >
            <SafetyRail />
          </aside>
        </div>
      </div>

      <Dialog
        open={licensesOpen}
        onClose={closeLicenses}
        title="ライセンス"
        footer={
          <Button onClick={closeLicenses}>閉じる</Button>
        }
      >
        <p>アプリのライセンスは MIT または Apache-2.0 です。</p>
        <p className="mt-2">
          依存クレートとフォントのライセンス全文の表示は P6 で追加します（egui 版
          #24 と同水準）。
        </p>
        <p className="mt-2">
          フォントはまだ同梱しておらず、OS のフォントで表示しています。
        </p>
        <p className="mt-3 font-mono text-meta text-smoke">MIT OR Apache-2.0</p>
      </Dialog>
    </div>
  );
}
