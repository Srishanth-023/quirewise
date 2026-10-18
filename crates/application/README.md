# quirewise-application

Use cases and ports. Defines the `Tool` contract, `ToolRegistry`, `RunTool` (in-process `UnitRunner`), batch semantics, the naming service, input validation and the worker IPC protocol (length-prefixed JSON frames).

Depends only on `quirewise-domain`. Engines and infrastructure implement the traits in `ports`.
