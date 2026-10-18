<script lang="ts">
  import { t } from '../../i18n';
  import Button from '../Button/Button.svelte';
  import Icon from '../Icon.svelte';

  interface Props {
    /** True while files are dragged over the window. */
    active?: boolean;
    onchoose: () => void;
  }

  let { active = false, onchoose }: Props = $props();
</script>

<section class="zone" class:active aria-labelledby="drop-title">
  <Icon name="file-arrow-up" size={20} />
  <h2 id="drop-title">{t('drop.title')}</h2>
  <p>{t('drop.hint')}</p>
  <Button variant="primary" onclick={onchoose}>{t('drop.browse')}</Button>
</section>

<style>
  .zone {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-4);
    padding: var(--space-8) var(--space-6);
    border: 1px dashed var(--border-strong);
    border-radius: var(--radius-panel);
    background: var(--surface);
    text-align: center;
    transition: background-color var(--motion-fast) var(--ease);
  }
  .active {
    border-style: solid;
    border-color: var(--accent);
    background: var(--selection);
  }
  h2,
  p {
    margin: 0;
  }
  p {
    color: var(--text-muted);
  }
</style>
