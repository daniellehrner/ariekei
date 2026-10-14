<script lang="ts">
  import { icons, type IconName } from "./icons";

  let { name, size = 16, label }: { name: IconName; size?: number; label?: string } = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 16 16"
  fill={name === "play" ? "currentColor" : "none"}
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
