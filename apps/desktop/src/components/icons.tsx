import type { SVGProps } from "react";

import { cn } from "../lib/cn";

/*
 * アイコンはインライン SVG の線画だけを使う（絵文字・アイコンフォントは使わない）。
 * 16px・1.5 stroke に統一し、色は chalk か compass-gold だけを使う
 * （currentColor を継承し、smoke などの副次色はアイコンに使わない）。
 * ライセンス表記を増やさないため、アイコンセットは導入しない。
 */

const iconProps = {
  viewBox: "0 0 16 16",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.5,
  strokeLinecap: "round",
  strokeLinejoin: "round",
  "aria-hidden": true,
} as const;

export function ChevronDownIcon({
  className,
  ...props
}: SVGProps<SVGSVGElement>) {
  return (
    <svg
      {...iconProps}
      width={16}
      height={16}
      className={cn("size-4", className)}
      {...props}
    >
      <path d="M4 6l4 4 4-4" />
    </svg>
  );
}

export function CloseIcon({ className, ...props }: SVGProps<SVGSVGElement>) {
  return (
    <svg
      {...iconProps}
      width={16}
      height={16}
      className={cn("size-4", className)}
      {...props}
    >
      <path d="M4 4l8 8M12 4l-8 8" />
    </svg>
  );
}

export function CheckIcon({ className, ...props }: SVGProps<SVGSVGElement>) {
  return (
    <svg
      {...iconProps}
      width={16}
      height={16}
      className={cn("size-4", className)}
      {...props}
    >
      <path d="M3 8.5l3.2 3L13 4.5" />
    </svg>
  );
}

/** 製品マーク: 実機のリング LED を 1 本の輪で表す。色は compass-gold 専用。 */
export function BrandRing({ className, ...props }: SVGProps<SVGSVGElement>) {
  return (
    <svg
      {...iconProps}
      width={16}
      height={16}
      className={cn("size-4", className)}
      {...props}
    >
      <circle cx="8" cy="8" r="6" />
    </svg>
  );
}

/** 処理中のインラインスピナー。reduce-motion では静止した輪になる。 */
export function Spinner({ className, ...props }: SVGProps<SVGSVGElement>) {
  return (
    <svg
      viewBox="0 0 16 16"
      fill="none"
      aria-hidden="true"
      width={16}
      height={16}
      className={cn("size-4 animate-spin motion-reduce:animate-none", className)}
      {...props}
    >
      <circle
        cx="8"
        cy="8"
        r="6"
        stroke="currentColor"
        strokeOpacity={0.25}
        strokeWidth={1.5}
      />
      <path
        d="M8 2a6 6 0 0 1 6 6"
        stroke="currentColor"
        strokeWidth={1.5}
        strokeLinecap="round"
      />
    </svg>
  );
}
