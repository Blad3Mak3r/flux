// Draws waveforms off the page's thread. It keeps the peaks it was sent, so a redraw at another
// zoom only sends the few numbers that changed.

import { renderWave } from "./waveform";
import type { PeakData, WaveReply, WaveRequest } from "./waveform";

const peaks = new Map<number, PeakData>();

self.onmessage = ({ data }: MessageEvent<WaveRequest>) => {
  if (data.type === "peaks") {
    peaks.set(data.key, { values: data.values, perSecond: data.perSecond });
  } else if (data.type === "forget") {
    peaks.delete(data.key);
  } else {
    const { id, job } = data;
    const canvas = new OffscreenCanvas(Math.max(1, Math.floor(job.width * job.dpr)), Math.max(1, Math.floor(job.height * job.dpr)));
    const context = canvas.getContext("2d");
    if (!context) return;
    renderWave(context, job, data.peaks === null ? undefined : peaks.get(data.peaks));
    const bitmap = canvas.transferToImageBitmap();
    const reply: WaveReply = { id, bitmap };
    self.postMessage(reply, { transfer: [bitmap] });
  }
};
