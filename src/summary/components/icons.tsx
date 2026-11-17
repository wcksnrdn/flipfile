import type { ReactNode } from "react";
import { cls } from "../../shared/cls";

const Icon = ({ children, className }: { children: ReactNode; className?: string }) => (
  <svg className={cls("sp-icon", className)} viewBox="0 0 24 24" aria-hidden="true">
    {children}
  </svg>
);

/** Sparkle: one large and one small four-pointed star, in pearl */
export const Sparkle = ({ className }: { className?: string }) => (
  <svg className={cls("sp-sparkle", className)} viewBox="0 0 24 24" aria-hidden="true">
    <path d="M10.5 3c.6 4.6 3.9 7.9 8.5 8.5-4.6.6-7.9 3.9-8.5 8.5-.6-4.6-3.9-7.9-8.5-8.5C6.6 10.9 9.9 7.6 10.5 3z" />
    <path d="M19 1.5c.25 1.7 1.3 2.75 3 3-1.7.25-2.75 1.3-3 3-.25-1.7-1.3-2.75-3-3 1.7-.25 2.75-1.3 3-3z" />
  </svg>
);

/** The pearl gradient the sparkles are filled with (referenced as url(#sp-ai) in the CSS) */
export const PearlGradient = () => (
  <svg className="sp-defs" aria-hidden="true">
    <defs>
      <linearGradient id="sp-ai" x1="0" y1="0" x2="1" y2="1">
        <stop offset="0" stopColor="#ffffff" />
        <stop offset="0.55" stopColor="#d6dfeb" />
        <stop offset="1" stopColor="#ffffff" />
      </linearGradient>
    </defs>
  </svg>
);

export const CopyIcon = () => (
  <Icon>
    <rect x="9" y="9" width="11" height="11" rx="2.5" />
    <path d="M5 15V6.5A2.5 2.5 0 0 1 7.5 4H15" />
  </Icon>
);
export const SaveIcon = () => (
  <Icon>
    <path d="M12 4v11M7.5 10.5 12 15l4.5-4.5M5 20h14" />
  </Icon>
);
export const CloseIcon = () => (
  <Icon>
    <path d="M7 7l10 10M17 7 7 17" />
  </Icon>
);
export const CheckIcon = () => (
  <Icon>
    <path d="m5.5 12.5 4 4L18.5 7.5" />
  </Icon>
);
export const LockIcon = () => (
  <Icon>
    <rect x="5.5" y="11" width="13" height="9" rx="2.5" />
    <path d="M8.5 11V8.5a3.5 3.5 0 0 1 7 0V11" />
  </Icon>
);
export const TranslateIcon = () => (
  <Icon>
    <path d="M4 6h9M8.5 4v2M11 6c-1 3.6-3.4 6.3-6.5 7.8M6.8 8.6c1 2.1 2.7 3.8 5 5" />
    <path d="m13 20 3.8-8.5L20.6 20M14.4 17.2h4.8" />
  </Icon>
);
export const FolderIcon = () => (
  <Icon>
    <path d="M4 7.5A2.5 2.5 0 0 1 6.5 5H10l2 2.2h5.5A2.5 2.5 0 0 1 20 9.7v6.8a2.5 2.5 0 0 1-2.5 2.5h-11A2.5 2.5 0 0 1 4 16.5z" />
  </Icon>
);
export const SendIcon = () => (
  <Icon>
    <path d="M12 19V5.5M6.5 11 12 5.5l5.5 5.5" />
  </Icon>
);
export const QuoteIcon = () => (
  <Icon>
    <path d="M9.5 8.5c-2.4.8-4 2.9-4 5.5v2.5h4V12.5h-2M18.5 8.5c-2.4.8-4 2.9-4 5.5v2.5h4V12.5h-2" />
  </Icon>
);
export const RetryIcon = () => (
  <Icon>
    <path d="M19.5 12a7.5 7.5 0 1 1-2.2-5.3" />
    <path d="M19.5 4.5v4.6h-4.6" />
  </Icon>
);
