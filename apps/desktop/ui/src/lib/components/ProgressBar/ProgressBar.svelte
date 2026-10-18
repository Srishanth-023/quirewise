<script lang="ts">
  import { t } from '../../i18n';

  interface Props {
    current: number;
    total: number;
    /** Accessible name for the bar, for example the file being processed. */
    label: string;
  }

  let { current, total, label }: Props = $props();

  const percent = $derived(total > 0 ? Math.min(100, Math.round((current / total) * 100)) : 0);
</script>

<div class="progress">
  <div
    class="track"
    role="progressbar"
    aria-label={label}
    aria-valuemin="0"
    aria-valuemax={total}
    aria-valuenow={current}
  >
    <div class="fill" style:width="{percent}%"></div>
  </div>
  <!-- Live region so screen readers hear page counts without focus moving. -->
  <p class="mono" aria-live="polite">{t('progress.pages', { current, total })}</p>
</div>

<style>
  .track {
    height: 8px;
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: var(--surface-2);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    background: var(--accent);
    transition: width var(--motion-base) var(--ease);
  }
  p {
    margin: var(--space-2) 0 0;
    color: var(--text-muted);
  }
</style>
