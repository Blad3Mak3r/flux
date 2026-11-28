// The media, generators and effects browsers.

import { fail } from "../state/fail";
import { Marquee } from "../components/Marquee";
import { ImportIcon } from "../components/icons";
import { useT } from "../i18n";
import { pickMediaPaths } from "../project/api";
import type { VjGrid, VjLayer } from "../project/types";
import { useDoc } from "../state/store";
import { startDrag } from "../state/pointerDrag";
import { importVideos, showMedia, useVj } from "./store";
import type { MediaKind } from "./store";
import { GENERATORS } from "./generators";
import { layerName } from "./names";
import { Segments } from "./controls";
import { EffectBrowser } from "./EffectStack";
import type { EffectTarget } from "./EffectStack";
import { VideoState, clock, Thumb, MediaState } from "./VideoThumb";
import { NO_VIDEOS, DRAG } from "./shared";
import type { Dragged } from "./shared";

const MEDIA_KINDS: MediaKind[] = ["all", "video", "image"];

/** The layer a double click or an import goes to: the selected one, else the first of the deck. */
function useBrowserLayer(grid: VjGrid): VjLayer {
  const layerId = useVj((vj) => vj.layer);
  return grid.layers.find((l) => l.id === layerId) ?? grid.layers.find((l) => !l.global) ?? grid.layers[0];
}

export function FilesPanel({ grid }: { grid: VjGrid }) {
  const t = useT();
  const videos = useDoc((doc) => doc.project.videos ?? NO_VIDEOS);
  const probes = useDoc((doc) => doc.video);
  const thumbs = useDoc((doc) => doc.thumbs);
  const target = useBrowserLayer(grid);
  // The clips playing as one string, so the panel draws again only when they change, not on every poll.
  const playingClips = useVj((vj) => vj.play.playing.map((p) => p.clip).join(" "));
  const onAir = new Set(grid.layers.flatMap((l) =>
    (l.clips ?? []).filter((c) => playingClips.split(" ").includes(c.id)).map((c) => c.asset)));
  const importing = useVj((vj) => vj.importing);
  const kind = useVj((vj) => vj.media);
  // A video still being read is listed under "All" only, until the backend says what it is.
  const shown = kind === "all" ? videos : videos.filter((video) => {
    const still = probes[video.id]?.still;
    return still !== undefined && still === (kind === "image");
  });
  const importFiles = async () => {
    try {
      await importVideos(await pickMediaPaths(), target.id, null);
    } catch (err) {
      fail(err);
    }
  };
  return (
    <>
      <header className="vj-ph">
        <div className="vj-tools">
          <Segments options={MEDIA_KINDS} value={kind} label={(o) => t(`vj.media.kind.${o}`)} onPick={showMedia} />
          <button type="button" className="vj-tb" title={t("vj.media.importNote", { layer: layerName(t, grid, target) })}
            onClick={() => void importFiles()}><ImportIcon />{t("vj.media.import")}</button>
        </div>
      </header>
      <div className="vj-pb">
        {videos.length === 0 && importing.length === 0 ? (
          <p className="vj-empty">{t("vj.media.empty")}</p>
        ) : (
          <div className="vj-mg">
            {shown.length === 0 && importing.length === 0 && <p className="vj-empty">{t("vj.media.none")}</p>}
            {shown.map((video) => {
              const probe = probes[video.id];
              return (
                <figure key={video.id} className={`vj-m${onAir.has(video.id) ? " using" : ""}`}
                  onPointerDown={(event) => startDrag(event, { [DRAG]: JSON.stringify({ asset: video.id } satisfies Dragged) }, video.name)}>
                  <Thumb strip={thumbs[video.id]}>
                    {probe && <span className="vj-d">{probe.still ? t("vj.media.still") : clock(probe.frames * probe.rateDen / probe.rateNum)}</span>}
                    <MediaState asset={video.id} />
                  </Thumb>
                  <figcaption><Marquee>{video.name}</Marquee><span className="vj-c">{probe?.codec ?? ""}</span></figcaption>
                </figure>
              );
            })}
            {importing.map((item) => (
              <figure key={item.key} className="vj-m busy">
                <Thumb strip={undefined}><VideoState state="importing" /></Thumb>
                <figcaption><Marquee>{item.name}</Marquee></figcaption>
              </figure>
            ))}
          </div>
        )}
      </div>
    </>
  );
}

export function GeneratorsPanel() {
  const t = useT();
  return (
    <>
      <header className="vj-ph"><span className="vj-dim">{t("vj.generators.note")}</span></header>
      <div className="vj-pb">
        <div className="vj-mg">
          {GENERATORS.map((kind) => (
            <figure key={kind} className="vj-m" title={t("vj.generators.note")}
              onPointerDown={(event) => startDrag(event, { [DRAG]: JSON.stringify({ generator: kind } satisfies Dragged) }, t(`vj.generator.${kind}`))}>
              <Thumb strip={undefined} generator={{ kind }} />
              <figcaption><Marquee>{t(`vj.generator.${kind}`)}</Marquee></figcaption>
            </figure>
          ))}
        </div>
      </div>
    </>
  );
}

export function EffectsPanel({ grid }: { grid: VjGrid }) {
  const t = useT();
  const target = useBrowserLayer(grid);
  const selectedClip = useVj((vj) => vj.clip);
  const tab = useVj((vj) => vj.tab);
  // What a double click on an effect adds it to: the composition while its tab is shown, else the clip selected, or else the layer.
  const effectTarget: EffectTarget = tab === "composition"
    ? "composition"
    : grid.layers.some((l) => l.clips?.some((c) => c.id === selectedClip))
      ? { clip: selectedClip as string }
      : { layer: target.id };
  return (
    <>
      <header className="vj-ph"><span className="vj-dim">{t("vj.effects.note")}</span></header>
      <div className="vj-pb"><EffectBrowser target={effectTarget} /></div>
    </>
  );
}
