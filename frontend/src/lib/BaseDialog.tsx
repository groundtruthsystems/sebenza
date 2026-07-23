import { useEffect, useRef, type MouseEvent, type ReactNode } from "react";

export default function BaseDialog({
  onclose,
  wide = false,
  maxWidth = "",
  className = "",
  children,
}: {
  onclose: () => void;
  wide?: boolean;
  maxWidth?: string;
  className?: string;
  children: ReactNode;
}) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const pressStartedOnBackdrop = useRef(false);

  useEffect(() => {
    dialogRef.current?.showModal();
  }, []);

  const widthClass = maxWidth ? "" : wide ? "max-w-[560px]" : "max-w-[380px]";

  return (
    <dialog
      ref={dialogRef}
      onClose={onclose}
      onMouseDown={(e: MouseEvent) => {
        pressStartedOnBackdrop.current = e.target === dialogRef.current;
      }}
      onClick={(e: MouseEvent) => {
        if (e.target === dialogRef.current && pressStartedOnBackdrop.current) {
          dialogRef.current?.close();
        }
        pressStartedOnBackdrop.current = false;
      }}
      className={`bg-sidebar text-primary border border-edge rounded-xl w-[90%] ${widthClass} ${className}`}
      style={maxWidth ? { maxWidth } : undefined}
    >
      <div className="p-6">{children}</div>
    </dialog>
  );
}
