<!-- The guide's risk badge: HIGH / MED / LOW. -->
<script lang="ts">
  let { risk }: { risk: "high" | "medium" | "low" } = $props();
  const TEXT = { high: "HIGH", medium: "MED", low: "LOW" };
</script>

<span class="risk {risk}" title="{risk} risk">{TEXT[risk]}</span>

<style>
  .risk {
    flex: none; display: inline-block; height: 16px; padding: 0 5px; border-radius: var(--r-sm);
    font-size: 10px; line-height: 16px; font-weight: 700; letter-spacing: 0.04em;
  }
  .high { background: var(--risk-high-bg); color: var(--risk-high-fg); }
  .medium { background: var(--risk-med-bg); color: var(--risk-med-fg); }
  .low { background: var(--risk-low-bg); color: var(--risk-low-fg); box-shadow: inset 0 0 0 1px var(--border); }
</style>
