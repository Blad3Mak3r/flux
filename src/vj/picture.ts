/** The backend's picture: its width and height as two little-endian u16, then its RGBA pixels. `null` when there is none. */
export function picture(bytes: ArrayBuffer): ImageData | null {
  if (bytes.byteLength < 4) return null;
  const size = new DataView(bytes);
  const width = size.getUint16(0, true);
  const height = size.getUint16(2, true);
  return new ImageData(new Uint8ClampedArray(bytes, 4, width * height * 4), width, height);
}
