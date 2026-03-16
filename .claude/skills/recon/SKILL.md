---
name: recon
description: Manage Claude Code tmux sessions via the recon CLI
---

You have access to `recon`, a CLI tool for monitoring and managing Claude Code sessions running in tmux.

## Step 1: Discover capabilities

```bash
recon --help
```

## Step 2: Execute the task

Use the commands discovered in Step 1. Common examples:

- `recon` — open the table dashboard
- `recon view` — open the visual dashboard
- `recon new` — interactive form to create a new session
- `recon next` — jump to the next agent waiting for input
- `recon resume` — interactive picker to resume a past session
- `recon json` — get all session state as JSON (useful for scripting)
