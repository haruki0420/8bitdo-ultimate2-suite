import { useEffect, useId, useRef, type ReactNode } from "react";

import { CloseIcon } from "./icons";

export interface DialogProps {
  open: boolean;
  title: string;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
  /** 幅 480（md） / 確認用 560（lg）（§5.5.7） */
  size?: "md" | "lg";
}

/**
 * ネイティブ `<dialog>` + `showModal()` を使う（§5.5.7）。
 * トップレイヤー・Esc・フォーカスの閉じ込めはブラウザに任せ、追加依存を増やさない。
 */
export function Dialog({
  open,
  title,
  onClose,
  children,
  footer,
  size = "md",
}: DialogProps) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  const previousFocus = useRef<HTMLElement | null>(null);

  useEffect(() => {
    if (!open) {
      return;
    }

    const dialog = dialogRef.current;
    if (dialog === null) {
      return;
    }

    previousFocus.current =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;

    if (typeof dialog.showModal === "function") {
      if (!dialog.open) {
        dialog.showModal();
      }
    } else {
      // jsdom など showModal が無い環境では open 属性で代替する。
      dialog.setAttribute("open", "");
    }

    const handleCancel = (event: Event) => {
      event.preventDefault();
      onClose();
    };
    dialog.addEventListener("cancel", handleCancel);

    return () => {
      dialog.removeEventListener("cancel", handleCancel);
      if (typeof dialog.close === "function" && dialog.open) {
        dialog.close();
      }
      previousFocus.current?.focus();
    };
  }, [open, onClose]);

  if (!open) {
    return null;
  }

  return (
    <dialog
      ref={dialogRef}
      className="app-dialog"
      data-size={size}
      aria-labelledby={titleId}
    >
      <div className="dialog-enter flex max-h-full flex-col overflow-hidden rounded-lg border border-line bg-panel shadow-pop">
        <header className="flex items-start justify-between gap-4 px-6 pt-6 pb-2">
          <h2 id={titleId} className="text-title font-bold text-ink">
            {title}
          </h2>
          <button
            type="button"
            onClick={onClose}
            aria-label="閉じる"
            className="inline-flex size-8 shrink-0 items-center justify-center rounded-sm text-muted transition-colors duration-90 ease-standard hover:bg-hover hover:text-ink focus-visible:shadow-focus focus-visible:outline-hidden"
          >
            <CloseIcon />
          </button>
        </header>
        <div className="min-h-0 flex-1 overflow-y-auto px-6 pb-4 text-body text-ink-2">
          {children}
        </div>
        {footer !== undefined && (
          <footer className="flex justify-end gap-2 px-6 py-4">
            {footer}
          </footer>
        )}
      </div>
    </dialog>
  );
}
