import { type PartialInfo } from "../api";
import { mmss } from "../lib";

export function PartialStatus({info}: {info?: PartialInfo | null}) {
  return info ? <p className="hint" data-nocopy>└ Готовая часть · текст до {mmss(info.text_end_sec)} · аудио до {mmss(info.audio_end_sec)} · полная расшифровка ещё не готова</p> : null;
}
