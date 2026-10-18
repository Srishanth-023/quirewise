<script lang="ts">
  import { icons, type IconName } from './icons';

  interface Props {
    name: IconName;
    size?: 16 | 20;
  }

  let { name, size = 16 }: Props = $props();
</script>

<!-- Markup comes from bundled Phosphor SVG files, never from user input. -->
<!-- eslint-disable-next-line svelte/no-at-html-tags -->
<span class="icon s{size}" aria-hidden="true">{@html icons[name]}</span>

<style>
  .icon {
    display: inline-flex;
    flex: none;
    color: currentColor;
  }
  .icon :global(svg) {
    width: 100%;
    height: 100%;
    fill: currentColor;
  }
  .s16 {
    width: 16px;
    height: 16px;
  }
  .s20 {
    width: 20px;
    height: 20px;
  }
</style>
