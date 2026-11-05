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

export const NoteIcon = () => (
  <Icon>
    <path d="M3 2.5 H13 V13.5 H3 Z M5.5 5.5 H10.5 M5.5 8 H10.5 M5.5 10.5 H8.5" />
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

// Inspector tabs and the collapse toggle.
export const SelectionIcon = () => (
  <Icon>
    <path d="M4 2.5 V12.5 L6.8 9.8 L9 13.5 L10.8 12.6 L8.6 8.9 H12.5 Z" />
  </Icon>
);

export const ProjectIcon = () => (
  <Icon>
    <path d="M2.5 4.5 H13.5 M2.5 8 H13.5 M2.5 11.5 H13.5" />
    <rect x="9" y="3" width="2" height="3" fill="currentColor" stroke="none" />
    <rect x="4.5" y="6.5" width="2" height="3" fill="currentColor" stroke="none" />
    <rect x="9" y="10" width="2" height="3" fill="currentColor" stroke="none" />
  </Icon>
);

export const OutputIcon = () => (
  <Icon>
    <path d="M2.5 8 H5 L6.5 4 L9.5 12 L11 8 H13.5" />
  </Icon>
);

export const ChevronLeftIcon = () => (
  <Icon>
    <path d="M10 3.5 L5.5 8 L10 12.5" />
  </Icon>
);

export const ChevronRightIcon = () => (
  <Icon>
    <path d="M6 3.5 L10.5 8 L6 12.5" />
  </Icon>
);

export const GearIcon = () => (
  <Icon>
    <path d="M7 3.2 L7.1 1.5 H8.9 L9 3.2 L10.7 3.9 L12 2.7 L13.3 4 L12.1 5.3 L12.8 7 L14.5 7.1 V8.9 L12.8 9 L12.1 10.7 L13.3 12 L12 13.3 L10.7 12.1 L9 12.8 L8.9 14.5 H7.1 L7 12.8 L5.3 12.1 L4 13.3 L2.7 12 L3.9 10.7 L3.2 9 L1.5 8.9 V7.1 L3.2 7 L3.9 5.3 L2.7 4 L4 2.7 L5.3 3.9 Z M8 6.2 A1.8 1.8 0 1 0 8.01 6.2 Z" />
  </Icon>
);

export const CheckIcon = () => (
  <Icon>
    <path d="M3.5 8.5 L6.5 11.5 L12.5 4.5" />
  </Icon>
);
