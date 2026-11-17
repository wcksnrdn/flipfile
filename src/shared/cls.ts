/** Joins the class names that are set: cls("a", busy && "busy", null) → "a busy" */
export const cls = (...names: (string | false | null | undefined)[]) => names.filter(Boolean).join(" ");
