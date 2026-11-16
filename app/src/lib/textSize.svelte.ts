// Text size of the launcher (accessibility): a scale applied to the root
// font size, remembered on this computer. The pages scroll when the larger
// text no longer fits.

export type TextSize = "normal" | "large";

export const TEXT_SIZES: TextSize[] = ["normal", "large"];

const SCALE: Record<TextSize, number> = { normal: 1, large: 1.12 };
const STORAGE_KEY = "rk.textSize";

function saved(): TextSize {
  try {
    const value = localStorage.getItem(STORAGE_KEY);
    return TEXT_SIZES.includes(value as TextSize) ? (value as TextSize) : "normal";
  } catch {
    return "normal";
  }
}

let current = $state<TextSize>(saved());

function apply(size: TextSize) {
  document.documentElement.style.setProperty("--text-scale", String(SCALE[size]));
}

apply(current);

export const textSize = {
  get value(): TextSize {
    return current;
  },
  set value(size: TextSize) {
    current = size;
    apply(size);
    try {
      localStorage.setItem(STORAGE_KEY, size);
    } catch {
      // Remembering it is a convenience only.
    }
  },
};
