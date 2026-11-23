import { Banner } from "../components";
import {
  formatSigned,
  formatTrigger,
  normalizeStick,
  normalizeTrigger,
  screenOffset,
  type StickVector,
} from "../input/input-report";

/** スティック図の一辺（SVG viewBox）。 */
const PAD_SIZE = 160;
/** 外周円の半径。 */
const PAD_RADIUS = 79;
/** 現在位置ドットが動ける半径（外周と重ならない値）。 */
const STICK_DOT_RADIUS = 72;
/** 現在位置ドットの半径。 */
const DOT_RADIUS = 5;
/** 軌跡として残す過去位置の比率（中心 → 現在位置）。 */
const TRAIL_STEPS = [0.4, 0.6, 0.8] as const;
/** 押下中のボタン（見本）。 */
const PRESSED = ["A", "L1"] as const;

/**
 * 画面の見本データ。実機の入力は次のフェーズで接続する。
 * 左スティックは右上、右スティックは左下、L2 は 62 % の想定。
 */
const SAMPLE = {
  left: normalizeStick(180, 140),
  right: normalizeStick(117, 88),
  leftTrigger: normalizeTrigger(158),
  rightTrigger: normalizeTrigger(31),
} as const;

interface StickPadProps {
  label: string;
  vector: StickVector;
}

/**
 * スティック 1 本の図。外周・中央十字・現在位置ドット・軌跡だけで組み、
 * コントローラの線画や装飾（グロー多重・影）は置かない。
 * Y の符号は `screenOffset`（アダプタ層の `Y_SIGN` を反映）で 1 回だけ反転する。
 */
function StickPad({ label, vector }: StickPadProps) {
  const center = PAD_SIZE / 2;
  const dot = screenOffset(vector, STICK_DOT_RADIUS);
  const trail = TRAIL_STEPS.map((step) =>
    screenOffset(
      { x: vector.x * step, y: vector.y * step },
      STICK_DOT_RADIUS,
    ),
  )
    .map((point) => `${center + point.x},${center + point.y}`)
    .join(" ");

  return (
    <figure
      data-testid="stick-pad"
      className="flex min-w-0 flex-col items-center gap-1.5"
    >
      <svg
        viewBox={`0 0 ${PAD_SIZE} ${PAD_SIZE}`}
        aria-hidden="true"
        className="block size-40"
      >
        {/* 外枠 */}
        <circle
          cx={center}
          cy={center}
          r={PAD_RADIUS}
          strokeWidth={1.5}
          className="fill-none stroke-stage-line-strong"
        />
        {/* 中央十字 */}
        <path
          d={`M${center - 12} ${center}H${center + 12}M${center} ${center - 12}V${center + 12}`}
          strokeWidth={1}
          className="stroke-stage-line"
        />
        {/* 軌跡（中心 → 現在位置） */}
        <polyline
          points={`${center},${center} ${trail}`}
          strokeWidth={1.5}
          strokeOpacity={0.3}
          strokeLinecap="round"
          strokeLinejoin="round"
          className="fill-none stroke-stage-led"
        />
        {/* 現在位置 */}
        <circle
          cx={center + dot.x}
          cy={center + dot.y}
          r={DOT_RADIUS}
          className="fill-stage-led"
        />
      </svg>
      <figcaption className="text-caption font-medium text-stage-text-2">
        {label}
      </figcaption>
      <p className="font-mono text-data tracking-data whitespace-nowrap text-stage-text">
        X {formatSigned(vector.x)}
        <span className="px-1 text-stage-line-strong">/</span>
        Y {formatSigned(vector.y)}
      </p>
    </figure>
  );
}

function TriggerBar({ label, value }: { label: string; value: number }) {
  return (
    <div className="flex items-center gap-2">
      <dt className="w-6 text-caption text-stage-text-muted">{label}</dt>
      <dd className="flex flex-1 items-center gap-2">
        <span className="h-1.5 flex-1 overflow-hidden rounded-full bg-stage-line">
          <span
            className="block h-full rounded-full bg-stage-led"
            style={{ width: `${Math.round(value * 100)}%` }}
          />
        </span>
        <span className="w-9 text-right font-mono text-data tracking-data text-stage-text">
          {formatTrigger(value)}
        </span>
      </dd>
    </div>
  );
}

/**
 * 入力モニター画面。ダークステージの見本（スティック図 2 つ・トリガーバー 2 本・
 * 押下ボタンのチップ）。静止プレビューなので、図と数値は同じ正規化値から作る（§5.6.1）。
 */
export function InputMonitorScreen() {
  return (
    <div>
      <h1 className="text-display font-bold text-balance text-ink">
        入力モニター
      </h1>
      <p className="mt-2 mb-5 max-w-prose text-body text-ink-2 text-pretty">
        スティックとトリガーの状態を表示します。
      </p>

      <div className="mb-5">
        <Banner tone="warning" title="見本の値です">
          ステージの表示はダミーデータです。実機の入力は次のフェーズで表示します。
        </Banner>
      </div>

      <section aria-labelledby="input-stage-title">
        <h2 id="input-stage-title" className="sr-only">
          入力モニターのステージ
        </h2>
        <div className="mb-2 flex justify-end">
          <span className="font-mono text-data tracking-data whitespace-nowrap text-muted">
            12 B / 0x0003
          </span>
        </div>

        <div
          data-surface="stage"
          className="rounded-lg border border-stage-line bg-stage-bg p-5 text-stage-text"
        >
          <div className="mb-4 flex items-center justify-between gap-2">
            <span className="inline-flex items-center gap-2 text-caption font-medium text-stage-text-2">
              <span aria-hidden="true" className="size-2 rounded-full bg-stage-led" />
              見本の値
            </span>
            <span className="font-mono text-caption tracking-caption text-stage-text-muted">
              受信していません
            </span>
          </div>

          <div className="mx-auto w-full max-w-lg">
            <div className="grid grid-cols-2 gap-4">
              <StickPad label="左スティック" vector={SAMPLE.left} />
              <StickPad label="右スティック" vector={SAMPLE.right} />
            </div>

            <dl className="mt-4 grid gap-2 border-t border-stage-line pt-4">
              <TriggerBar label="L2" value={SAMPLE.leftTrigger} />
              <TriggerBar label="R2" value={SAMPLE.rightTrigger} />
              <div className="flex items-center justify-between gap-2">
                <dt className="text-caption text-stage-text-muted">押下</dt>
                <dd className="flex flex-wrap justify-end gap-1.5">
                  {PRESSED.map((button) => (
                    <span
                      key={button}
                      className="inline-flex h-5 min-w-6 items-center justify-center rounded-xs border border-stage-led px-1.5 font-mono text-caption text-stage-led"
                    >
                      {button}
                    </span>
                  ))}
                </dd>
              </div>
            </dl>
          </div>
        </div>

        <div className="mt-2 flex justify-between gap-2 text-meta text-muted">
          <span>Y 軸は上が正</span>
          <span>実機の入力は未接続</span>
        </div>
      </section>
    </div>
  );
}
