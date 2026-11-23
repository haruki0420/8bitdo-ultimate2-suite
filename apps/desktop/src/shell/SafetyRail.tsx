import { Button } from "../components";

/**
 * 右レールの常設プレースホルダ「書き込み前の確認」（§5.4.1）。
 * 幅 344 で内側は約 254 px のため、1 行 20 文字前後で折り返さない短文にする。
 * 空状態は余白を 4px グリッド（8 / 12 / 16）で詰め、縦に間延びさせない。
 */
export function SafetyRail() {
  return (
    <section
      aria-labelledby="safety-rail-title"
      className="rounded-lg border border-line bg-panel p-5"
    >
      <div className="mb-2 flex items-center justify-between gap-2">
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

      <p className="text-body text-ink-2">まだ変更はありません。</p>
      <p className="mt-2 text-meta font-medium text-muted">次にすること</p>
      <ul className="mt-1 list-disc pl-4 text-meta text-muted">
        <li>設定の変更は差分で表示します。</li>
        <li>接続後に書き込みできます。</li>
      </ul>

      <div className="mt-3">
        <Button disabled disabledReason="接続すると書き込めます。">
          実機に書き込む
        </Button>
      </div>

      <p className="mt-3 border-t border-line pt-3 text-meta text-muted">
        書き込み前にバックアップします。
      </p>
    </section>
  );
}
