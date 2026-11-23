/**
 * 共通コンポーネント（§5.5）。
 *
 * 状態の網羅（docs/DESIGN_TAURI.md §5.5）:
 * - default / hover / active / focus-visible / disabled: 操作部品すべて
 *   （Button / Field / Select / Switch / Tabs / ナビ行。無効は理由を添える）
 * - loading: Button（スピナー + aria-busy、ラベルは変えない）、
 *   Table（骨組み 3 行 + aria-busy）
 * - error: Field（aria-invalid + 直下の文言）、Banner（danger）
 * - success: Toast（success の状態バー）、Banner の完了表示
 */

export { Button, type ButtonProps, type ButtonSize, type ButtonVariant } from "./Button";
export { Field, type FieldProps } from "./Field";
export { Select, type SelectOption, type SelectProps } from "./Select";
export { Switch, type SwitchProps } from "./Switch";
export { TabPanel, Tabs, type TabItem, type TabPanelProps, type TabsProps } from "./Tabs";
export { Table, type TableColumn, type TableProps, type TableRow } from "./Table";
export { Dialog, type DialogProps } from "./Dialog";
export { Toast, type ToastProps, type ToastTone } from "./Toast";
export { Banner, type BannerProps, type BannerTone } from "./Banner";
export { EmptyState, type EmptyStateProps } from "./EmptyState";
export {
  BrandRing,
  CheckIcon,
  ChevronDownIcon,
  CloseIcon,
  Spinner,
} from "./icons";
