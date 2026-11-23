import { Button, EmptyState } from "../components";

/**
 * 右レールの常設プレースホルダ「書き込み前の確認」（§5.4.1）。
 * 書き込みが実装されるまでは、変更が無いことと、無効の理由だけを出す。
 */
export function SafetyRail() {
  return (
    <section
      aria-labelledby="safety-rail-title"
      className="rounded-lg border border-line bg-panel p-5"
    >
      <div className="mb-3 flex items-center justify-between gap-2">
        <h2
          id="safety-rail-title"
          className="text-heading font-medium text-ink"
        >
          書き込み前の確認
        </h2>
        <span className="inline-flex h-4.5 items-center rounded-xs bg-accent-soft px-1.5 text-caption text-accent">
          0 件
        </span>
      </div>

      <EmptyState
        message="まだ変更はありません。"
        instruction="設定画面で変更すると、現在値との差分がここに出ます。"
        action={
          <Button disabled disabledReason="接続して変更すると、ここから書き込めます。">
            実機に書き込む
          </Button>
        }
      />

      <p className="mt-4 border-t border-line pt-3 text-meta text-muted">
        書き込みの前にバックアップを保存します。
      </p>
    </section>
  );
}
