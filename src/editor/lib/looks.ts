// One-tap looks: ready-made adjustments, which the sliders can then fine-tune

import { NO_ADJUST, type Adjust } from "./pipeline";

export type Look = { id: string; label: string; adjust: Adjust };

const look = (id: string, label: string, a: Partial<Adjust>): Look => ({ id, label, adjust: { ...NO_ADJUST, ...a } });

export const LOOKS: Look[] = [
  look("original", "Original", {}),
  look("vivid", "Vivid", { contrast: 15, saturation: 35 }),
  look("bright", "Bright", { brightness: 25, contrast: -5 }),
  look("warm", "Warm", { warmth: 40, saturation: 10 }),
  look("cool", "Cool", { warmth: -40, saturation: 5 }),
  look("dramatic", "Dramatic", { brightness: -10, contrast: 45, saturation: -20 }),
  look("fade", "Fade", { brightness: 10, contrast: -30, saturation: -25 }),
  look("mono", "Mono", { contrast: 15, saturation: -100 }),
  look("noir", "Noir", { brightness: -10, contrast: 50, saturation: -100 }),
];

/** The look these adjustments are, if they are one */
export const lookOf = (a: Adjust) =>
  LOOKS.find(
    (l) =>
      l.adjust.brightness === a.brightness &&
      l.adjust.contrast === a.contrast &&
      l.adjust.saturation === a.saturation &&
      l.adjust.warmth === a.warmth
  );
