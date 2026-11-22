import type { ModelNotice } from "./api";

/** Local context, not an alert: existing documents remain usable. */
export function ModelHints({ notices = [], component }: { notices?: ModelNotice[]; component: string }) {
  return <>{notices.filter(n => n.component === component).map(n =>
    <p key={n.component} className="hint" role="note" data-nocopy>{n.text}</p>
  )}</>;
}
