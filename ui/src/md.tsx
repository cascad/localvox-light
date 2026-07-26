import { marked } from "marked";
import DOMPurify from "dompurify";

/** Render markdown from an LLM answer.
 *
 *  marked → HTML → DOMPurify → the DOM. The sanitiser is NOT optional: the text is model output
 *  about the user's own material, but a poisoned document could make the model echo a `<script>`,
 *  and this page holds the archive's API token — unsanitised HTML here would be an XSS straight at
 *  the archive. GFM is on so fenced code blocks (where the model puts ASCII tables) render
 *  monospace and real `| … |` tables become tables, instead of drifting in a proportional font. */
marked.setOptions({ gfm: true, breaks: false });

// A link in an answer must leave to a real browser, not navigate this webview away from the app
// (which would replace the whole SPA — and its archive token session — with an external page).
DOMPurify.addHook("afterSanitizeAttributes", (node) => {
  if (node.tagName === "A") {
    node.setAttribute("target", "_blank");
    node.setAttribute("rel", "noreferrer noopener");
  }
});

export function Markdown({ text, className }: { text: string; className?: string }) {
  const raw = marked.parse(text ?? "", { async: false }) as string;
  const html = DOMPurify.sanitize(raw);
  return (
    <div
      className={className ? `md ${className}` : "md"}
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}
