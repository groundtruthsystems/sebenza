/**
 * The standing BR-08 notice. Persistent rather than dismissable: the scan
 * that backs it is heuristic, so the operator is the real control.
 */
export default function PhiNotice({ className = "" }: { className?: string }) {
  return (
    <p role="note" className={`m-0 text-[11px] text-warning ${className}`}>
      Do not enter PHI. Inbox content is stored in plaintext and sent to the
      configured model provider.
    </p>
  );
}
