# AGENT EXECUTION POLICY — RAG-FIRST

Broad file scanning and large-context loading are forbidden in this project.
All discovery and analysis must go through the `rag` MCP server.

## MCP Server

The `rag` MCP server is automatically active in this project.
It is registered in `.claude/settings.json`.

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
