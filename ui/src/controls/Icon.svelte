<script lang="ts">
  import { icons, type IconName } from "./icons";

  // `filled`: the shape is filled too (a starred star); "play" always is.
  let { name, size = 16, label, filled = false }: { name: IconName; size?: number; label?: string; filled?: boolean } = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 16 16"
  fill={filled || name === "play" ? "currentColor" : "none"}
  stroke="currentColor"
  stroke-width={name === "pending" ? 2.2 : 1.5}
  stroke-linecap="round"
  stroke-linejoin="round"
  role={label ? "img" : undefined}
  aria-label={label}
  aria-hidden={label ? undefined : "true"}
>
  <path d={icons[name]} />
</svg>

<style>
  svg { flex: none; display: block; }
</style>
