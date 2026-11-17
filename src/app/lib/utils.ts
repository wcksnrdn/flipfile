import { OFFICE_FILE } from "./constants";

export const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

// "Clipboard 2026-10-20 at 14.03.12", in local time (matches the macOS screenshot style)
export const clipboardName = (d = new Date()) => {
  const p = (n: number) => String(n).padStart(2, "0");
  return `Clipboard ${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} at ${p(d.getHours())}.${p(d.getMinutes())}.${p(d.getSeconds())}`;
};

// Several *images* dropped on PDF are merged into one; Word documents become one PDF each
export const isMergeable = (paths: string[]) => paths.length > 1 && paths.every((path) => !OFFICE_FILE.test(path));
