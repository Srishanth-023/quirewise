# Smart Split (Appendix A row 12)

## Purpose

Splits a PDF where a language model on this computer says new parts start, following an instruction in your own words, such as "a new file for each invoice". You see the proposed parts first and can change them; what you checked is what is made. The original is never changed. Nothing is sent anywhere else.

## Options

| Option | Type | Meaning |
|---|---|---|
| `instruction` | text | How to split, in your own words (up to 2000 characters). Needed unless `parts` is given. |
| `parts` | list | The parts to make: `{ "start": 1, "end": 3, "title": "Invoice 1001" }` with pages from 1. Empty: the model proposes them from the instruction. The desktop app fills this in from the preview. Parts must be in page order, must not overlap and must lie inside the document; pages that are in no part are left out. |
| `ai` | group | The model: `backend` (`bundled`, default, or `ollama`), `model`, `ollama_url`, `allow_ollama`. See "The model" below. |

```
qw run smart-split scans.pdf --options '{"instruction":"a new file for each invoice","ai":{"backend":"ollama","allow_ollama":true,"model":"llama3.2:3b"}}'
```

Each part is written as `{name} ({title}).pdf`.

## How it works

1. The first 120 characters of every page are read (on one line) and sent with the instruction, 40 pages at a time. Longer documents are read in windows, and each window is told the title of the part that was open before it.
2. The model answers with the pages where a new part starts and a short title. Only page numbers inside the window are used; repeats and nonsense are dropped. Page 1 always starts a part.
3. The parts run from each start to the page before the next one. If the model found no place to split, the tool says so and asks for a closer description instead of writing one file.
4. In the app, **Show the parts** runs steps 1 to 3 and lists the parts with their pages and names, which you can edit or remove. The run then uses exactly those parts and does not ask the model again.
5. Each part is copied out of the document and checked like every other output.

## The model

`bundled` runs the llama.cpp server that you download in Settings, Components, with a model file whose license is shown before it downloads. It starts when the first tool needs it and stops when the worker stops. `ollama` talks to an Ollama server that already runs on this computer, only at an address on this computer (`127.0.0.1`, `localhost` or `[::1]`), and only when `allow_ollama` is on. The text of the pages never goes anywhere else and no proxy is used.

## Behavior and limits

- The text of your pages is data for the model. The prompt tells it never to follow sentences inside the text, and its answer is only ever read as page numbers and titles, which are checked.
- Scanned pages have no text: run OCR first. A page without text is shown to the model as "(no text)".
- Small models split worse than large ones. Read the preview.
- A very long document takes several requests; a run can be cancelled at any time.

## Errors

`QW-OPTION-INVALID` (no instruction, parts that overlap or lie outside the document, no text on the pages, no place to split), `QW-NETWORK-CONSENT-REQUIRED` (Ollama without `allow_ollama`), `QW-COMPONENT-MISSING` (the program or the model is not installed, or Ollama has no model), `QW-NETWORK-UNAVAILABLE` (Ollama does not answer), `QW-OUTPUT-VERIFY-FAILED` (the model did not answer in a usable form), `QW-ENGINE-TIMEOUT`, `QW-JOB-CANCELLED`, and the input errors of every PDF tool.

## License impact

llama.cpp (MIT) runs as a separate process; models are separate files under their own licenses (see `docs/components.md`). Nothing is linked.
