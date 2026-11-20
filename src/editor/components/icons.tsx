// The editor's line icons (24×24, stroked in the current colour)

import type { ReactNode } from "react";

const Icon = ({ children }: { children: ReactNode }) => (
  <svg
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    {children}
  </svg>
);

export const CropIcon = () => (
  <Icon>
    <path d="M6 2v14a2 2 0 002 2h14" />
    <path d="M18 22V8a2 2 0 00-2-2H2" />
  </Icon>
);

export const AdjustIcon = () => (
  <Icon>
    <path d="M4 21v-7M4 10V3M12 21v-9M12 8V3M20 21v-5M20 12V3M1 14h6M9 8h6M17 16h6" />
  </Icon>
);

export const LooksIcon = () => (
  <Icon>
    <circle cx="9" cy="9" r="6" />
    <circle cx="15" cy="9" r="6" />
    <circle cx="12" cy="15" r="6" />
  </Icon>
);

export const ResizeIcon = () => (
  <Icon>
    <path d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7" />
  </Icon>
);

export const RotateCwIcon = () => (
  <Icon>
    <path d="M21 12a9 9 0 11-9-9c2.52 0 4.93 1 6.74 2.74L21 8" />
    <path d="M21 3v5h-5" />
  </Icon>
);

export const RotateCcwIcon = () => (
  <Icon>
    <path d="M3 12a9 9 0 109-9c-2.52 0-4.93 1-6.74 2.74L3 8" />
    <path d="M3 3v5h5" />
  </Icon>
);

export const FlipHIcon = () => (
  <Icon>
    <path d="M8 3H5a2 2 0 00-2 2v14a2 2 0 002 2h3" />
    <path d="M16 3h3a2 2 0 012 2v14a2 2 0 01-2 2h-3" />
    <path d="M12 20V4" strokeDasharray="2 2" />
  </Icon>
);

export const FlipVIcon = () => (
  <Icon>
    <path d="M21 8V5a2 2 0 00-2-2H5a2 2 0 00-2 2v3" />
    <path d="M21 16v3a2 2 0 01-2 2H5a2 2 0 01-2-2v-3" />
    <path d="M4 12h16" strokeDasharray="2 2" />
  </Icon>
);

export const UndoIcon = () => (
  <Icon>
    <path d="M3 7v6h6" />
    <path d="M21 17a9 9 0 00-9-9 9 9 0 00-6 2.3L3 13" />
  </Icon>
);

export const RedoIcon = () => (
  <Icon>
    <path d="M21 7v6h-6" />
    <path d="M3 17a9 9 0 019-9 9 9 0 016 2.3L21 13" />
  </Icon>
);

export const CompareIcon = () => (
  <Icon>
    <rect x="3" y="4" width="18" height="16" rx="2" />
    <path d="M12 2v20" />
  </Icon>
);

export const CloseIcon = () => (
  <Icon>
    <path d="M18 6L6 18M6 6l12 12" />
  </Icon>
);

export const PhotoIcon = () => (
  <Icon>
    <rect x="3" y="3" width="18" height="18" rx="2" />
    <circle cx="8.5" cy="8.5" r="1.5" />
    <path d="M21 15l-5-5L5 21" />
  </Icon>
);
