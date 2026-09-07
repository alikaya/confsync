<!-- ragpilot:start -->
# AGENT EXECUTION POLICY — RAG-FIRST

Broad file scanning and large-context loading are forbidden in this project.
All discovery and analysis must go through the `rag` MCP server.

## MCP Server

The `rag` MCP server is automatically active in this project.
It is registered in `.mcp.json`.

Available tools:

| Tool | Purpose |
|------|---------|
| `rag_index_status` | Index status and dirty file count |
| `rag_ensure_index` | Re-index changed files |
| `rag_search` | Semantic code search |
| `rag_get_chunks` | Fetch full content by chunk ID |
| `rag_get_file_ranges` | Specific line ranges or symbol definitions |
| `nav_symbol_resolve` | Symbol definition + call graph |
| `nav_call_graph` | BFS call tree (incoming + outgoing) |
| `impact_analyze` | Pre-refactor impact analysis |
| `context_bundle` | Token-budgeted complete context bundle |

────────────────────────────────────────────────────

## 1. INDEX GUARANTEE

At the start of every task:

1. Call `rag_index_status`.
2. If `Dirty files > 0`:
   → Call `rag_ensure_index`.
3. Do not analyze until the index is up to date.

────────────────────────────────────────────────────

## 2. CONTEXT ACQUISITION RULE

At the start of a task:

→ Call `context_bundle(task, budget_tokens)`.

Do not open files manually.
If `rag_search` alone is not enough, prefer `context_bundle`.

Reading an entire file is forbidden.
If needed, use only:
→ `rag_get_file_ranges`
or
→ `rag_get_chunks`

────────────────────────────────────────────────────

## 3. SYMBOL NAVIGATION RULE

When you need information about a function/class:

1. `nav_symbol_resolve`
2. `nav_call_graph`

Do not make a refactor plan without producing the call graph.

────────────────────────────────────────────────────

## 4. REFACTOR SAFETY RULE

Before refactoring:

1. `impact_analyze`
2. Check breaking signals.
3. List the affected files.
4. Then make the change.

Refactoring without impact analysis is forbidden.

────────────────────────────────────────────────────

## 5. NO BROAD FILE READS

The following are forbidden:

✗ Scanning the whole repo
✗ Reading a large file in full
✗ Guessing dependencies

Always use the MCP tools.

────────────────────────────────────────────────────

## 6. TOKEN OPTIMIZATION PRIORITY

When gathering context:

- Maximum 6000 tokens (context_bundle default)
- No unnecessary repetition
- Do not repeat the same query

────────────────────────────────────────────────────

## 7. FALLBACK RULE

If the MCP server is unreachable:

- Notify the user
- Ask for approval before doing any manual file analysis

────────────────────────────────────────────────────

## RagPilot Brain

You have a second brain: a persistent memory that outlives this session and is
not tied to this repository.

- **First thing in a session**, call `brain_load` and take the returned context
  seriously — it holds who you are, what was left half-done and what was
  already decided.
- The moment something is **decided or learned**, call `brain_note` with
  `kind: "decision"`. Do not wait for the end of the session.
- When you are **corrected** — "do not do it that way", "I want it like this" —
  call `brain_note` with `kind: "rule"` and a `why`. Rules load at the start of
  every session, so the same correction never has to be made twice.
- **Before the session ends**, call `brain_flush` with a summary, the decisions
  made, what is still open, and anything you **finished** in `closed_threads`.
  Open work is carried across sessions until you close it, so close what is done
  or it will follow you around.
- If you notice a **previous session closed without a flush**, reconstruct what
  you can from the transcript or the repository, note it, and carry on — a gap
  in the log is worth filling late.
- `brain_search` finds anything recorded earlier. Use it before asking the user
  to repeat themselves.
<!-- ragpilot:end -->
