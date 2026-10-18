# quirewise-worker

`quirewise-worker.exe` is the only process that parses untrusted PDF bytes. It confines itself in a Windows Job Object (memory cap, CPU time cap, kill on close), then serves length-prefixed JSON requests over stdio. Run `quirewise-worker --check-confinement` to verify the Job Object is in effect.
