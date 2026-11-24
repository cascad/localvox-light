import type { StageStatus } from "./api";

export function StageArtifact({ stage }: { stage?: Pick<StageStatus, "files" | "artifact"> }) {
  if (!stage?.files?.length) return null;
  return <details className="hint">
    <summary>Результат: {stage.files.join(", ")}</summary>
    {stage.artifact?.source != null && <div>Версия текста: v{stage.artifact.source}</div>}
    {stage.artifact && !stage.artifact.recorded_checksums && <div>Старая запись: проверены файл и версия; историческая контрольная сумма не сохранялась.</div>}
    {stage.artifact?.receipt?.outputs.map(file => <div key={file.path}>
      <code>{file.path}</code> · {file.bytes} байт · {file.transcript ? "SHA-256 текста и таймкодов" : "SHA-256"}: <code>{file.sha256.slice(0, 16)}…</code>
    </div>)}
  </details>;
}
