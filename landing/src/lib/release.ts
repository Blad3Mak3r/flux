// The newest published release, written at build time by scripts/release-site.mjs. The file does
// not exist before the first release, and the page then says so.

export interface Download {
  name: string;
  url: string;
  size: number;
  sha256: string | null;
  os: "windows" | "macos" | "linux";
  arch: "x86_64" | "aarch64";
  format: "msi" | "dmg" | "deb" | "rpm";
}

export interface Release {
  version: string;
  tag: string;
  publishedAt: string;
  files: Download[];
}

const found = import.meta.glob<Release>("../data/release.json", { eager: true, import: "default" });

export const release: Release | null = Object.values(found)[0] ?? null;

/** Windows first, then macOS, then Linux; Apple silicon before Intel, .deb before .rpm. */
const ORDER = { windows: 0, macos: 1, linux: 2 } as const;
export const downloads: Download[] = [...(release?.files ?? [])].sort(
  (a, b) =>
    ORDER[a.os] - ORDER[b.os] ||
    (a.arch === b.arch ? 0 : a.arch === "aarch64" ? -1 : 1) ||
    a.format.localeCompare(b.format),
);

export const megabytes = (bytes: number) => `${(bytes / 1_000_000).toFixed(1)} MB`;
