import { IntlMessageFormat } from 'intl-messageformat';
import en from './en.json';

/** Message keys available in the default locale. */
export type MessageKey = keyof typeof en;

const locale = 'en';
const cache = new Map<MessageKey, IntlMessageFormat>();

/**
 * Formats an ICU MessageFormat string for the active locale.
 * Strings are never concatenated: pass values instead.
 */
export function t(key: MessageKey, values?: Record<string, string | number>): string {
  let format = cache.get(key);
  if (!format) {
    format = new IntlMessageFormat(en[key], locale);
    cache.set(key, format);
  }
  return String(format.format(values));
}
