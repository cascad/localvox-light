import { type Provenance as Prov } from "../api";

/** What produced this text. A derived document is only disposable because it can be rebuilt — and
 *  it can only be rebuilt if you know which model and which template made it. Comparing two
 *  summaries of the same recording is the whole point of being able to swap models, and without
 *  this line the two are indistinguishable.
 *
 *  Absent for a document written before the header existed, or one edited by hand. Then there is
 *  nothing to show — and inventing a plausible model name would be the worst of both. */
export function Provenance({ p }: { p?: Prov | null }) {
  if (!p) return null;
  // Sliced, not re-parsed into a Date: the stamp already carries the offset it was written in,
  // and re-rendering it in the browser's zone would silently restate WHEN the cook ran.
  const when = p.at ? p.at.slice(0, 16).replace("T", " ") : "";
  return (
    <div className="prov" data-nocopy>
      <span>
        сварено <b>{p.model || "—"}</b>
      </span>
      {p.template && (
        <>
          <span className="sep">·</span>
          <span>шаблон {p.template}</span>
        </>
      )}
      <span className="sep">·</span>
      <span>глоссарий: {p.glossary} {plural(p.glossary, "замена", "замены", "замен")}</span>
      {when && (
        <>
          <span className="sep">·</span>
          <span>{when}</span>
        </>
      )}
    </div>
  );
}


/** 1 замена, 2 замены, 5 замен. */
function plural(n: number, one: string, few: string, many: string): string {
  const mod100 = n % 100;
  if (mod100 >= 11 && mod100 <= 14) return many;
  const mod10 = n % 10;
  if (mod10 === 1) return one;
  if (mod10 >= 2 && mod10 <= 4) return few;
  return many;
}
