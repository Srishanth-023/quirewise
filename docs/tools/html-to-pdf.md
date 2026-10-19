# HTML to PDF (Appendix A row 22)

## Purpose

Prints a saved web page, or a web address, to PDF.

## Options

| Option | Type | Meaning |
|---|---|---|
| `url` | text | a web address to print instead of the files in the list; needs `allow_network` |
| `allow_network` | switch | let the page load from the internet; default off |
| `page_size` | choice | `a4`, `letter`, `legal`, `a3` or `custom`; default `a4` |
| `custom_width_mm, custom_height_mm` | number | for `custom`; default 210 by 297 |
| `orientation` | choice | `portrait` or `landscape`; default `portrait` |
| `margin_mm` | number | 0 to 100; default 10 |
| `scale_percent` | number | 10 to 200; default 100 |
| `background_graphics` | switch | print backgrounds; default on |
| `header_footer` | switch | print the title and page number in the margins; default off |
| `load_timeout_s` | number | 1 to 300 seconds the page may take to load; default 15 |

```
qw run html-to-pdf --options '{"url":"https://example.com","allow_network":true}'
```

## How it works

Microsoft Edge (the engine of WebView2) is started headless with an empty profile and driven over the DevTools protocol on a local port: the page loads, the fonts settle, and `Page.printToPDF` writes the file with the paper, margins and scale chosen. Without `allow_network` every request that is not a local file, `data:` or `blob:` address is stopped before it leaves the computer, so a saved page prints without calling the web.

## Behavior and limits

- Needs Microsoft Edge or the WebView2 Runtime on the computer (it comes with Windows 11).
- A page that does not finish loading in time fails with a message instead of printing half a page.
- Pages that need a sign-in print the sign-in page: the profile is empty and removed afterwards.
- Scripts on the page run, as in a browser; only use pages you trust.

## Errors

`QW-INPUT-NOT-PDF`, `QW-COMPONENT-MISSING`, `QW-OPTION-INVALID`, `QW-ENGINE-TIMEOUT`, `QW-JOB-CANCELLED`, `QW-OUTPUT-VERIFY-FAILED`, `QW-PATH-DENIED`. Without consent: `QW-NETWORK-CONSENT-REQUIRED`; unreachable address: `QW-NETWORK-UNAVAILABLE`.

## License impact

Microsoft Edge (not shipped), tungstenite (MIT or Apache-2.0) for the local socket. No copyleft code is linked.
