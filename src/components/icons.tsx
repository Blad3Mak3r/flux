import type { ReactNode } from "react";

// Drawn for this app: one 16px grid, 1.5px stroke, square caps, as engraved legends would be.
function Icon({ children }: { children: ReactNode }) {
  return (
    <svg
      className="icon"
      viewBox="0 0 16 16"
      width="16"
      height="16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="square"
      strokeLinejoin="miter"
      aria-hidden="true"
    >
      {children}
    </svg>
  );
}

export const StopIcon = () => (
  <Icon>
    <rect x="3.5" y="3.5" width="9" height="9" fill="currentColor" stroke="none" />
  </Icon>
);

export const PlayIcon = () => (
  <Icon>
    <path d="M4.5 3 L13 8 L4.5 13 Z" fill="currentColor" stroke="none" />
  </Icon>
);

export const PauseIcon = () => (
  <Icon>
    <rect x="3.5" y="3" width="3" height="10" fill="currentColor" stroke="none" />
    <rect x="9.5" y="3" width="3" height="10" fill="currentColor" stroke="none" />
  </Icon>
);

export const WaveIcon = () => (
  <Icon>
    <path d="M1.5 8 H3 M5 4.5 V11.5 M7.5 2.5 V13.5 M10 5.5 V10.5 M12.5 3.5 V12.5 M14.5 8 H15" />
  </Icon>
);

export const FlagIcon = () => (
  <Icon>
    <path d="M4 14 V2 M4 3 H12 L10 5.5 L12 8 H4" />
  </Icon>
);

export const MuteIcon = ({ on }: { on: boolean }) => (
  <Icon>
    <path d="M2.5 6 H5 L8.5 3 V13 L5 10 H2.5 Z" fill="currentColor" stroke="none" />
    {on ? <path d="M11 5.5 L15 10.5 M15 5.5 L11 10.5" /> : <path d="M11 5.5 Q13 8 11 10.5" />}
  </Icon>
);

export const ZoomInIcon = () => (
  <Icon>
    <path d="M3 8 H13 M8 3 V13" />
  </Icon>
);

export const ZoomOutIcon = () => (
  <Icon>
    <path d="M3 8 H13" />
  </Icon>
);

export const RefreshIcon = () => (
  <Icon>
    <path d="M13 8 A5 5 0 1 1 11.5 4.5 M13 2.5 V5.5 H10" />
  </Icon>
);

export const TrashIcon = () => (
  <Icon>
    <path d="M3 4.5 H13 M6 4.5 V2.5 H10 V4.5 M4.5 4.5 V13.5 H11.5 V4.5 M7 7 V11 M9 7 V11" />
  </Icon>
);

export const PlusIcon = () => (
  <Icon>
    <path d="M8 3 V13 M3 8 H13" />
  </Icon>
);

export const FolderIcon = () => (
  <Icon>
    <path d="M2.5 4.5 H6.5 L8 6 H13.5 V12.5 H2.5 Z" />
  </Icon>
);

// Window buttons sit on the same 16px grid; the stroke is 1px lighter than the content icons so
// they stay quiet beside the menu text.
export const MinimizeIcon = () => (
  <Icon>
    <path d="M3 8.5 H13" />
  </Icon>
);

export const MaximizeIcon = () => (
  <Icon>
    <rect x="3.5" y="3.5" width="9" height="9" />
  </Icon>
);

export const RestoreIcon = () => (
  <Icon>
    <path d="M5.5 5.5 V3.5 H12.5 V10.5 H10.5" />
    <rect x="3.5" y="5.5" width="7" height="7" />
  </Icon>
);

export const CloseIcon = () => (
  <Icon>
    <path d="M3.5 3.5 L12.5 12.5 M12.5 3.5 L3.5 12.5" />
  </Icon>
);
