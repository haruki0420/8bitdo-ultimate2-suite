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
/** 現在位置ドットの半径（直径 6px = pulse-green のステータスドット）。 */
const DOT_RADIUS = 3;
/** 軌跡として残す過去位置の比率（中心 → 現在位置）。 */
const TRAIL_STEPS = [0.4, 0.6, 0.8] as const;
/** 軌跡の不透明度（中心側ほど淡い）。色は変えず、不透明度だけを変える。 */
const TRAIL_OPACITY = [0.18, 0.32, 0.48] as const;
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
 * スティック 1 本の図。外周 1px iron・中央十字 1px graphite・現在位置は
 * pulse-green の 6px ドット・軌跡は pulse-green の 1px 線（不透明度のみ変化）。
 * Y の符号は `screenOffset`（アダプタ層の `Y_SIGN` を反映）で 1 回だけ反転する。
 */
function StickPad({ label, vector }: StickPadProps) {
  const center = PAD_SIZE / 2;
  const dot = screenOffset(vector, STICK_DOT_RADIUS);
  const points = TRAIL_STEPS.map((step) =>
    screenOffset({ x: vector.x * step, y: vector.y * step }, STICK_DOT_RADIUS),
  ).map((point) => ({ x: center + point.x, y: center + point.y }));

  return (
    <figure
      data-testid="stick-pad"
      className="flex min-w-0 flex-col items-center gap-2"
    >
      <svg
        viewBox={`0 0 ${PAD_SIZE} ${PAD_SIZE}`}
        aria-hidden="true"
        className="block size-40"
      >
        {/* 外枠 1px iron */}
        <circle
          cx={center}
          cy={center}
          r={PAD_RADIUS}
          strokeWidth={1}
          className="fill-none stroke-iron"
        />
        {/* 中央十字 1px graphite */}
        <path
          d={`M${center - 12} ${center}H${center + 12}M${center} ${center - 12}V${center + 12}`}
          strokeWidth={1}
          className="stroke-graphite"
        />
        {/* 軌跡: pulse-green の 1px 線。色は変えず不透明度だけを変える */}
        {points.map((point, index) => {
          const previous = index === 0 ? { x: center, y: center } : points[index - 1];
          const opacity = TRAIL_OPACITY[index] ?? 0.48;
          return (
            <line
              key={`trail-${String(index)}`}
              x1={previous?.x ?? center}
              y1={previous?.y ?? center}
              x2={point.x}
              y2={point.y}
              strokeWidth={1}
              strokeOpacity={opacity}
              strokeLinecap="round"
              className="stroke-pulse-green"
            />
          );
        })}
        {/* 現在位置: pulse-green の 6px ドット */}
        <circle
          cx={center + dot.x}
          cy={center + dot.y}
          r={DOT_RADIUS}
          className="fill-pulse-green"
        />
      </svg>
      <figcaption className="font-mono text-micro tracking-meta text-smoke">
        {label}
      </figcaption>
      <p className="font-mono text-meta whitespace-nowrap text-chalk">
        X {formatSigned(vector.x)}
        <span className="px-1 text-smoke">/</span>
        Y {formatSigned(vector.y)}
      </p>
    </figure>
  );
}

function TriggerBar({ label, value }: { label: string; value: number }) {
  return (
    <div className="flex items-center gap-3">
      <dt className="w-6 font-mono text-micro tracking-meta text-smoke uppercase">
        {label}
      </dt>
      <dd className="flex flex-1 items-center gap-3">
        {/* 1px iron の枠 + chalk の塗りバー（高さ 4px） */}
        <span className="flex h-2.5 flex-1 items-center border border-iron px-px">
          <span
            className="block h-1 bg-chalk"
            style={{ width: `${Math.round(value * 100)}%` }}
          />
        </span>
        <span className="w-9 text-right font-mono text-meta text-chalk">
          {formatTrigger(value)}
        </span>
      </dd>
    </div>
  );
}

/**
 * 入力モニター画面。全体が暗いため「暗いステージ」は carbon 地 + 1px graphite の
 * パネルに置き換える（面の色替えはしない）。静止プレビューなので、図と数値は
 * 同じ正規化値から作る。
 */
export function InputMonitorScreen() {
  return (
    <div>
      <h1 className="text-display tracking-display text-balance text-chalk">
        入力モニター
      </h1>
      <p className="mt-3 mb-6 max-w-prose text-body text-smoke text-pretty">
        スティックとトリガーの状態を表示します。
      </p>

      <div className="mb-6">
        <Banner tone="warning" title="見本の値です">
          ステージの表示はダミーデータです。実機の入力は次のフェーズで表示します。
        </Banner>
      </div>

      <section aria-labelledby="input-stage-title">
        <h2 id="input-stage-title" className="sr-only">
          入力モニターのステージ
        </h2>
        <div className="mb-2 flex justify-end">
          <span className="font-mono text-micro tracking-meta whitespace-nowrap text-smoke">
            12 B / 0x0003
          </span>
        </div>

        <div
          data-testid="input-stage"
          className="rounded-md border border-graphite bg-carbon p-6 text-chalk"
        >
          <div className="mb-5 flex items-center justify-between gap-2">
            <span className="font-mono text-micro tracking-meta text-smoke">
              見本の値
            </span>
            <span className="font-mono text-micro tracking-meta text-smoke">
              受信していません
            </span>
          </div>

          <div className="mx-auto w-full max-w-lg">
            <div className="grid grid-cols-2 gap-6">
              <StickPad label="左スティック" vector={SAMPLE.left} />
              <StickPad label="右スティック" vector={SAMPLE.right} />
            </div>

            <dl className="mt-6 grid gap-3 border-t border-graphite pt-5">
              <TriggerBar label="L2" value={SAMPLE.leftTrigger} />
              <TriggerBar label="R2" value={SAMPLE.rightTrigger} />
              <div className="flex items-center justify-between gap-2">
                <dt className="font-mono text-micro tracking-meta text-smoke uppercase">
                  押下
                </dt>
                <dd className="flex flex-wrap justify-end gap-2">
                  {PRESSED.map((button) => (
                    <span
                      key={button}
                      className="inline-flex h-5 min-w-6 items-center justify-center rounded-xs border border-chalk px-2 font-mono text-micro text-chalk"
                    >
                      {button}
                    </span>
                  ))}
                </dd>
              </div>
            </dl>
          </div>
        </div>

        <div className="mt-2 flex justify-between gap-2 font-mono text-micro tracking-meta text-smoke">
          <span>画面表示では上が正（生値の向きは実機確認前）</span>
          <span>実機の入力は未接続</span>
        </div>
      </section>
    </div>
  );
}
