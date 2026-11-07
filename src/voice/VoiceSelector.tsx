import { useT } from "../i18n";
import { SelectField } from "../components/fields";
import { changePreferences } from "../preferences/store";
import { loadVoices, operatorEdit, useVoice } from "./store";

export function VoiceSelector({ scope, value, trackId }: { scope: "flux" | "project" | "track"; value?: string | null; trackId?: string }) {
  const t = useT();
  const voices = useVoice((v) => v.voices);
  const loading = useVoice((v) => v.loading);
  const error = useVoice((v) => v.listError);
  return <>
    <SelectField label={t(`voice.scope.${scope}`)} value={value ?? ""} onChange={(voice) => {
      if (scope === "flux") void changePreferences({ op: "voice", voice: voice || null }).catch(() => undefined);
      else void operatorEdit({ op: "setVoice", trackId: trackId ?? null, voice: voice || null });
    }}>
      <option value="">{t(scope === "flux" ? "voice.none" : "voice.inherit")}</option>
      {value && !voices.some((v) => v.voice_id === value) && <option value={value}>{value}</option>}
      {voices.map((voice) => <option key={voice.voice_id} value={voice.voice_id}>{voice.name}</option>)}
    </SelectField>
    <button className="button" type="button" disabled={loading} onClick={() => void loadVoices()}>{t(loading ? "voice.loading" : "voice.loadVoices")}</button>
    {error && <p className="error" role="alert">{error}</p>}
  </>;
}

