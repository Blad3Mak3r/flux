// Points the Download button at the installer for the visitor's system. Without this script, or
// when the system is not recognised, the button goes to the full list of downloads.

type Os = "windows" | "macos" | "linux";
type Arch = "x86_64" | "aarch64";

interface NavigatorWithData extends Navigator {
  userAgentData?: {
    platform?: string;
    getHighEntropyValues?: (hints: string[]) => Promise<{ architecture?: string }>;
  };
}

function detectOs(nav: NavigatorWithData): Os | null {
  const ua = nav.userAgent;
  // Phones and tablets cannot run the installers. iPadOS says it is a Mac, but it has a touchscreen.
  if (/android|iphone|ipad|ipod|cros/i.test(ua) || (/Macintosh/.test(ua) && nav.maxTouchPoints > 1)) return null;
  const platform = nav.userAgentData?.platform ?? nav.platform ?? "";
  if (/win/i.test(platform) || /Windows/.test(ua)) return "windows";
  if (/mac/i.test(platform) || /Mac OS X/.test(ua)) return "macos";
  if (/linux|x11/i.test(platform) || /Linux|X11/.test(ua)) return "linux";
  return null;
}

/** Browsers that hide the CPU leave Apple silicon, which every Mac made since 2020 has. */
async function detectArch(nav: NavigatorWithData): Promise<Arch> {
  try {
    const { architecture } = (await nav.userAgentData?.getHighEntropyValues?.(["architecture"])) ?? {};
    if (architecture === "x86") return "x86_64";
  } catch {
    // The default below applies.
  }
  return "aarch64";
}

const button = document.querySelector<HTMLAnchorElement>("#download");
const meta = document.querySelector<HTMLElement>("#download-file");

if (button && meta) {
  const nav = navigator as NavigatorWithData;
  const os = detectOs(nav);
  const rows = [...document.querySelectorAll<HTMLElement>("[data-download]")];

  const pick = async (): Promise<HTMLElement | undefined> => {
    if (os === "windows") return rows.find((row) => row.dataset.os === "windows");
    if (os === "macos") {
      const arch = await detectArch(nav);
      return rows.find((row) => row.dataset.os === "macos" && row.dataset.arch === arch);
    }
    if (os === "linux") {
      const rpm = /Fedora|SUSE|Red Hat|CentOS|Rocky|Alma/i.test(nav.userAgent);
      return (
        rows.find((row) => row.dataset.os === "linux" && row.dataset.format === (rpm ? "rpm" : "deb")) ??
        rows.find((row) => row.dataset.os === "linux")
      );
    }
    return undefined;
  };

  void pick().then((row) => {
    const link = row?.querySelector<HTMLAnchorElement>("a[href]");
    if (!os || !row || !link) return;
    const labels = JSON.parse(button.dataset.labels ?? "{}") as Record<Os, string>;
    button.href = link.href;
    button.textContent = labels[os] ?? button.textContent;
    meta.textContent = `${row.dataset.formatLabel} · ${row.dataset.sizeLabel} · `;
    row.dataset.match = "true";
  });
}
