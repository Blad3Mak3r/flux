// The fields of a slice (its corners, edges and blend) and of a mask, in the video mapping editor.

import { useT } from "../i18n";
import { TextField } from "../components/fields";
import { maskEdit, mappingEdit } from "../video/mapping";
import type { Mask, MappingOp, MaskOp } from "../video/mapping";
import type { Size, Slice } from "../video/mapping";

/** A fraction of `of` pixels, as the whole number of pixels shown in a field. */
const px = (v: number, of: number) => String(Math.round(v * of));

const CORNERS = ["topLeft", "topRight", "bottomRight", "bottomLeft"] as const;

const EDGES = ["left", "right", "top", "bottom"] as const;

/** A fraction as the percentage shown in a field, to a tenth. */
const percent = (v: number) => String(Math.round(v * 1000) / 10);

/** The fields of one mask of `screenId`; `point` is its selected point. */
export function MaskFields({
  screenId,
  mask,
  point,
  setPoint,
  onRemoved,
}: {
  screenId: string;
  mask: Mask;
  point: number;
  setPoint: (point: number) => void;
  onRemoved: () => void;
}) {
  const t = useT();
  const editMask = (op: MaskOp) => maskEdit(screenId, op);
  return (
    <div className="mapping-slice">
          <TextField
            label={t("setup.video.maskName")}
            value={mask.name}
            onCommit={(name) =>
              void editMask({ op: "rename", id: mask.id, name })
            }
          />
          <div className="button-row">
            <button
              type="button"
              className="button"
              aria-pressed={mask.invert ?? false}
              onClick={() =>
                void editMask({
                  op: "setInvert",
                  id: mask.id,
                  invert: !mask.invert,
                })
              }
            >
              {t("setup.video.invertMask")}
            </button>
            <button
              type="button"
              className="button"
              aria-pressed={mask.hidden ?? false}
              onClick={() =>
                void editMask({
                  op: "setHidden",
                  id: mask.id,
                  hidden: !mask.hidden,
                })
              }
            >
              {t("setup.video.hide")}
            </button>
            <button
              type="button"
              className="button"
              onClick={() =>
                void editMask({
                  op: "addPoint",
                  id: mask.id,
                  after: Math.min(point, mask.points.length - 1),
                }).then(() => setPoint(point + 1))
              }
            >
              {t("setup.video.addPoint")}
            </button>
            <button
              type="button"
              className="button"
              disabled={mask.points.length <= 3}
              onClick={() =>
                void editMask({
                  op: "removePoint",
                  id: mask.id,
                  index: Math.min(point, mask.points.length - 1),
                }).then(() => setPoint(Math.max(0, point - 1)))
              }
            >
              {t("setup.video.removePoint")}
            </button>
            <button
              type="button"
              className="button danger"
              onClick={() =>
                void editMask({ op: "remove", id: mask.id }).then(() =>
                  onRemoved(),
                )
              }
            >
              {t("setup.video.removeMask")}
            </button>
          </div>
          <p className="field-note">{t("setup.video.maskNote")}</p>
        </div>
  );
}

/** The fields of one slice of `screenId`, cut from `picture` and placed on `display`. */
export function SliceFields({
  screenId,
  slice,
  picture,
  display,
}: {
  screenId: string;
  slice: Slice;
  picture: Size;
  display: Size;
}) {
  const t = useT();
  const edit = (op: MappingOp) => mappingEdit(screenId, op);
  return (
    <div className="mapping-slice">
          <TextField
            label={t("setup.video.sliceName")}
            value={slice.name}
            onCommit={(name) =>
              void edit({ op: "rename", id: slice.id, name })
            }
          />
          <fieldset className="mapping-values">
            <legend>{t("setup.video.inputValues")}</legend>
            {(["x", "y", "width", "height"] as const).map((field) => (
              <TextField
                key={field}
                label={t(`setup.video.field.${field}`)}
                inputMode="numeric"
                monospace
                value={px(
                  slice.input[field],
                  field === "x" || field === "width"
                    ? picture.width
                    : picture.height,
                )}
                onCommit={(text) =>
                  void edit({
                    op: "setInputValue",
                    id: slice.id,
                    field,
                    text,
                    size: picture,
                  })
                }
              />
            ))}
          </fieldset>
          <fieldset className="mapping-values mapping-corners">
            <legend>{t("setup.video.cornerValues")}</legend>
            {CORNERS.map((name, corner) =>
              (["x", "y"] as const).map((axis) => (
                <TextField
                  key={`${name}-${axis}`}
                  label={t(`setup.video.cornerAxis.${axis}`, {
                    corner: t(`setup.video.cornerName.${name}`),
                  })}
                  inputMode="numeric"
                  monospace
                  value={px(
                    slice.corners[corner][axis],
                    axis === "x" ? display.width : display.height,
                  )}
                  onCommit={(text) =>
                    void edit({
                      op: "setCornerValue",
                      id: slice.id,
                      corner,
                      axis,
                      text,
                      size: display,
                    })
                  }
                />
              )),
            )}
          </fieldset>
          <fieldset className="mapping-values mapping-edges">
            <legend>{t("setup.video.softEdge")}</legend>
            {EDGES.map((field) => (
              <TextField
                key={field}
                label={t(`setup.video.edge.${field}`)}
                inputMode="decimal"
                monospace
                value={percent(slice.softEdge?.[field] ?? 0)}
                onCommit={(text) =>
                  void edit({ op: "setSoftEdge", id: slice.id, field, text })
                }
              />
            ))}
            <TextField
              label={t("setup.video.edge.gamma")}
              inputMode="decimal"
              monospace
              value={String(slice.softEdge?.gamma ?? 1)}
              onCommit={(text) =>
                void edit({
                  op: "setSoftEdge",
                  id: slice.id,
                  field: "gamma",
                  text,
                })
              }
            />
          </fieldset>
          <div className="button-row">
            <button
              type="button"
              className="button"
              aria-pressed={slice.hidden ?? false}
              onClick={() =>
                void edit({
                  op: "setHidden",
                  id: slice.id,
                  hidden: !slice.hidden,
                })
              }
            >
              {t("setup.video.hide")}
            </button>
            <button
              type="button"
              className="button"
              onClick={() => void edit({ op: "resetCorners", id: slice.id })}
            >
              {t("setup.video.resetCorners")}
            </button>
            <button
              type="button"
              className="button danger"
              onClick={() => void edit({ op: "remove", id: slice.id })}
            >
              {t("setup.video.removeSlice")}
            </button>
          </div>
        </div>
  );
}
