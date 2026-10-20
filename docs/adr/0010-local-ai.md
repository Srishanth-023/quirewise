# ADR-0010: Local AI with a llama.cpp sidecar and loopback Ollama

Status: accepted

## Context

Smart Split, AI Summarizer, Translate PDF and the AI parts of Redact and PDF to Markdown need a language model. Documents are confidential: nothing may leave the computer, the feature must be off unless the person chooses it, and licenses of the model files matter to a company.

## Decision

- **Engine.** The llama.cpp server (MIT) runs as a separate process, an on-demand component `llama-cpp` (ADR-0005). Model files are on-demand components `model-<id>`, never part of the installer, from a small catalog in the domain (`domain::tools::ai::MODELS`) in which every entry has a license that allows use at a company (Apache-2.0 today). The Components screen shows the license and asks for a yes before a model downloads.
- **Protocol.** The OpenAI-compatible chat API (`/v1/chat/completions`, streamed) over plain http on the loopback address. The same code talks to Ollama. No proxy from the environment is used.
- **Ollama.** Allowed only at `127.0.0.1`, `localhost` or `[::1]` (parsed, not matched by prefix, so `127.0.0.1.example.com` and `user@host` are refused) and only when the options carry `allow_ollama`. Without it the tool fails with `QW-NETWORK-CONSENT-REQUIRED` before anything is sent. The model list is read only after that.
- **Off by default.** No tool starts a model unless the person runs an AI tool or switches on an AI option. Nothing is started at launch.
- **Sidecar.** The worker starts the server on a free loopback port with a random key in the environment (`LLAMA_API_KEY`, not on the command line), one slot and an 8192-token context, keeps it for the next request and stops it when the worker ends. It is started outside the per-process CPU and memory caps of the worker's job (a language model needs both) in a job of its own that ends it when the worker goes, however the worker ends. The job of the worker allows breakaway only for a child that asks for it.
- **Port.** `LlmEngine` takes the options of the tool (which model and where) and a request, and returns text. It lists the models that are ready. Tools never see processes, ports or HTTP.
- **Trust.** The text of a document is data. Every prompt says so; answers are parsed and checked before use: page numbers must be inside the window, personal details must appear in the page word for word, translations are matched by id and bounded in length, a tidied Markdown piece must have the same words as the original. A wrong answer costs a retry or keeps the original, never a damaged file.
- **Preview.** Smart Split shows its parts before anything is made, and what was shown is what is made. Redact's AI suggestions are added to the list of words, where the person reads them, instead of being searched for invisibly.
- **Text out.** New text is written with installed TrueType fonts embedded whole as composite fonts with glyph codes and a map back to the characters. No shaping is done, so scripts that need it are refused rather than drawn wrong.

## Consequences

- With nothing installed the tools say what to download. The preview and the run never fall back to the internet.
- Quality depends on the model. Small models are fast and weak; the catalog offers three sizes and Ollama can bring any model.
- A language model server can use all the memory and CPU of the computer while it answers; it stops when the worker does.
- Right to left, Indic and Thai text cannot be written into PDFs until a shaping engine is added. Chinese and Japanese depend on an installed plain TrueType font.
- Fonts are embedded whole, so a summary in Arial is about 0.4 MB larger than one in Helvetica.
