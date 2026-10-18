<script lang="ts">
  import { formatBytes, groupPageSizes } from '../../format';
  import { t } from '../../i18n';
  import type { DocumentReport, Permissions } from '../../ipc/generated';

  interface Props {
    report: DocumentReport;
  }

  let { report }: Props = $props();

  const sizes = $derived(groupPageSizes(report.pages));
  const permissionKeys = ['print', 'copy', 'modify', 'annotate', 'fill_forms', 'assemble'] as const;
  const permissionLabel = {
    print: t('inspect.perm.print'),
    copy: t('inspect.perm.copy'),
    modify: t('inspect.perm.modify'),
    annotate: t('inspect.perm.annotate'),
    fill_forms: t('inspect.perm.fill_forms'),
    assemble: t('inspect.perm.assemble'),
  } satisfies Record<keyof Permissions, string>;
  const yesNo = (value: boolean): string => (value ? t('inspect.yes') : t('inspect.no'));
  const breakdown = $derived([
    ['images', report.size_breakdown.images],
    ['fonts', report.size_breakdown.fonts],
    ['content', report.size_breakdown.content],
    ['other', report.size_breakdown.other],
  ] as const);
  const breakdownLabel = {
    images: t('inspect.breakdown.images'),
    fonts: t('inspect.breakdown.fonts'),
    content: t('inspect.breakdown.content'),
    other: t('inspect.breakdown.other'),
  };
</script>

<section aria-labelledby="inspect-title">
  <h2 id="inspect-title">{t('inspect.title')}</h2>

  {#if report.locked}
    <p class="note">{t('inspect.locked')}</p>
  {/if}

  <dl>
    {#if report.pdf_version}
      <dt>{t('inspect.version')}</dt>
      <dd class="mono">{report.pdf_version}</dd>
    {/if}
    {#if !report.locked}
      <dt>{t('inspect.pages')}</dt>
      <dd class="mono">{report.page_count}</dd>
    {/if}
    <dt>{t('inspect.size')}</dt>
    <dd class="mono">{formatBytes(report.file_size)}</dd>

    <dt>{t('inspect.encryption')}</dt>
    <dd>
      {#if report.encryption}
        <span class="mono">{report.encryption.method}</span>
        {#if report.encryption.key_bits}<span class="mono">, {report.encryption.key_bits} bit</span>{/if}
      {:else}
        {t('inspect.notEncrypted')}
      {/if}
    </dd>
    {#if report.permissions}
      <dt>{t('inspect.permissions')}</dt>
      <dd>
        <ul class="plain">
          {#each permissionKeys as key (key)}
            <li>{permissionLabel[key]}: {yesNo(report.permissions[key])}</li>
          {/each}
        </ul>
      </dd>
    {/if}

    {#if !report.locked}
      <dt>{t('inspect.pageSizes')}</dt>
      <dd>
        <ul class="plain">
          {#each sizes as size (size.firstPage)}
            <li class="mono">{t('inspect.pageSize', { count: size.count, width: size.width, height: size.height })}</li>
          {/each}
        </ul>
      </dd>

      <dt>{t('inspect.fonts')}</dt>
      <dd>
        {#if report.fonts.length === 0}
          {t('inspect.noFonts')}
        {:else}
          <ul class="plain">
            {#each report.fonts as font (font.name)}
              <li>
                <span class="mono">{font.name}</span>,
                {font.embedded ? t('inspect.fontEmbedded') : t('inspect.fontNotEmbedded')}
              </li>
            {/each}
          </ul>
        {/if}
      </dd>

      <dt>{t('inspect.images')}</dt>
      <dd>
        {#if report.images.count === 0}
          {t('inspect.imagesNone')}
        {:else}
          <span class="mono">
            {t('inspect.imageSummary', {
              count: report.images.count,
              min: Math.round(report.images.min_dpi ?? 0),
              max: Math.round(report.images.max_dpi ?? 0),
            })}
          </span>
        {/if}
        {#if report.images.scanned_pages < report.page_count}
          <small>{t('inspect.imagesSampled', { pages: report.images.scanned_pages })}</small>
        {/if}
      </dd>

      <dt>{t('inspect.signatures')}</dt>
      <dd class="mono">{report.signature_count}</dd>
      <dt>{t('inspect.forms')}</dt>
      <dd>{yesNo(report.has_forms)}</dd>
      <dt>{t('inspect.textLayer')}</dt>
      <dd>{yesNo(report.has_text_layer)}</dd>
      <dt>{t('inspect.javascript')}</dt>
      <dd>{report.has_javascript ? t('inspect.javascriptNote') : t('inspect.no')}</dd>
      <dt>{t('inspect.pdfa')}</dt>
      <dd class="mono">{report.pdfa_claim ?? t('inspect.none')}</dd>
      <dt>{t('inspect.attachments')}</dt>
      <dd class="mono">{report.embedded_files}</dd>

      <dt>{t('inspect.breakdown')}</dt>
      <dd>
        <ul class="plain">
          {#each breakdown as [key, bytes] (key)}
            <li>{breakdownLabel[key]}: <span class="mono">{formatBytes(bytes)}</span></li>
          {/each}
        </ul>
      </dd>
    {/if}
  </dl>
</section>

<style>
  h2 {
    margin: 0 0 var(--space-4);
  }
  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: var(--space-3) var(--space-5);
    margin: 0;
  }
  dt {
    color: var(--text-muted);
  }
  dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .plain {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .note {
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--warning);
    border-radius: var(--radius-panel);
  }
  small {
    display: block;
    color: var(--text-muted);
  }
</style>
