<script lang="ts">
  interface Props {
    title: string;
    body: string;
  }

  let { title, body }: Props = $props();
</script>

<article class="legal">
  <h1>{title}</h1>
  <pre>{body}</pre>
</article>

<style>
  .legal {
    max-width: 80ch;
  }
  pre {
    margin: 0;
    white-space: pre-wrap;
    font-family: var(--font-sans);
    font-size: var(--text-base);
    line-height: var(--leading-body);
  }
</style>
