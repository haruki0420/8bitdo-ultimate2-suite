import { Button } from "../components";

/**
 * 右レールの常設プレースホルダ「書き込み前の確認」。
 * 幅 344 で内側は約 254 px のため、1 行 20 文字前後で折り返さない短文にする。
 * カードは透明地 + 1px graphite + 8px 角丸。影は使わない。
 */
export function SafetyRail() {
  return (
    <section
      aria-labelledby="safety-rail-title"
      className="rounded-md border border-graphite p-6"
    >
      <div className="mb-3 flex items-center justify-between gap-2">
        <h2 id="safety-rail-title" className="text-subheading text-chalk">
          書き込み前の確認
        </h2>
        <span className="font-mono text-micro tracking-meta text-smoke uppercase">
          0 件
        </span>
      </div>

      <p className="text-body text-smoke">まだ変更はありません。</p>
      <p className="mt-4 font-mono text-micro tracking-meta text-smoke uppercase">
        次にすること
      </p>
      <ul className="mt-2 list-disc pl-4 text-meta text-smoke">
        <li>設定の変更は差分で表示します。</li>
        <li>接続後に書き込みできます。</li>
      </ul>

      <div className="mt-5">
        <Button
          variant="secondary"
          disabled
          disabledReason="接続すると書き込めます。"
        >
          実機に書き込む
        </Button>
      </div>

      {/* TODO(p3): 書き込み経路（pre-write backup + read-back）を実装したら、
          「書き込み前にバックアップします。」の 1 行をここへ戻す。 */}
    </section>
  );
}
