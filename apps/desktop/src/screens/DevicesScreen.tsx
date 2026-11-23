import { Banner, Button, EmptyState, Table } from "../components";

/** 未接続の値を表すダッシュ。 */
const NO_VALUE = "—";

interface ConnectionRow {
  key: string;
  label: string;
  note: string;
}

const CONNECTION_ROWS: readonly ConnectionRow[] = [
  { key: "device", label: "デバイス", note: "BLE / USB" },
  { key: "address", label: "アドレス", note: "設定モード" },
  { key: "firmware", label: "ファームウェア", note: NO_VALUE },
  { key: "mtu", label: "MTU", note: NO_VALUE },
  { key: "battery", label: "バッテリー生値", note: "% 未換算" },
  { key: "rate", label: "受信レート", note: "20 ms 間隔" },
];

export interface DevicesScreenProps {
  /** Rust バックエンド（app_info）に届かなかったときの 1 文 */
  ipcError: string | null;
}

/**
 * デバイス画面（未接続）。空状態 + スキャン / 接続の操作 + 接続情報の表。
 * 主アクション（白ピル）は 1 画面に 1 つだけ置く。
 */
export function DevicesScreen({ ipcError }: DevicesScreenProps) {
  return (
    <div>
      <h1 className="text-display tracking-display text-balance text-chalk">
        デバイス
      </h1>
      <p className="mt-3 mb-6 max-w-prose text-body text-smoke text-pretty">
        コントローラを設定モードにして接続します。接続すると現在値を読み込みます。
      </p>

      {ipcError !== null && (
        <div className="mb-6">
          <Banner tone="danger" title="Rust バックエンドに接続できませんでした。">
            {ipcError}
          </Banner>
        </div>
      )}

      <EmptyState
        message="まだ接続していません。"
        instruction="コントローラを設定モードにしてから接続します。"
        action={
          <Button
            disabled
            disabledReason="スキャンは次のフェーズで追加します。"
          >
            スキャン
          </Button>
        }
      />

      <section className="mt-10" aria-labelledby="connection-info-title">
        <div className="mb-3 flex flex-wrap items-start justify-between gap-3">
          <h2 id="connection-info-title" className="text-subheading text-chalk">
            接続情報
          </h2>
          <Button
            variant="secondary"
            size="sm"
            disabled
            disabledReason="先にスキャンでデバイスを選びます。"
          >
            接続する
          </Button>
        </div>

        <Table
          ariaLabel="接続情報"
          columns={[
            { key: "item", label: "項目" },
            { key: "value", label: "値", mono: true },
            { key: "note", label: "補足", lowPriority: true },
          ]}
          rows={CONNECTION_ROWS.map((row) => ({
            key: row.key,
            cells: {
              item: row.label,
              value: <span className="font-mono text-meta text-ash">{NO_VALUE}</span>,
              note: <span className="font-mono text-micro text-smoke">{row.note}</span>,
            },
          }))}
          empty={<span>接続すると値が入ります。</span>}
        />

        <p className="mt-3 text-meta text-smoke">
          接続すると現在値（設定 2768 バイト）を読み込みます。
        </p>
      </section>
    </div>
  );
}
