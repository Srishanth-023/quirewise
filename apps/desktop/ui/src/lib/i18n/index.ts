import { IntlMessageFormat } from 'intl-messageformat';
import en from './en.json';

/** Message keys available in the default locale. */
export type MessageKey = keyof typeof en;

const locale = 'en';
const cache = new Map<string, IntlMessageFormat>();

/** Values substituted into a message. */
export type MessageValues = Record<string, string | number>;

function format(key: string, text: string, values?: MessageValues): string {
  let compiled = cache.get(key);
  if (!compiled) {
    compiled = new IntlMessageFormat(text, locale);
    cache.set(key, compiled);
  }
  return String(compiled.format(values));
}

/**
 * Formats an ICU MessageFormat string for the active locale.
 * Strings are never concatenated: pass values instead.
 */
export function t(key: MessageKey, values?: MessageValues): string {
  return format(key, en[key], values);
}

/**
 * Like {@link t} for keys built at run time, such as `status.${status}`. Unknown keys return the
 * fallback (or the key itself) so a missing string never breaks the screen.
 */
export function tDynamic(key: string, fallback?: string, values?: MessageValues): string {
  const text = (en as Record<string, string>)[key];
  return text === undefined ? (fallback ?? key) : format(key, text, values);
}
