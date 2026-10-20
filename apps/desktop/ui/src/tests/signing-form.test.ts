import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';

const ipc = vi.hoisted(() => ({ listCertificates: vi.fn() }));

vi.mock('../lib/ipc/client', () => ({
  ipc,
  toAppError: (e: unknown) => e,
}));

import CertificatePicker from '../lib/forms/CertificatePicker.svelte';
import { formFor, ruleMet } from '../lib/forms/toolForms';
import { toasts } from '../lib/stores/toasts.svelte';

const cert = (n: number) => ({
  thumbprint: String(n).repeat(40),
  subject: `Signer ${n}`,
  issuer: 'Test Root',
  valid_until: '2030-01-01',
});

describe('the certificate picker', () => {
  beforeEach(() => {
    ipc.listCertificates.mockReset();
  });

  it('lists the certificates of the Windows store and reports the choice', async () => {
    ipc.listCertificates.mockResolvedValue([cert(1), cert(2)]);
    const onchange = vi.fn();
    render(CertificatePicker, { id: 'c', label: 'Certificate', value: '', onchange });
    const select = await screen.findByLabelText('Certificate');
    expect(screen.getByText(/Signer 1, issued by Test Root, valid until 2030-01-01/)).toBeTruthy();
    expect(screen.getByText(/Signer 2/)).toBeTruthy();
    await fireEvent.change(select, { target: { value: cert(2).thumbprint } });
    expect(onchange).toHaveBeenCalledWith(cert(2).thumbprint);
    // Nothing was chosen for the person while there were two.
    expect(onchange).toHaveBeenCalledTimes(1);
  });

  it('takes the only certificate without asking', async () => {
    ipc.listCertificates.mockResolvedValue([cert(3)]);
    const onchange = vi.fn();
    render(CertificatePicker, { id: 'c', label: 'Certificate', value: '', onchange });
    await waitFor(() => expect(onchange).toHaveBeenCalledWith(cert(3).thumbprint));
  });

  it('says so when there is nothing to sign with', async () => {
    ipc.listCertificates.mockResolvedValue([]);
    render(CertificatePicker, { id: 'c', label: 'Certificate', value: '', onchange: vi.fn() });
    expect(await screen.findByText(/No certificate with a private key was found/)).toBeTruthy();
    expect(screen.queryByRole('combobox')).toBeNull();
  });

  it('shows the error and an empty list when Windows cannot be asked', async () => {
    const error = { code: 'QW-COMPONENT-MISSING', message: 'Not on this machine.' };
    ipc.listCertificates.mockRejectedValue(error);
    const spy = vi.spyOn(toasts, 'error');
    render(CertificatePicker, { id: 'c', label: 'Certificate', value: '', onchange: vi.fn() });
    await waitFor(() => expect(spy).toHaveBeenCalledWith(error));
    expect(await screen.findByText(/No certificate with a private key was found/)).toBeTruthy();
  });
});

describe('the digital signature form', () => {
  const hints = formFor('digital-sign');
  const shown = (key: string, values: Record<string, unknown>) => ruleMet(hints.visibleWhen?.[key], values);

  it('asks for the file and its password, or for a certificate of Windows, never both', () => {
    const file = { source: 'pfx' };
    const store = { source: 'windows-store' };
    expect(shown('pfx_path', file) && shown('pfx_password', file)).toBe(true);
    expect(shown('thumbprint', file)).toBe(false);
    expect(shown('thumbprint', store)).toBe(true);
    expect(shown('pfx_path', store) || shown('pfx_password', store)).toBe(false);
  });

  it('shows the place on the page only for a visible signature', () => {
    for (const key of ['page', 'left', 'top', 'width', 'height']) {
      expect(shown(key, { visible: false }), key).toBe(false);
      expect(shown(key, { visible: true }), key).toBe(true);
    }
  });

  it('shows the server and the consent only for levels that use the internet', () => {
    for (const key of ['timestamp_url', 'allow_network']) {
      expect(shown(key, { level: 'b-b' }), key).toBe(false);
      for (const level of ['b-t', 'b-lt', 'b-lta']) expect(shown(key, { level }), `${key} ${level}`).toBe(true);
    }
  });

  it('hides the password, and picks the certificate file and the certificate list', () => {
    expect(hints.secret).toContain('pfx_password');
    expect(hints.picker?.pfx_path).toBe('certificate');
    expect(hints.certificatePicker).toContain('thumbprint');
  });
});

describe('the signature check form', () => {
  const hints = formFor('verify-signatures');

  it('asks for consent only when revocation is checked', () => {
    const rule = hints.visibleWhen?.allow_network;
    expect(ruleMet(rule, { check_revocation: false })).toBe(false);
    expect(ruleMet(rule, { check_revocation: true })).toBe(true);
    expect(hints.picker?.trusted_certificates).toBe('trust');
  });
});
