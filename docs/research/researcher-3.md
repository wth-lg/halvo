# researcher #3: Comparable products
STATUS: ISSUES
now: 2026-11-09    stale cutoff: 2026-05-09    lane: 3 (comparable products: vendor docs, repos, release pages, reviews)

## Summary
- The five spoken names resolve as follows (details in `## Names`). "Open Claw" is OpenClaw, an open-source personal agent that runs on the user's machine, latest release v2026.11.2 on 2026-11-08 (#1, #19, #20). "Muse" is Meta Muse, launched 2026-09-08 on iOS, Android and the muse.ai web app; it runs in a Meta-hosted cloud VM, not on the user's computer (#12). "Claude Code" is Anthropic's agentic coding tool, v2.1.319 on 2026-11-06, with terminal, IDE, desktop-app and web surfaces (#2, #24). "Codex" is OpenAI's coding agent: CLI rust-v0.167.0 on 2026-11-06, and a desktop app that merged into the ChatGPT desktop app for macOS and Windows on 2026-07-09 with a Computer Use mode (#3, #26, #27, #29). "the Grok bot" best matches xAI's grok-4.20-multi-agent model, whose reasoning.effort setting selects 4 or 16 collaborating agents (#30, #31); it is an API model and a grok.com mode, not a desktop agent, and no vendor page describes the orchestration interface the owner remembers, so the match is medium confidence.
- Ten more products in the category, newest first by current release: Screenpipe app-v2.7.145 (2026-11-06), Manus Studio (docs 2026-11-04), Browser Use 0.13.14 (2026-10-26), UI-TARS Desktop v0.3.1 (2026-10-08), Claude Cowork (current form 2026-09-16, GA 2026-04-09), Gemini auto browse in Chrome (2026-01-28; Gemini Spark desktop integration still announced-only), Perplexity Personal Computer (all Mac users 2026-05-07), Microsoft Copilot Actions with Recall (Windows Insider preview, vendor pages 2025-11-18 and 2025-12-05), Claude in Chrome (GA, undated), Simular Pro (2025-07-08). Rewind and Limitless were checked and are discontinued (#38); Fellou was checked and its Windows build and release date could not be confirmed (#44).
- What each demands of the user, from vendor pages: OpenClaw needs Node.js 24 or 26 and an existing Claude Code or Codex CLI login or a provider API key, installed by a shell one-liner or a vendor app (#19, #20). Claude Code starts with "open a terminal" (#24). Codex and ChatGPT Computer Use need a ChatGPT account on a plan that includes it, and on macOS the Screen Recording and Accessibility permissions (#26, #27). Cowork needs a paid Claude plan and runs sessions in Anthropic's cloud (#6). Manus Studio needs an Apple Silicon Mac or Windows 10/11, a Manus account with credits, and Screen Recording on macOS (#5). Perplexity Personal Computer needs a Mac and a Pro or Max subscription (#17). Copilot Actions needs a Windows Insider build and an opt-in toggle that is off by default; Recall needs a Copilot+ PC and Windows Hello biometrics (#32, #33, #34, #35). Screenpipe runs as a desktop app and budgets 5 to 20 percent CPU and 0.5 to 3 GB RAM, self-reported (#14). UI-TARS Desktop and Browser Use need a model provider API key, and Browser Use needs Python and pip (#10, #42). Simular Pro needs macOS 15 or later (#36).
- Where each fails a person with no technical background, with the cited source: OpenClaw's own guides route through the terminal and reviewers say it is not a starting point for someone who has never used a command line (#21, #22), and Cisco found a third-party skill that exfiltrated data (#23). Claude Code requires a terminal to install (#24). Codex Computer Use is a plan-gated feature inside a developer-oriented app (#27). Muse has no desktop app and Amazon blocked it from shopping on its site over credential and transparency concerns (#11, #12). Cowork is paid-only and cloud-hosted, which conflicts with the owner's retention constraint (#6). Copilot Actions is an experimental Insider preview that is off by default (#32, #33). Recall is limited to Copilot+ hardware (#34, #35). Perplexity Personal Computer is Mac-only and orchestrates from Perplexity's servers (#17, #18). Manus Studio excludes Intel Macs and bills by credits (#5). Screenpipe's commercial license and resource budget are developer-facing facts the user must weigh (#14). UI-TARS, Browser Use and Agent S are developer tools that require API keys and command-line install (#10, #37, #42). Simular Pro requires macOS 15 and a 2025 vendor page is the newest primary source (#36).
- Observation over time: only Screenpipe and Windows Recall capture the user's computer continuously, and both keep the capture on the device by default. Screenpipe's README claims it maps workflows and helps "find work worth automating", which is the closest match to Halvo's identify-workflows step, but the claim is self-described and aimed at companies (#14, #15). Recall is opt-in per user, requires Windows Hello at every launch, filters sensitive content by default, and proposes nothing (#34, #35). Every other product is task-driven with per-action consent: ChatGPT Computer Use asks per app with an "Always allow" option (#27); Manus Studio defaults to "Approve for me" per task (#5); Copilot Actions runs under a separate agent account with access to a few known folders and asks before sensitive actions (#32); Muse routes all network egress through a Sentinel agent that asks the person when needed (#12); Cowork offers user-scheduled tasks and account-level memory, not observation (#6). No product in this set was confirmed to propose automations from passive observation.

## Findings
| # | claim | source | type | date | primary? | confidence |
|---|---|---|---|---|---|---|
| 1 | OpenClaw: latest GitHub release is v2026.11.2, published 2026-11-08; 100 or more releases in the last 12 months (API page cap), MIT license, not archived | https://github.com/openclaw/openclaw/releases | release | 2026-11-08 | yes | high |
| 2 | Claude Code: latest GitHub release is v2.1.319, published 2026-11-06; 100 or more releases in the last 12 months (API page cap) | https://github.com/anthropics/claude-code/releases | release | 2026-11-06 | yes | high |
| 3 | Codex: latest stable CLI release is rust-v0.167.0, published 2026-11-06, with 0.169.0 alphas on 2026-11-06 and 2026-11-07 | https://github.com/openai/codex/releases | release | 2026-11-06 | yes | high |
| 4 | screenpipe: latest desktop app release is app-v2.7.145, published 2026-11-06; 100 or more releases in the last 12 months (API page cap) | https://github.com/screenpipe/screenpipe/releases | release | 2026-11-06 | yes | high |
| 5 | Manus Studio: the desktop app for macOS and Windows supports Apple Silicon only ("Intel-based Macs are not supported") and Windows 10 and 11, needs Screen Recording on macOS for Computer Use, uses the same credits as Manus on the web, and defaults to the "Approve for me" permission mode per task | https://manus.im/docs/features/desktop | docs | 2026-11-04 | yes | high |
| 6 | Claude Cowork: available on paid plans only (Pro, Max, Ultra, Team, Enterprise), "with no terminal required", runs sessions remotely in the cloud so files live with the Claude account, and supports scheduled tasks and memory | https://support.claude.com/en/articles/13345190-getting-started-with-cowork | docs | unknown | yes | high |
| 7 | Claude Cowork: generally available on macOS and Windows through the Claude Desktop app on 2026-04-09; "Claude Cowork comes to every conversation" entry dated 2026-09-16; latest release-notes entries 2026-11-05 and 2026-11-06 | https://support.claude.com/en/articles/12138966-release-notes | release | 2026-09-16 | yes | high |
| 8 | browser-use: latest release is 0.13.14, published 2026-10-26; 44 releases in the last 12 months, MIT license | https://github.com/browser-use/browser-use/releases | release | 2026-10-26 | yes | high |
| 9 | UI-TARS: UI-TARS Desktop latest release is v0.3.1, published 2026-10-08; one release in the last 12 months, Apache-2.0 | https://github.com/bytedance/UI-TARS-desktop/releases | release | 2026-10-08 | yes | high |
| 10 | UI-TARS: README states cross-platform support (Windows, macOS, browser) and starts the agent with a command that takes --provider, --model and --apiKey | https://github.com/bytedance/UI-TARS-desktop | repo | 2026-10-09 | yes | high |
| 11 | Muse: Amazon blocked Meta's Muse agent from shopping on amazon.com from 2026-09-20, saying the agent does not identify itself and can reach account pages and order history; Meta says Muse cannot see passwords or payment methods (secondhand report of GeekWire's account; GeekWire and The Register return 403 or a bot challenge to plain curl) | https://ppc.land/amazon-blocks-metas-muse-ai-agent-from-shopping-on-its-site/ | blog | 2026-10-08 | no | low |
| 12 | Muse: launched 2026-09-08 in the US on iOS, Android and muse.ai; runs in a dedicated virtual machine with a separate Sentinel agent that must approve anything reaching the internet and asks the person for permission when needed; Muse cannot see passwords or payment methods; free for most uses with subscription plans | https://about.fb.com/news/2026/09/introducing-muse-personal-ai-agent/ | blog | 2026-09-08 | yes | high |
| 13 | screenpipe: Launch HN post "Record how you work and turn that into agents" was published 2026-07-23 (Hacker News API time field) | https://news.ycombinator.com/item?id=49024620 | forum | 2026-07-23 | yes | high |
| 14 | screenpipe: README says it continuously captures screen and audio, stores capture history on the device by default, maps workflows and helps "find work worth automating"; planning estimates (self-reported, hardware not stated) are 5-20% CPU and 0.5-3 GB RAM; license is a source-available commercial license with personal non-commercial use free | https://github.com/screenpipe/screenpipe | repo | 2026-11-09 | yes | high |
| 15 | Gemini Spark: at I/O 2026 Google said that on desktop it "will be integrating auto browse with Gemini Spark in the coming months", so the desktop agent integration is announced, not shipped | https://developer.chrome.com/blog/chrome-at-io26 | blog | 2026-05-19 | yes | high |
| 16 | Personal Computer: on 2026-05-07 Perplexity made Personal Computer available to all Mac users through its Mac app; it had been limited to Max subscribers with a waitlist; agent features need a Pro or Max subscription | https://techcrunch.com/2026/05/07/perplexitys-personal-computer-is-now-available-everyone-on-mac/ | blog | 2026-05-07 | no | medium |
| 17 | Personal Computer: announced 2026-03-11 as running "on a dedicated Mac mini that can run 24/7, connected to your local apps and Perplexity's secure servers", waitlist opened; page returns 403 to plain curl, 200 with a browser user agent, confirmed by the harness fetch tool | https://www.perplexity.ai/hub/blog/everything-is-computer | blog | 2026-03-11 | yes | high |
| 18 | OpenClaw: getting-started page requires Node.js 24.16.0+ or 26.1.0+ and "an existing Claude Code or Codex CLI login, or a provider API key"; install is a shell one-liner on macOS and Linux, a PowerShell one-liner on Windows, or npx; onboarding asks whether to keep the Gateway running in the background | https://docs.openclaw.ai/start/getting-started | docs | unknown | yes | high |
| 19 | OpenClaw: vendor site offers "Full apps that install everything for you" for macOS, Windows and Linux, including Node.js, next to the curl-pipe-bash installer | https://openclaw.ai/ | docs | unknown | yes | high |
| 20 | OpenClaw: a non-developer reviewer wrote "If you've never used a command line, this isn't your starting point" and estimated 4+ hours for a first deployment | https://genaiunplugged.substack.com/p/what-is-openclaw-beginner-non-developer-assessment | blog | 2026-03-09 | no | low |
| 21 | OpenClaw: the Lenny's Newsletter setup guide's first install step is "Opening the terminal and installing OpenClaw" | https://www.lennysnewsletter.com/p/openclaw-the-complete-guide-to-building | blog | 2026-03-31 | no | low |
| 22 | OpenClaw: Cisco's AI security team found a third-party OpenClaw skill that performed data exfiltration and prompt injection without user awareness | https://blogs.cisco.com/ai/personal-ai-agents-like-openclaw-are-a-security-nightmare | blog | 2026-01-28 | no | medium |
| 23 | OpenClaw: renamed from Clawdbot to Moltbot on 2026-01-27 and to OpenClaw days later; OpenClaw 2.0 (v2026.8.1) released 2026-08-30; security scrutiny over broad permissions; Chinese authorities restricted it on state-enterprise computers in March 2026 (page last edited 2026-10-30) | https://en.wikipedia.org/wiki/OpenClaw | blog | 2026-10-30 | no | medium |
| 24 | Claude Code: overview says it is "Available in your terminal, IDE, desktop app, and browser" and the install instruction is "open a terminal and run the command for your system" | https://code.claude.com/docs/en/overview | docs | unknown | yes | high |
| 25 | Claude Code: setup page lists Windows 10 1809+ or Windows Server 2019+ and the shells Bash, Zsh, PowerShell or CMD | https://code.claude.com/docs/en/setup | docs | unknown | yes | high |
| 26 | Codex: the ChatGPT desktop app page offers downloads for macOS, Windows and Linux, requires signing in with a ChatGPT account, and lists "Run projects in parallel, work with files, use your computer" with Codex as one view | https://learn.chatgpt.com/docs/app | docs | unknown | yes | high |
| 27 | Codex: Computer Use "in the ChatGPT desktop app is available on macOS and Windows with ChatGPT Work and Codex"; macOS needs Screen Recording and Accessibility permissions; ChatGPT asks permission before using each app, with an "Always allow" option that admins can restrict | https://learn.chatgpt.com/docs/computer-use | docs | unknown | yes | high |
| 28 | Codex: the Codex desktop app came to Windows on 2026-03-04 | https://www.engadget.com/ai/openai-brings-its-codex-coding-app-to-windows-195345429.html | blog | 2026-03-04 | no | medium |
| 29 | Codex: available through the ChatGPT web app, the Codex CLI and a desktop app for macOS and Windows; "OpenAI merged the Codex app with the ChatGPT desktop app on July 9, 2026" (page last edited 2026-10-15) | https://en.wikipedia.org/wiki/OpenAI_Codex_(AI_agent) | blog | 2026-10-15 | no | medium |
| 30 | grok-4.20-multi-agent: xAI docs say that for this model the reasoning.effort parameter "Controls agent count (4 or 16)" instead of reasoning depth | https://docs.x.ai/developers/model-capabilities/text/reasoning | docs | unknown | yes | high |
| 31 | grok-4.20-multi-agent: the xAI models page lists grok-4.20-multi-agent-0309 with a 1M-token context and per-token pricing, with no release date on the page | https://docs.x.ai/developers/models | docs | unknown | yes | high |
| 32 | Copilot Actions: experimental, disabled by default, enabled only by the Windows setting "Experimental agentic features"; actions run under a separate agent account inside an agent workspace; during the preview the agent reaches a limited set of known folders and asks for authorization beyond them | https://learn.microsoft.com/en-us/windows/security/book/operating-system-agentic-security | docs | 2025-11-18 | yes | high |
| 33 | experimental agentic features: the setting is off by default, is a security feature for agents such as Copilot Actions, creates a separate Windows session for the agent workspace, and is in preview for Windows Insiders; page updated 2025-12-05 | https://support.microsoft.com/en-us/windows/ai/ai-features/experimental-agentic-features | docs | 2025-12-05 | yes | high |
| 34 | Recall: "Recall (preview) is an experience exclusive to Copilot+ PCs"; saving snapshots is opt-in per user and off by default; every launch or settings change requires Windows Hello; content is processed and stored locally; users can pause, filter apps and websites, and delete snapshots | https://support.microsoft.com/en-us/windows/privacy/privacy-and-control-over-your-recall-experience | docs | unknown | yes | high |
| 35 | Recall: minimum requirements are a Copilot+ PC meeting the Secured-core standard with at least one Windows Hello biometric option; Recall is disabled and removed by default on managed devices and admins cannot enable snapshot saving for users | https://learn.microsoft.com/en-us/windows/client-management/manage-recall | docs | unknown | yes | high |
| 36 | Simular Pro: vendor article dated 2025-07-08 offers "Download Simular Pro (macOS 15+ only)"; no newer vendor page found (articles index returns 404) | https://www.simular.ai/articles/introducing-simular-pro-our-first-production-ready-computer-use-agent | blog | 2025-07-08 | yes | medium |
| 37 | Agent S: open-source computer-use framework, Apache-2.0, installed with pip, runs on macOS, Windows and Linux; latest release v0.3.2 on 2025-12-16, last push 2026-10-30; Agent S4 OSWorld V2 score 21.3% is self-reported by Simular with Qwen3.8-27B | https://github.com/simular-ai/Agent-S | repo | 2026-10-30 | yes | high |
| 38 | Rewind: Limitless states "The Rewind app is sunsetting. The latest update disables all screen and audio capture starting December 19, 2025" and the Pendant is no longer sold as of 2025-12-05 | https://www.limitless.ai/ | docs | unknown | yes | high |
| 39 | Claude in Chrome: "now generally available", a browser extension on all paid plans; "Sessions live with your account, not the machine" | https://claude.com/chrome | docs | unknown | yes | high |
| 40 | auto browse: Chrome's Gemini auto browse rolled out 2026-01-28 to Google AI Pro and AI Ultra subscribers in the US, with a daily action limit; the user must press the buy button | https://9to5google.com/2026/01/28/chrome-gemini-auto-browse/ | blog | 2026-01-28 | no | medium |
| 41 | Claude Cowork: came to Windows on 2026-02-10 as a research preview for paid plans, about a month after the macOS launch | https://www.unite.ai/anthropic-brings-claude-cowork-to-windows-with-full-feature-parity/ | blog | 2026-02-11 | no | medium |
| 42 | browser-use: quickstart is a Python setup with pip and an API key in an environment file | https://docs.browser-use.com/quickstart | docs | unknown | yes | high |
| 43 | Fellou: download page lists Mac builds chosen by chip ("About this machine"); the page text names no Windows build and carries no date or version | https://fellou.ai/download | docs | unknown | yes | low |

## Names
name: Open Claw -> OpenClaw (OpenClaw Foundation, open-source personal AI assistant that runs on the user's machine), https://openclaw.ai/ and https://github.com/openclaw/openclaw, v2026.11.2 released 2026-11-08
name: Muse -> Meta Muse (Meta personal AI agent on iOS, Android and the muse.ai web app, hosted in a Meta cloud VM), https://about.fb.com/news/2026/09/introducing-muse-personal-ai-agent/, launched 2026-09-08
name: Claude Code -> Claude Code (Anthropic agentic coding tool: terminal, IDE, desktop app, web), https://code.claude.com/docs/en/overview, v2.1.319 released 2026-11-06
name: Codex -> OpenAI Codex (coding agent: CLI, IDE extension, cloud, and a desktop view inside the ChatGPT desktop app for macOS and Windows with Computer Use), https://learn.chatgpt.com/docs/app and https://github.com/openai/codex, CLI rust-v0.167.0 released 2026-11-06; app merged into the ChatGPT desktop app 2026-07-09
name: the Grok bot -> xAI grok-4.20-multi-agent (reasoning.effort selects 4 or 16 collaborating agents; an API model and grok.com mode, not a desktop agent; the orchestration interface the owner recalls is not described on any vendor page found), https://docs.x.ai/developers/model-capabilities/text/reasoning, release date unknown

## Options for the planner
Hard requirements: desktop or local-first; macOS or Windows; exists publicly today. Ranked by date of current form, newest first.

1. **OpenClaw v2026.11.2** (2026-11-08)
   - What: open-source personal agent with a local Gateway, messaging channels, skills, memory and heartbeats; vendor apps for macOS and Windows.
   - Fit: meets desktop, macOS and Windows, public. Inference is a hosted provider or a local llama.cpp or Ollama provider (docs index).
   - Tradeoffs: largest community and release cadence; demands Node.js and a provider login or API key; guides and reviews route non-developers through the terminal; documented skill-supply-chain and permission risks.
   - Health: last commit 2026-11-09, latest release v2026.11.2 (2026-11-08), releases in last 12 months 100 or more (API cap), open issues 7936, archived no, license MIT
   - Evidence: #1, #18, #19, #20, #21, #22, #23
2. **Screenpipe app-v2.7.145** (2026-11-06)
   - What: continuous local screen and audio capture with search, an MCP server, "pipes" and a stated goal of finding work worth automating.
   - Fit: meets desktop, macOS and Windows, local by default, public. License is source-available with commercial use requiring a license, which matters for a closed-now, open-later product.
   - Tradeoffs: the only product here that matches Halvo's observation step; self-reported CPU and RAM budget; company-oriented positioning; license is not OSI-open.
   - Health: last commit 2026-11-09, latest release app-v2.7.145 (2026-11-06), releases in last 12 months 100 or more (API cap), open issues 26, archived no, license NOASSERTION on GitHub (Screenpipe Commercial License per README)
   - Evidence: #4, #13, #14
3. **Claude Code v2.1.319** (2026-11-06)
   - What: agentic coding tool from Anthropic; terminal CLI plus IDE, desktop app and web surfaces.
   - Fit: meets desktop, macOS and Windows, public. Inference is hosted.
   - Tradeoffs: the terminal is the install path; aimed at developers.
   - Health: last commit 2026-11-06, latest release v2.1.319 (2026-11-06), releases in last 12 months 100 or more (API cap), open issues 16891, archived no, license none listed on GitHub
   - Evidence: #2, #24, #25
4. **Codex, CLI rust-v0.167.0, ChatGPT desktop app with Computer Use** (2026-11-06)
   - What: OpenAI coding agent; the desktop app runs projects in parallel and can see, click and type in local apps on macOS and Windows.
   - Fit: meets desktop, macOS and Windows, public. Inference is hosted; Computer Use is plan-gated.
   - Tradeoffs: per-app consent with "Always allow" is a usable model for Halvo; the product is still developer-first.
   - Health: openai/codex latest release rust-v0.167.0 (2026-11-06); other health fields not collected (not ranked as a dependency)
   - Evidence: #3, #26, #27, #28, #29
5. **Manus Studio** (docs 2026-11-04)
   - What: Manus desktop app that works on the user's own files, terminal and apps with per-task permission modes and remote control from a phone.
   - Fit: meets desktop, macOS (Apple Silicon only) and Windows 10 and 11, public. Cloud sandbox is the default; local work is a toggle.
   - Tradeoffs: clear permission modes; excludes Intel Macs; credit billing; closed.
   - Health: n/a (closed product, no public repo)
   - Evidence: #5
6. **Browser Use 0.13.14** (2026-10-26)
   - What: Python library that drives a local Chromium with an LLM.
   - Fit: meets local and macOS or Windows, public. It is a library, not an end-user app.
   - Tradeoffs: active and MIT-licensed; needs Python, pip and an API key.
   - Health: last commit 2026-11-09, latest release 0.13.14 (2026-10-26), releases in last 12 months 44, open issues 627, archived no, license MIT
   - Evidence: #8, #42
7. **UI-TARS Desktop v0.3.1** (2026-10-08)
   - What: ByteDance's open-source desktop GUI agent app driven by vision-language models.
   - Fit: meets desktop, macOS and Windows, public. Needs a model provider and API key.
   - Tradeoffs: Apache-2.0; one release in 12 months; developer-facing launch command.
   - Health: last commit 2026-10-09, latest release v0.3.1 (2026-10-08), releases in last 12 months 1, open issues 512, archived no, license Apache-2.0
   - Evidence: #9, #10
8. **Claude Cowork** (current form 2026-09-16; GA 2026-04-09)
   - What: Anthropic's no-terminal agent inside the Claude Desktop app with file access, scheduled tasks, plugins and memory.
   - Fit: meets desktop, macOS and Windows, public. Misses local-first: sessions run in Anthropic's cloud. Paid plans only.
   - Tradeoffs: the closest consumer-grade experience to Halvo's "no terminal, no prompt" goal; cloud hosting conflicts with the retention constraint.
   - Health: n/a (closed product, no public repo)
   - Evidence: #6, #7, #41
9. **Perplexity Personal Computer** (2026-05-07)
   - What: always-on agent in the Perplexity Mac app that uses local files, native Mac apps and the web, orchestrated with Perplexity's servers.
   - Fit: meets desktop and macOS, public. Misses Windows (waitlist) and local-first.
   - Tradeoffs: 24/7 operation and 400+ connectors; Pro or Max subscription; cloud orchestration.
   - Health: n/a (closed product, no public repo)
   - Evidence: #16, #17
10. **Microsoft Copilot Actions with Recall** (vendor pages 2025-11-18 and 2025-12-05; preview)
   - What: OS-level agent account and agent workspace for Copilot Actions; Recall is opt-in local snapshot capture on Copilot+ PCs.
   - Fit: meets desktop and Windows, public as a Windows Insider preview (Copilot Actions) and a Copilot+ preview feature (Recall). Misses macOS.
   - Tradeoffs: the most complete consent model in the set (separate account, known folders, Windows Hello, sensitive-content filter); preview status; hardware-gated.
   - Health: n/a (OS feature)
   - Evidence: #32, #33, #34, #35
11. **Simular Pro** (2025-07-08)
   - What: computer-use agent app for macOS from Simular, built on the Agent S research line.
   - Fit: meets desktop and macOS (15+), public. Misses Windows.
   - Tradeoffs: newest primary source is older than the cutoff; Agent S library is Apache-2.0 and active.
   - Health: simular-ai/Agent-S last commit 2026-10-30, latest release v0.3.2 (2025-12-16), releases in last 12 months 1, open issues 77, archived no, license Apache-2.0; Simular Pro app itself has no public repo
   - Evidence: #36, #37

Rejected: Meta Muse: no desktop app; runs in a Meta cloud VM (fails desktop or local-first) (#12).
Rejected: the Grok bot (grok-4.20-multi-agent): an API model and chat mode, not an agent that lives on the computer (#30, #31).
Rejected: Claude in Chrome: browser extension whose sessions live with the account, not the machine (fails local-first) (#39).
Rejected: Gemini auto browse and Gemini Spark on desktop: auto browse is a Chrome feature for AI Pro and Ultra in the US; the Spark desktop agent is announced only (#15, #40).
Rejected: Rewind and Limitless: capture disabled 2025-12-19, product discontinued (#38).
Rejected: Fellou: Windows build and release date unconfirmed on the vendor page (#43); see Unknowns.

## Unknowns
- Which xAI product the owner means by "the Grok bot" · checked: docs.x.ai reasoning and models pages, x.ai news (403), grok.com (JavaScript shell) · would settle it: the owner naming the screen (grok.com Heavy mode, Grok Build, or the API) or a vendor page for the orchestration UI.
- Exact release dates for the undated vendor pages (Claude Code docs, Cowork getting-started, xAI docs, Recall pages, Claude in Chrome, OpenClaw docs) · checked: page text and HTML meta tags · would settle it: vendor changelogs or the Wayback Machine.
- Whether Fellou ships a Windows build today · checked: fellou.ai/download text, fellou.ai blog (503) · would settle it: a vendor release note or the installer list in a browser.
- Whether Gemini Spark's desktop auto browse integration has shipped since I/O 2026 · checked: Chrome developer blog, one secondary report · would settle it: a Google Chrome or Gemini blog post after 2026-05-19.
- Meta Muse paid tiers ($20 and $100 per month per CNBC) · checked: Meta's post names tiers without prices; CNBC page returns 403 to plain curl · would settle it: muse.ai pricing page.
- Screenpipe's macOS and Windows permission grant flow · checked: README lists what it captures, not the prompts · would settle it: the app's onboarding or docs.

## Stale flags
- https://learn.microsoft.com/en-us/windows/security/book/operating-system-agentic-security (2025-11-18): older than the cutoff on a fast topic; newer vendor page found only as the 2025-12-05 support update (#33), no 2026 Microsoft page found.
- https://support.microsoft.com/en-us/windows/ai/ai-features/experimental-agentic-features (2025-12-05): older than the cutoff; no newer primary found.
- https://www.simular.ai/articles/introducing-simular-pro-our-first-production-ready-computer-use-agent (2025-07-08): older than the cutoff; the vendor articles index returns 404, no newer primary found.
- https://blogs.cisco.com/ai/personal-ai-agents-like-openclaw-are-a-security-nightmare (2026-01-28): older than the cutoff; OpenClaw has shipped many releases since, including 2.0 on 2026-08-30 (#23).
- https://9to5google.com/2026/01/28/chrome-gemini-auto-browse/ (2026-01-28): older than the cutoff; superseded in part by the I/O 2026 post (#15).
- https://www.perplexity.ai/hub/blog/everything-is-computer (2026-03-11): older than the cutoff; superseded by the 2026-05-07 Mac availability (#16).
- Overruled: the 2025-07-08 Simular Pro article by nothing; a Product Hunt listing dated 2026-09-21 was found but returns 403 and was not used.
- Overruled: Chrome auto browse "Gemini 3-powered" rollout (#40) by the I/O 2026 post (#15) on the state of the desktop agent.

## Changes
none

## Evidence
- ~/.wstack/runs/20261108-halvo-kickoff/evidence/researcher-3/fetch.sh, fetch2.sh, fetch3.sh, fetch4.sh: idempotent fetchers; fetch-log.txt holds every HTTP code.
- ~/.wstack/runs/20261108-halvo-kickoff/evidence/researcher-3/raw/<label>.html and .txt: every fetched page and its text.
- ~/.wstack/runs/20261108-halvo-kickoff/evidence/researcher-3/health.sh and health.txt: GitHub API repo health for the six ranked repos.
- ~/.wstack/runs/20261108-halvo-kickoff/evidence/researcher-3/raw/muse-register.txt (The Register, fetched with a browser user agent) backs the Amazon block in Summary; codex-releases.txt, screenpipe-hn-date.txt, openclaw-onboarding-issues.txt, plain-curl-check.txt, snips.txt.
- ~/.wstack/runs/20261108-halvo-kickoff/evidence/researcher-3/url-check.sh and url-check.txt: per-row curl and grep check; row-<n>.html saved beside them.
- ~/.wstack/runs/20261108-halvo-kickoff/evidence/researcher-3/verify.txt: outputs of the Verify by commands.

## Assumptions
- `date -u` printed 2026-11-09 while the harness reported 2026-11-08; the brief says to use the system date, so now is 2026-11-09 and the cutoff is 2026-05-09.
- Repo README rows (#10, #14, #37) carry the repository's last-push date from the GitHub API as the source date.
- The Perplexity changelog entry dated 2026-04-17 in its URL (evidence raw/comet-pc-changelog.txt) is cited in Summary only, because the page returns 403 to plain curl; TechCrunch (#16) carries the Mac availability claim instead.
- "Current release" for OpenClaw, Claude Code, Codex, Screenpipe, Browser Use and UI-TARS is the latest GitHub release tag at fetch time, not a beta or alpha.
- Lane 3 covers products; Astra and the model names stay in lane 5 as the kickoff states.
- The subagent and Workflow tooling were not used, because the dispatch brief forbids subagents.

## Issues and risks
- STATUS is ISSUES because "the Grok bot" rests on a medium-confidence match, several vendor pages carry no date, and three secondary sources carry failure claims (#20, #21, #28, #40, #41).
- Rows that return 403 to plain curl: #17 (perplexity.ai blog) returns 200 with a browser user agent and the harness fetch tool read the page and its Personal Computer sentences; it stays per the brief's 403 rule. CNBC (Muse pricing) and Product Hunt (Simular Pro date) also return 403 and were dropped.
- openai.com, help.openai.com and x.ai return 403 to both curl and the harness fetch tool, so Codex app launch dates come from Engadget and Wikipedia (#28, #29) and Grok consumer tiers were not confirmed.
- The GitHub API release count caps at 100 per page; "100 or more" is a floor for OpenClaw, Claude Code and Screenpipe.
- Screenpipe CPU and RAM figures are the vendor's planning estimates with no stated hardware or run count.
- Agent S OSWorld figures are self-reported by Simular; not comparable across products and not used for ranking.
- Secondary OpenClaw onboarding reviews (#20, #21) are opinion pieces; the GitHub issue search for onboarding complaints (openclaw-onboarding-issues.txt) returned provider and bug reports, not usability complaints.

## Learnings
- openai.com, help.openai.com and x.ai block plain fetches; learn.chatgpt.com, developers.openai.com and docs.x.ai are the reachable primary mirrors.
- perplexity.ai and cnbc.com answer 403 to curl's default user agent and 200 to a browser user agent, and theregister.com returns a 200 bot-challenge page with no article text; the verifier's plain curl will fail all three, so check candidate URLs with a plain curl before citing them.
- The GitHub releases API is the fastest way to a dated "current release" for open-source products; vendor marketing pages rarely carry dates.
