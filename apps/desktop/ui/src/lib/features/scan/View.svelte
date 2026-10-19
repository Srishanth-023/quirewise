<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '../../i18n';
  import Dialog from '../../components/Dialog/Dialog.svelte';
  import { ipc, toAppError } from '../../ipc/client';
  import { files } from '../../stores/files.svelte';
  import { toasts } from '../../stores/toasts.svelte';
  import { tools } from '../../stores/tools.svelte';

  let scanner = $state(false);
  let scanning = $state(false);
  let cameraOpen = $state(false);
  let cameraReady = $state(false);
  let taken = $state(0);
  let video: HTMLVideoElement | undefined = $state();
  let stream: MediaStream | undefined;

  onMount(() => {
    void ipc
      .scanAvailable()
      .then((found) => (scanner = found))
      .catch(() => (scanner = false));
    return stopCamera;
  });

  /** The picture goes in the work list and the tool that makes the PDF is chosen. */
  async function useFile(path: string): Promise<void> {
    await files.add([path]);
    tools.select('scan-to-pdf');
  }

  async function scan(): Promise<void> {
    scanning = true;
    try {
      const path = await ipc.scanAcquire();
      if (path) await useFile(path);
    } catch (error) {
      toasts.error(toAppError(error));
    } finally {
      scanning = false;
    }
  }

  async function openCamera(): Promise<void> {
    taken = 0;
    cameraOpen = true;
    cameraReady = false;
    try {
      stream = await navigator.mediaDevices.getUserMedia({ video: { facingMode: 'environment' } });
      if (video) {
        video.srcObject = stream;
        await video.play();
      }
      cameraReady = true;
    } catch {
      cameraOpen = false;
      toasts.info(t('scan.cameraError'));
    }
  }

  function stopCamera(): void {
    stream?.getTracks().forEach((track) => track.stop());
    stream = undefined;
    cameraReady = false;
  }

  function closeCamera(): void {
    stopCamera();
    cameraOpen = false;
  }

  async function capture(): Promise<void> {
    if (!video || !cameraReady) return;
    const canvas = document.createElement('canvas');
    canvas.width = video.videoWidth;
    canvas.height = video.videoHeight;
    canvas.getContext('2d')?.drawImage(video, 0, 0);
    const base64 = canvas.toDataURL('image/png').split(',')[1] ?? '';
    try {
      await useFile(await ipc.saveCapture(base64));
      taken += 1;
    } catch (error) {
      toasts.error(toAppError(error));
    }
  }

  const offered = $derived(tools.items.some((tool) => tool.id === 'scan-to-pdf'));
  const hasCamera = typeof navigator !== 'undefined' && !!navigator.mediaDevices?.getUserMedia;
</script>

{#if offered}
  <section class="scan" aria-labelledby="scan-title">
    <h2 id="scan-title">{t('scan.title')}</h2>
    <div class="row">
      <button type="button" disabled={!scanner || scanning} onclick={() => void scan()}>
        {scanning ? t('scan.scanning') : t('scan.scanner')}
      </button>
      {#if hasCamera}
        <button type="button" onclick={() => void openCamera()}>{t('scan.camera')}</button>
      {/if}
    </div>
    {#if !scanner}<p class="hint">{t('scan.noScanner')}</p>{/if}
    <p class="hint">{t('scan.help')}</p>
  </section>

  <Dialog open={cameraOpen} title={t('scan.cameraTitle')} onclose={closeCamera} wide>
    <video bind:this={video} playsinline muted aria-label={t('scan.preview')}></video>
    <p class="hint" aria-live="polite">{t('scan.taken', { count: taken })}</p>
    <div class="row">
      <button type="button" class="primary" disabled={!cameraReady} onclick={() => void capture()}>
        {t('scan.take')}
      </button>
      <button type="button" onclick={closeCamera}>{t('scan.done')}</button>
    </div>
  </Dialog>
{/if}

<style>
  .scan {
    max-width: 70ch;
  }
  h2 {
    margin: 0 0 var(--space-3);
    font-size: var(--text-base);
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }
  .hint {
    margin: var(--space-2) 0 0;
    color: var(--text-muted);
  }
  video {
    width: 100%;
    max-height: 50vh;
    background: var(--bg);
    border-radius: var(--radius-control);
  }
  button {
    min-height: 32px;
    padding: 0 var(--space-4);
    border: 1px solid var(--control-border);
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--text);
  }
  button.primary {
    border-color: var(--accent);
    color: var(--accent);
    font-weight: 500;
  }
  button:disabled {
    color: var(--text-muted);
  }
</style>
