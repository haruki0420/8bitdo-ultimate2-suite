/** 条件付きクラス名の最小ヘルパー（依存追加を避けるため clsx 相当だけを持つ）。 */
export function cn(
  ...classes: Array<string | false | null | undefined>
): string {
  return classes.filter(Boolean).join(" ");
}
