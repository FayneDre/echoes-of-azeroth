<script lang="ts">
  // The RealmKeeper mark (ADR 0021): a crenellated keep whose gate is a
  // keyhole, in a hexagonal seal. An original drawing; the same paths make
  // the application icons (`app/src-tauri/icons/logo.svg`).
  let { size = 40 }: { size?: number } = $props();

  const id = `logo-${Math.random().toString(36).slice(2)}`;
</script>

<svg width={size} height={size} viewBox="0 0 64 64" aria-hidden="true">
  <defs>
    <linearGradient id={`${id}-gold`} x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#fff1b8" />
      <stop offset="0.45" stop-color="#ffd25a" />
      <stop offset="1" stop-color="#a8721f" />
    </linearGradient>
    <linearGradient id={`${id}-line`} x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#ffe08a" />
      <stop offset="1" stop-color="#8a5c1a" />
    </linearGradient>
    <radialGradient id={`${id}-inner`} cx="0.5" cy="0.35" r="0.7">
      <stop offset="0" stop-color="#2a1d10" />
      <stop offset="1" stop-color="#120c07" />
    </radialGradient>
  </defs>
  <path
    d="M32 2.5 L58 17.5 V46.5 L32 61.5 L6 46.5 V17.5 Z"
    fill={`url(#${id}-inner)`}
    stroke={`url(#${id}-line)`}
    stroke-width="2.4"
    stroke-linejoin="round"
  />
  <path d="M32 7.5 L53.5 20 V44 L32 56.5 L10.5 44 V20 Z" fill="none" stroke="#e0b04f" stroke-opacity="0.35" />
  <path d="M19 49 V18 H24.2 V22.2 H29.4 V18 H34.6 V22.2 H39.8 V18 H45 V49 Z" fill={`url(#${id}-gold)`} />
  <path
    d="M32 29.5 a4.6 4.6 0 0 1 2.4 8.5 l1.9 9 h-8.6 l1.9 -9 a4.6 4.6 0 0 1 2.4 -8.5 Z"
    fill="#140d07"
  />
  <path d="M14.5 49.2 H49.5" stroke={`url(#${id}-line)`} stroke-width="2.4" stroke-linecap="round" />
</svg>
