<script lang="ts">
  // Original emblems, one per expansion, referenced by id from the catalog
  // (client.branding.emblem). Never Blizzard logos (CLAUDE.md principle 7).
  // Unknown ids fall back to a generic emblem, so a catalog can ship a new
  // client before the launcher has its artwork.
  let { emblem, accent, label, size = 56 }: { emblem: string; accent: string; label: string; size?: number } =
    $props();

  const known = new Set(["frost-shield"]);
  const kind = $derived(known.has(emblem) ? emblem : "generic");
  const gradientId = `emblem-${Math.random().toString(36).slice(2)}`;
</script>

<svg width={size} height={size} viewBox="0 0 64 64" role="img" aria-label={label}>
  <title>{label}</title>
  <defs>
    <linearGradient id={gradientId} x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color={accent} stop-opacity="0.95" />
      <stop offset="1" stop-color={accent} stop-opacity="0.35" />
    </linearGradient>
  </defs>

  {#if kind === "frost-shield"}
    <!-- Heater shield with a six-branch ice crystal. -->
    <path
      d="M32 4 L56 12 V30 C56 45 45 55 32 60 C19 55 8 45 8 30 V12 Z"
      fill="var(--bg-elevated)"
      stroke={`url(#${gradientId})`}
      stroke-width="2.5"
    />
    <path d="M32 9 L51 15.5 V30 C51 42 42 50.5 32 55 C22 50.5 13 42 13 30 V15.5 Z" fill={accent} opacity="0.1" />
    <g stroke={accent} stroke-width="2.2" stroke-linecap="round" fill="none">
      {#each [0, 60, 120] as angle (angle)}
        <g transform={`rotate(${angle} 32 31)`}>
          <line x1="32" y1="17" x2="32" y2="45" />
          <polyline points="27.5,21 32,24.5 36.5,21" />
          <polyline points="27.5,41 32,37.5 36.5,41" />
        </g>
      {/each}
    </g>
    <circle cx="32" cy="31" r="2.6" fill={accent} />
  {:else}
    <!-- Generic: faceted gem in a hexagon. -->
    <polygon
      points="32,4 56,18 56,46 32,60 8,46 8,18"
      fill="var(--bg-elevated)"
      stroke={`url(#${gradientId})`}
      stroke-width="2.5"
    />
    <polygon points="32,18 44,30 32,46 20,30" fill={accent} opacity="0.85" />
    <polyline points="20,30 44,30" stroke="var(--bg-elevated)" stroke-width="1.5" />
  {/if}
</svg>
