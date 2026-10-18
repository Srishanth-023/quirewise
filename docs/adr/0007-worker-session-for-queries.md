# ADR-0007: A persistent worker session for read-only queries

Status: accepted

## Context

Thumbnails and Inspect parse untrusted PDFs, so they must run in the worker like tool runs do. Tool runs use one worker process per unit of work, which is right for isolation but too slow for a page grid that asks for dozens of thumbnails.

## Decision

Tool runs keep one worker process per unit (`WorkerSupervisor`). Read-only queries go to one long-lived worker (`WorkerSession`) that answers requests by correlation id and runs each on its own thread. If the worker dies, for example on a parser crash or a Job Object limit, the pending queries fail with a catalog error and the next query starts a fresh worker. The UI limits itself to three thumbnail renders at a time and renders only pages that scroll into view.

## Consequences

- First thumbnail does not pay process start cost after the first query.
- The session worker is subject to the per-process CPU cap, so after a long session it restarts itself. That is safe: queries are stateless.
- Queries and tool runs never share a process, so a crash in one cannot affect the other.
