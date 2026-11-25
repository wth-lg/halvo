# The hosted model: what leaves, to whom, under which term

This page names the one host to which a build of this tree may send a model request, what leaves for it, and what that
host's own pages said on 2026-11-25, the day on which they were read. The build does not read those pages: a change of
them after that day is not in the build, and the rows and the rates below stay as written until a commit changes them.

## The host and its two models

The host is Anthropic. Its API overview page said on 2026-11-25: "The Claude API is a RESTful API at
`https://api.anthropic.com` that provides programmatic access to Claude models and Claude Managed Agents."
(https://platform.claude.com/docs/en/api/overview)

The table of hosts, `crates/halvo-route/data/hosts.tsv`, holds one row for each hosted tier, and both rows name that one
origin:

| tier | origin | model | adapter |
|---|---|---|---|
| `low_cost_hosted` | `https://api.anthropic.com` | `claude-haiku-5-5` | `anthropic-messages-2026-10-21` |
| `frontier` | `https://api.anthropic.com` | `claude-opus-5-5` | `anthropic-messages-2026-10-21` |

The router makes a grant for the origin of the row of its tier and for no other origin, by equal bytes
(`crates/halvo-route/src/leave.rs`). The sender connects to that origin with the path `/v1/messages` and to nothing else
(`crates/halvo-connect/src/model.rs`).

## What leaves, and to whom

One request goes to `https://api.anthropic.com/v1/messages` for each grant. It holds the model word of the row, the
output limit (at most 2,048 tokens), a system text and the text of one step: the text of the page that the agent has
open, or of the message that it answers, with the fixed texts around it. The whole body is at most 131,072 bytes
(`crates/halvo-route-wire/src/lib.rs`). The person's own key goes in the header `x-api-key`, beside a closed list of five
other headers. The tier of the request stays on the machine; the level that a person set (low, medium or high)
leaves as the host's effort (`output_config.effort`).

The request asks the host for nothing that the host keeps: it holds no cache instruction, no thread, no file and no tool
(`messages_form` in `crates/halvo-connect/src/model.rs` writes the keys `model`, `max_tokens`, `output_config` with its
one key `effort`, `system` and `messages` alone).

## The written term

A key that a person makes at the host is used under Anthropic's Commercial Terms of Service, which show "Effective
Jun 17, 2025" (https://www.anthropic.com/legal/commercial-terms, read 2026-11-25). They say: "They govern Customer’s use
of Anthropic API keys and any other Anthropic offerings that references these Terms, as well as all related Anthropic
tools, documentation and services (the “Services”)." They also say: "Services under these Terms are not for consumer use.
Our consumer offerings (e.g., Claude.ai) are governed by our Consumer Terms of Service instead." And: "Data submitted
through the Services will be processed in accordance with the Anthropic Data Processing Addendum (“DPA”), which is
incorporated into these Terms by reference."

### Whether the host trains on what it is sent

- The Commercial Terms: "Anthropic may not train models on Customer Content from Services."
- The host's privacy article "Is my data used for model training?", dated August 18, 2026
  (https://privacy.claude.com/en/articles/7996868-is-my-data-used-for-model-training, read 2026-11-25): "By default, we
  will not use your inputs or outputs from our commercial products (e.g. Claude for Work, Anthropic API, Claude Gov,
  etc.) to train our models." And: "If you explicitly report feedback or bugs to us (e.g. via our thumbs up/down feedback
  button), or otherwise choose to allow us to use your data, then we may use your chats and coding sessions to train our
  models." Halvo sends no feedback to the host.
- The host's API documentation (https://platform.claude.com/docs/en/manage-claude/api-and-data-retention, read
  2026-11-25): "Retained data is never used for model training without your express permission."

### How long the host keeps it

- The host's article "How long do you store my organization’s data?", dated July 1, 2026
  (https://privacy.claude.com/en/articles/7996866-how-long-do-you-store-my-organization-s-data, read 2026-11-25): "This
  article is about our commercial products such as Claude for Work and the Anthropic API." Its standard term: "For
  Anthropic API users, we automatically delete inputs and outputs on our backend within 30 days of receipt or
  generation, except:" "When you use a service with longer retention under your control (e.g. Files API)", "When you
  and we have agreed otherwise (e.g. zero data retention agreement)", "If we need to retain them for longer to enforce our
  Usage Policy (UP)", "In compliance with the law".
- A request that is flagged, the same article: "We retain inputs and outputs for up to 2 years and trust and safety
  classification scores for up to 7 years if your chat is flagged by our automated trust and safety systems as violating
  our Usage Policy." The API documentation: "Even with ZDR or HIPAA arrangements in place, Anthropic may retain data where
  required by law or where it has been flagged by Anthropic’s automated trust and safety systems. As a result, if a chat
  or session is flagged, Anthropic may retain inputs and outputs for up to 2 years."
- The law and anonymized data, the same article: "In all cases, we may retain your chats or sessions as required by law
  or as necessary to combat violations of our Usage Policy. If permitted in your contract with us, we may anonymize your
  organization’s data for research or statistical purposes, in which case we may retain this information for longer."
  The pages do not say whether the Commercial Terms of a key that a person makes for themselves permit this.
- Which term holds for such a key. The API documentation says: "For Anthropic’s standard retention policies outside these
  arrangements, see the commercial data retention policy and the consumer data retention policy." The arrangements that it
  names (zero data retention, HIPAA readiness, Enterprise Frontier Safeguards) are made with the host: "To request ZDR for
  your organization, contact the Anthropic sales team." A key that a person makes for themselves comes with none of them,
  so the standard term holds: 30 days, up to 2 years where a request is flagged, and longer where the law requires it.
- A sentence of the same documentation that reads otherwise, quoted so that it stands beside the others. Under "Where a
  feature necessarily requires storage, Anthropic designs for the smallest possible retention footprint under the
  following commitments:" it says: "Only what is technically necessary for the feature to work is retained.
  Conversation content (your prompts and Claude’s outputs) is not retained by default; the exception is Covered Models,
  which require 30-day retention." The pages do not say how this sentence and the 30 days of the article fit together;
  the documentation names the article for standard retention, so this page takes the article's term as the one that
  holds.
- The two models are not "Covered Models", which the host keeps 30 days under every arrangement. The API documentation
  names them: "Claude Fable 5.2, Claude Mythos 5.2, Claude Fable 5.1, Claude Mythos 5.1, Claude Fable 5, and Claude
  Mythos 5 are designated Covered Models." The host's article "Data retention practices for Covered Models"
  (https://privacy.claude.com/en/articles/15425996-data-retention-practices-for-covered-models, read 2026-11-25) says:
  "For all other models, everything you use is unaffected and stays under the current terms."
- Features that store, which the request does not use: "Files retained until explicitly deleted or they reach their
  configured expiration." (Files API); "29-day retention; async storage required." (batch processing); a thread's
  contents are "deleted no more than 1 hour after the thread’s last use" (message threads); all in the API documentation.

## The cache at the host

The host's page on prompt caching (https://platform.claude.com/docs/en/build-with-claude/prompt-caching, read
2026-11-25) says: "Messages API: Opt in with `cache_control`, at the top level or on blocks." and "KV (key-value) cache
representations and cryptographic hashes of cached content are held in memory only and are not stored at rest. Cached
entries have a minimum lifetime of 5 minutes (standard) or 1 hour (extended), after which they are promptly, though not
immediately, deleted." The request holds no `cache_control`, so it asks the host for no cache.

## The host's sub-processors

The host's Data Processing Addendum, which shows "Effective Feb 24, 2025"
(https://www.anthropic.com/legal/data-processing-addendum, read 2026-11-25), says: "Anthropic’s list of subprocessors is
available at https://www.anthropic.com/subprocessors." On 2026-11-25 that address led to
https://trust.anthropic.com/subprocessors, a page that shows its list only through a script of another service; this page
names the list by that page and does not copy it.

## What a request costs

The price file `crates/halvo-route/data/prices.tsv` holds the host's rates from its pricing page
(https://platform.claude.com/docs/en/about-claude/pricing, read 2026-11-25), in micro-units per million tokens; the page
says "All prices are in USD."

| tier | model | input | output | the price page's words |
|---|---|---|---|---|
| `low_cost_hosted` | `claude-haiku-5-5` | 500,000 | 2,500,000 | "Claude Haiku 5.5 (for prompts over 100,000 tokens) ... $0.50 / MTok ... $2.50 / MTok" |
| `frontier` | `claude-opus-5-5` | 4,000,000 | 20,000,000 | "Claude Opus 5.5 ... $4 / MTok ... $20 / MTok" |

For `claude-haiku-5-5` the page gives a lower price for a prompt of up to 100,000 tokens ($0.10 and $0.50 per million
tokens). Halvo counts at the higher price for every request, so its count is never below what the host bills for these
tokens and may be up to five times above it.
