# Roleplay assistant model research: local, offline, tool-calling

Date: 2026-11-04. Status: **research only. No model was downloaded, loaded, benchmarked, bundled or qualified by this investigation.** Every size below is publisher metadata read from the Hugging Face API (`?blobs=true`) or from a published GGUF tree, not a locally verified byte count. Every licence claim is read from the publisher's own card metadata or licence text, at the revision noted. Claims that could not be traced to a primary source are listed in [What I could not verify](#what-i-could-not-verify) instead of being asserted.

Audience: choosing a roleplay/in-character assistant model that can ship inside Pulsar's self-contained, offline-first worker.

## Summary

**Recommended default for a 16 GB machine: `Qwen/Qwen3.5-2B`, Q4_K_M or Q6_K, GGUF, Apache-2.0.** It is the only current-generation instruct model that clears both the 2 GiB artifact bound and a plausible 16 GB coexistence budget while keeping a native tool-calling lineage (`qwen3_coder` parser, [Qwen3.5-2B card](https://huggingface.co/Qwen/Qwen3.5-2B)) and a 262,144-token declared context ([config.json](https://huggingface.co/Qwen/Qwen3.5-2B/raw/main/config.json)). This is not a quality claim: 2B prose will be serviceable, not good, and it is *smaller* than the 4B–12B tier that actual roleplay use favours.

**Best quality if the user has 24 GB+: `Ministral-3-14B` Q4_K_M (8.24 GB) or `Gemma-4-12B` QAT q4_0 (6.98 GB) on CPU; `Qwen/Qwen3.8-27B` at Q3-class (10.9–13.1 GB) only with GPU offload.** The 27B tier is where 2026 roleplay quality actually lives — the derivative ecosystem has largely re-based onto `Qwen3.8-27B` and `gemma-4-12B/26B-A4B` — but 16.7 GB of Q4 weights cannot share a 16 GB machine with a video worker, and at ~2.4 tok/s on CPU it is not interactive even when it fits.

**Recommended cloud option: OpenRouter, a 24B-class roleplay-tuned model in the `thedrummer`/`anthracite`/`cognitivecomputations` families, consent-gated per session.** OpenRouter's own terms contain no blanket sexual-content prohibition; they prohibit unlawful content and content that breaches the underlying model or provider terms ([OpenRouter Terms](https://openrouter.ai/terms)). Anthropic and Google *do* prohibit sexually explicit content outright (below), so those are not viable for this feature regardless of preference.

The single most decision-relevant fact in this report is not a model: **Pulsar's current native launch cannot host roleplay at all, at any model size.** `-c 2048` hardcoded caps chat history at roughly 1,500 words, and `--seed 42 --temp 0` is deterministic decoding, which produces degenerate prose. Both are in [`assistant_worker.rs`](../crates/pulsar-engine/src/assistant_worker.rs) and must change before model choice matters.

## Comparison table

Sizes are publisher-reported GGUF file bytes. "Artifact bound" tests the current protocol limit of 2 GiB (`self.model.byte_len > 2 * 1024 * 1024 * 1024` in [`assistant.rs`](../crates/pulsar-protocol/src/assistant.rs)). KV figures are computed from `config.json` at F16 KV, full attention, and are *bytes of KV alone*, excluding weights, graph, scratch and allocator overhead.

| Model | Params | Quant → size (GB) | Fits 2 GiB bound | Declared ctx | KV @2048 / @8192 | Licence | Native tool calls | GGUF | MLX | Ollama | NSFW per licence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Qwen3.5-2B | 2B | IQ4_XS 1.17 · Q4_K_M 1.28 · Q6_K 1.57 · Q8_0 2.01 | Yes (all listed) | 262,144 | 96 MiB / 384 MiB | Apache-2.0 | Yes ([card](https://huggingface.co/Qwen/Qwen3.5-2B)) | [unsloth](https://huggingface.co/unsloth/Qwen3.5-2B-GGUF) | [4-bit](https://huggingface.co/mlx-community/Qwen3.5-2B-OptiQ-4bit) | [yes](https://ollama.com/library/qwen3.5) | Not restricted by licence; card states no use policy |
| Qwen3.5-4B | 4B | IQ2_M 1.76 · Q3_K_S 2.11 · Q4_K_M 2.74 · Q8_0 4.48 | Only IQ2/Q3 | 262,144 | 256 MiB / 1.0 GiB | Apache-2.0 | Yes | [unsloth](https://huggingface.co/unsloth/Qwen3.5-4B-GGUF) | yes | yes | same |
| Qwen3.5-9B | 9B | IQ4_XS 5.17 · Q4_K_M 5.68 · Q6_K 7.46 | No | 262,144 | 256 MiB / 1.0 GiB | Apache-2.0 | Yes | [unsloth](https://huggingface.co/unsloth/Qwen3.5-9B-GGUF) | [4-bit](https://huggingface.co/mlx-community/Qwen3.5-9B-4bit) | yes | same |
| Qwen3.5-27B | 27B | IQ2_XXS 8.57 · IQ4_XS 14.98 · Q4_K_M 16.74 | No | 262,144 | 512 MiB / 2.0 GiB | Apache-2.0 | Yes | [unsloth](https://huggingface.co/unsloth/Qwen3.5-27B-GGUF) | 4-bit | yes | same |
| Qwen3.8-27B | 27B | IQ2_XXS 7.27 · IQ4_XS 14.25 · Q4_K_S 15.36 | No | 262,144 | 512 MiB / 2.0 GiB | Apache-2.0 | Yes | [unsloth](https://huggingface.co/unsloth/Qwen3.8-27B-GGUF) | 4-bit | [yes](https://ollama.com/library/qwen3.8) | same |
| Ministral-3-3B-2512 | 3B | Q4_K_M 2.147 · Q5_K_M 2.47 · Q8_0 3.65 | **Yes, by 460,640 bytes** | 262,144 | 208 MiB / 832 MiB | Apache-2.0 | Yes ([card](https://huggingface.co/mistralai/Ministral-3-3B-Instruct-2512)) | [official](https://huggingface.co/mistralai/Ministral-3-3B-Instruct-2512-GGUF) | 4-bit | yes | same |
| Ministral-3-8B-2512 | 8B | Q4_K_M 5.20 · Q5_K_M 6.06 · Q8_0 9.03 | No | 262,144 | 272 MiB / 1.06 GiB | Apache-2.0 | Yes | [official](https://huggingface.co/mistralai/Ministral-3-8B-Instruct-2512-GGUF) | 4-bit | yes | same |
| Ministral-3-14B-2512 | 14B | Q4_K_M 8.24 · Q5_K_M 9.62 | No | 262,144 | 320 MiB / 1.25 GiB | Apache-2.0 | Yes | [official](https://huggingface.co/mistralai/Ministral-3-14B-Instruct-2512-GGUF) | 4-bit | yes | same |
| Gemma-4-E2B-it QAT | ~2B eff. | q4_0 3.35 | No | 131,072 | 70 MiB / 280 MiB | Apache-2.0 | Yes ([card](https://huggingface.co/google/gemma-4-12B-it)) | [official](https://huggingface.co/google/gemma-4-E2B-it-qat-q4_0-gguf) | yes | [yes](https://ollama.com/library/gemma4) | same |
| Gemma-4-E4B-it QAT | ~4B eff. | q4_0 5.15 | No | 131,072 | 168 MiB / 672 MiB | Apache-2.0 | Yes | [official](https://huggingface.co/google/gemma-4-E4B-it-qat-q4_0-gguf) | yes | yes | same |
| Gemma-4-12B-it QAT | 12B | q4_0 6.98 | No | 262,144 | 768 MiB / 3.0 GiB | Apache-2.0 | Yes | [official](https://huggingface.co/google/gemma-4-12B-it-qat-q4_0-gguf) | [4-bit](https://huggingface.co/mlx-community/gemma-4-12B-4bit) | yes | same |
| Llama-3.1-8B-Instruct | 8B | Q4_K_M ≈4.9 (third-party) | No | 131,072 | — | **Llama 3.1 licence** | Yes | third-party only | 4-bit | yes | AUP-gated, see below |
| Command-R7B-12-2024 | 7B | third-party only | No | 128k | — | **CC-BY-NC-4.0** | Yes | third-party | no | yes | Non-commercial only |
| magnum-v4-12b | 12B | third-party | No | 32k train | — | Apache-2.0 | No (RP tune of Nemo) | yes | yes | yes | Not restricted by licence |
| Cydonia-24B-v4.1 | 24B | third-party | No | — | — | **None declared** | No | community | no | no | **Redistribution unlicensed** |
| Anubis-Mini-8B-v1 | 8B | third-party | No | — | — | **None declared** | No | community | no | no | **Redistribution unlicensed** |
| Fimbulvetr-11B-v2 | 11B | third-party | No | 8k | — | **CC-BY-NC-4.0** | No | community | no | no | Non-commercial only |
| Stheno-L3-8B-v3.2 | 8B | third-party | No | 8k | — | **CC-BY-NC-4.0** | No | community | no | no | Non-commercial only |

Licence and gating values were read from `https://huggingface.co/api/models/<repo>` `cardData.license` and `tags[]` on 2026-11-04; GGUF byte counts from the same API with `blobs=true`.

## Local open-weight candidates

### Qwen3.5 / Qwen3.8 (Apache-2.0, the strongest permissive option)

Publisher metadata declares `license: apache-2.0` with `license_link` to the repository `LICENSE` for [Qwen3.5-2B](https://huggingface.co/Qwen/Qwen3.5-2B), [Qwen3.5-4B](https://huggingface.co/Qwen/Qwen3.5-4B), [Qwen3.5-9B](https://huggingface.co/Qwen/Qwen3.5-9B), [Qwen3.5-27B](https://huggingface.co/Qwen/Qwen3.5-27B) and [Qwen3.8-27B](https://huggingface.co/Qwen/Qwen3.8-27B); none is gated. Apache-2.0 contains no field-of-use restriction, so neither commercial redistribution nor adult content is restricted *by the licence*. The model cards state no prohibited-use policy — the [Qwen3.5-9B card](https://huggingface.co/Qwen/Qwen3.5-9B) contains only the front-matter licence lines and no restrictions section, which is a notable difference from the Gemma 1–3 and Llama lineages. Absence of a use policy is not the same as endorsement of any use; it means no contractual term.

Architecture, read from each `config.json`: `model_type` `qwen3_5_text`, `max_position_embeddings` 262,144, vocabulary 248,320. KV cost per token is small because `num_key_value_heads` is 2–4 with `head_dim` 256 — 48 KiB/token at 2B, 128 KiB/token at 4B and 9B, 256 KiB/token at 27B. This matters: at 8,192 context a 9B costs 1.0 GiB of KV, which is entirely affordable; the paper context length is *not* the constraint, RAM is.

Tool calling is claimed by the publisher: the 2B and 9B cards both state "Qwen3.5 excels in tool calling capabilities" and document vLLM `--tool-call-parser qwen3_coder` and SGLang `--tool-call-parser qwen3_coder` ([Qwen3.5-2B card](https://huggingface.co/Qwen/Qwen3.5-2B), [Qwen3.5-9B card](https://huggingface.co/Qwen/Qwen3.5-9B)). That is a parser name in a third-party serving stack, not a measured function-calling score.

GGUF is not published by Qwen; [unsloth](https://huggingface.co/unsloth/Qwen3.5-9B-GGUF) publishes the quantisation ladder used for the sizes above, including IQ2/IQ3 variants with an imatrix. llama.cpp at Pulsar's pinned revision supports the architecture: `src/models/qwen35.cpp` and `qwen35moe.cpp` exist and `llama-arch.cpp` maps `LLM_ARCH_QWEN35 → "qwen35"` at [pin `b86d2f0`](https://github.com/ggml-org/llama.cpp/blob/b86d2f07542b29ab099aed34fd6b6d1b2fd4b81c/src/llama-arch.cpp#L41).

### Ministral-3 (Apache-2.0, official GGUF, best packaging story)

The 2512 Ministral-3 line moved to `license: apache-2.0` — [3B](https://huggingface.co/mistralai/Ministral-3-3B-Instruct-2512), [8B](https://huggingface.co/mistralai/Ministral-3-8B-Instruct-2512), [14B](https://huggingface.co/mistralai/Ministral-3-14B-Instruct-2512). This is a change from the earlier [Ministral-8B-Instruct-2410](https://huggingface.co/mistralai/Ministral-8B-Instruct-2410), which declares `license: other` and is not usable as a permissive baseline without reading that custom text.

Mistral publishes **official** GGUF for all three sizes, with a clean quant ladder and no IQ2/IQ1 variants: Q4_K_M 5.20 GB, Q5_K_M 6.06 GB, Q8_0 9.03 GB for 8B; Q4_K_M 8.24 GB for 14B ([8B GGUF](https://huggingface.co/mistralai/Ministral-3-8B-Instruct-2512-GGUF), [14B GGUF](https://huggingface.co/mistralai/Ministral-3-14B-Instruct-2512-GGUF)). Official publication is a real redistribution advantage: the bytes come from the licensee, not from a third-party converter whose provenance Pulsar would otherwise have to inventory.

The 8B card claims "best-in-class agentic capabilities with native function calling and JSON outputting" and documents `--tool-call-parser mistral` ([card](https://huggingface.co/mistralai/Ministral-3-8B-Instruct-2512)). Context is 262,144 declared, vocabulary 131,072, and the KV cost is the cheapest of any 8B-class candidate here (136 KiB/token) because `num_key_value_heads` is 8 against `head_dim` 128.

The 3B variant is a curiosity worth recording: its official Q4_K_M is 2,147,023,008 bytes, which clears the current 2 GiB artifact bound by **460,640 bytes**. Any re-quantisation, added metadata or a stricter bound kills it. Do not design around a 0.02% margin.

### Gemma 4 (Apache-2.0 — a genuine licence change)

Gemma 4 declares `license: apache-2.0` in card front-matter with `license_link: https://ai.google.dev/gemma/docs/gemma_4_license`, and that page's title is "Apache License 2.0 | Gemma" ([Gemma 4 model card](https://huggingface.co/google/gemma-4-12B-it), [Gemma 4 licence page](https://ai.google.dev/gemma/docs/gemma_4_license)). This is a substantive change from the custom Gemma Terms of Use that governed Gemma 1/2/3 and carried a prohibited-use policy; the Gemma 4 card front-matter makes no reference to prohibited-use terms. The card does describe safety evaluation work aligned with Google's AI principles and lists harmful categories the evaluations target ([card, safety section](https://huggingface.co/google/gemma-4-12B-it)) — that is descriptive of evaluation, not a licence condition.

Google publishes **official QAT q4_0 GGUF** (`google/gemma-4-12B-it-qat-q4_0-gguf`): 6.98 GB for 12B, 5.15 GB for E4B, 3.35 GB for E2B. The uncomfortable finding is that **even E2B does not fit the 2 GiB artifact bound** — 3.35 GB for a nominally 2B-effective model, because the vocabulary is 262,144 and the architecture carries large per-layer embeddings. Gemma 4's smallest member is out of reach for the current budget without a re-quantisation Pulsar would have to own.

Gemma 4 is also the most KV-expensive candidate per token: 48 layers × 8 KV heads × 256 head_dim = 384 KiB/token, so 3.0 GiB of KV at 8,192 context. The card documents `sliding_window` 1024 in `config.json`, which should reduce real KV for most layers, but I did not verify how llama.cpp allocates cache for sliding-window layers, so treat 3.0 GiB as an upper bound.

Tool calling is claimed natively: "native function-calling support" and "Native support for structured tool use, enabling agentic workflows" ([card](https://huggingface.co/google/gemma-4-12B-it)).

### Llama 3.x (licence friction, no offsetting benefit in 2026)

`meta-llama/Llama-3.1-8B-Instruct` and `Llama-3.2-3B-Instruct` are `gated: manual` and declare the `llama3.1` / `llama3.2` licence. That licence is not permissive in the way Apache-2.0 is, and three clauses bear directly on shipping a downloadable pack, read from [llama.com/llama3_1/license](https://www.llama.com/llama3_1/license/):

- an **Additional Commercial Terms** clause applying where the licensee or its affiliates exceeded 700 million monthly active users in the preceding calendar month;
- a requirement to "provide a copy of this Agreement with any such Llama Materials" and to "prominently display 'Built with Llama' on a related website, user interface, blogpost, about page, or product documentation";
- a naming rule: if you use the materials "to create, train, fine tune, or otherwise improve an AI model, which is distributed or made available, you shall also include 'Llama' at the beginning of any such AI model name";
- an Acceptable Use Policy incorporated by reference.

An acceptable-use policy incorporated into the licence is the operative difference from Apache-2.0: adult roleplay content sits inside a policy Meta controls and can revise, rather than outside any use restriction. Given that Qwen3.5 and Ministral-3 offer equal or better tool-calling claims under Apache-2.0, Llama 3.x has no remaining advantage as a shipped default.

### Command-R7B (non-commercial — disqualified as a shipped default)

`CohereLabs/c4ai-command-r7b-12-2024` declares **`license: cc-by-nc-4.0`** and is `gated: auto` ([metadata](https://huggingface.co/CohereLabs/c4ai-command-r7b-12-2024)). CC-BY-NC-4.0 forbids use "primarily intended for or directed towards commercial advantage or monetary compensation". Pulsar shipping the model in a product is exactly that. This is a hard blocker for a redistributable pack, not a caveat. It also has the most mature native tool format in llama.cpp's handler list, which is a real loss — but not one worth a licence violation.

### Roleplay-tuned fine-tunes (the actual NSFW tier, and a licensing minefield)

The 2026 roleplay ecosystem has largely re-based onto `Qwen3.8-27B` and `gemma-4-12B/26B-A4B`, which is good news for licensing: those bases are Apache-2.0, so a fine-tune can legitimately inherit a permissive licence. Concretely:

- [`ReadyArt/Dark-Scarlett-27B-v2.0`](https://huggingface.co/ReadyArt/Dark-Scarlett-27B-v2.0) — declares `apache-2.0`, `base_model: Qwen/Qwen3.8-27B`. 57,636 downloads.
- [`ReadyArt/Omega-Arsenal-12B`](https://huggingface.co/ReadyArt/Omega-Arsenal-12B) — declares `apache-2.0`, `base_model: google/gemma-4-12B-it`. Only viable as a licence matter because Gemma 4 is itself Apache-2.0; the same tune on Gemma 3 would not have been.
- [`0bserverx/Qwen3.8-27B-Heretic-Abliterated-Uncensored-GGUF`](https://huggingface.co/0bserverx/Qwen3.8-27B-Heretic-Abliterated-Uncensored-GGUF) — declares `apache-2.0`, `base_model: Qwen/Qwen3.8-27B`, `base_model:quantized`. Apache-2.0 permits modification but requires a prominent notice that files were changed (§4(b)), which a redistributor must carry.

Against that, the older and still-cited roleplay lineages are either non-commercial or undeclared:

- [`TheDrummer/Cydonia-24B-v4.1`](https://huggingface.co/TheDrummer/Cydonia-24B-v4.1) — **no licence declared at all**: no `license:` tag, no `cardData.license`, and no `LICENSE` file in the repository tree. The card contains a `## Usage` heading and no licence section. Absent a licence, default copyright applies and Pulsar has no redistribution right. The same is true of [`TheDrummer/Anubis-Mini-8B-v1`](https://huggingface.co/TheDrummer/Anubis-Mini-8B-v1). These are among the most-recommended roleplay models in circulation and they are the ones Pulsar must *not* ship.
- [`Sao10K/Fimbulvetr-11B-v2`](https://huggingface.co/Sao10K/Fimbulvetr-11B-v2) — `cc-by-nc-4.0`. Non-commercial.
- [`Lewdiculous/L3-8B-Stheno-v3.2-GGUF-IQ-Imatrix`](https://huggingface.co/Lewdiculous/L3-8B-Stheno-v3.2-GGUF-IQ-Imatrix) — `cc-by-nc-4.0`.
- [`NeverSleep/Lumimaid-v0.2-8B`](https://huggingface.co/NeverSleep/Lumimaid-v0.2-8B) — `cc-by-nc-4.0`.
- [`anthracite-org/magnum-v4-12b`](https://huggingface.co/anthracite-org/magnum-v4-12b) — `apache-2.0`, built on `mistralai/Mistral-Nemo-Instruct-2407` (also Apache-2.0). One of the few older RP tunes with a clean licence, but it is a 12B and therefore outside the 2 GiB bound, and the card documents a Mistral-style `[INST]` prompt rather than any tool format.

**A mis-tagged licence worth flagging.** [`HauhauCS/Gemma4-12B-QAT-Uncensored-HauhauCS-Balanced`](https://huggingface.co/HauhauCS/Gemma4-12B-QAT-Uncensored-HauhauCS-Balanced) carries `license: gemma` and `base_model: google/gemma-4-12B-it` with `base_model:quantized`. The base is Apache-2.0; the tag says Gemma terms. The most likely explanation is a copied template, but a redistributor who reads the tag will conclude the stricter terms apply. Whatever Pulsar ships must carry the licence it *verified from the base*, not the tag the uploader left behind. This is the single strongest argument for pinning base-model-derived licences in Pulsar's own manifest rather than trusting community metadata.

Every roleplay tune in this section also inherits the "RP fine-tunes degrade at structured output" risk. None of the cards I read claims function calling, none publishes a tool-calling evaluation, and several are deliberate *de-alignments* whose stated goal is the removal of the instruction-following behaviour that schema compliance depends on. [`TheDrummer/Anubis-Mini-8B-v1`](https://huggingface.co/TheDrummer/Anubis-Mini-8B-v1) states outright that "intelligence, correctness, and problem solving are not my priority" — that is a fair description of a prose model and a disqualifying description of a tool-calling model.

## Tool calling, per candidate

**First, a scoping fact that changes the weight of this criterion.** Pulsar does not use the model's native tool-call format. The native launch passes `--json-schema-file` (verified at [`assistant_worker.rs`](../crates/pulsar-engine/src/assistant_worker.rs), the `.arg("--json-schema-file")` call), so output is grammar-constrained to a JSON schema by llama.cpp's GBNF sampler. Grammar enforcement means a model *cannot* emit syntactically invalid JSON; it can still emit a valid-but-wrong action, a hallucinated identifier, or degenerate repetition inside a string field. So for the *current* single-object contract, native tool-call formatting is not on the critical path, and the real requirements are instruction-following, field-value accuracy, and resistance to the grammar wasting the token budget on retries.

Native tool calling becomes first-class if Pulsar ever wants multi-turn or multi-call agentic behaviour, which a roleplay assistant plausibly does (scene state queries, memory lookups). On that axis:

| Candidate | Publisher claim | Primary evidence | Assessment |
| --- | --- | --- | --- |
| Qwen3.5 family | "excels in tool calling capabilities" | `--tool-call-parser qwen3_coder` documented for vLLM and SGLang in the [2B](https://huggingface.co/Qwen/Qwen3.5-2B) and [9B](https://huggingface.co/Qwen/Qwen3.5-9B) cards | Strongest permissive-lineage claim; a dedicated parser name implies a distinct wire format |
| Ministral-3 | "native function calling and JSON outputting" | `--tool-call-parser mistral` in [8B card](https://huggingface.co/mistralai/Ministral-3-8B-Instruct-2512) | Strong; Mistral's format has the longest third-party parser support |
| Gemma 4 | "native function-calling support", "Native support for structured tool use" | [12B card](https://huggingface.co/google/gemma-4-12B-it) | Claimed, but no parser name given in the card, and no equivalent of a documented serving-stack flag |
| Llama 3.1/3.2 | n/a in card | llama.cpp lists "Llama 3.1 / 3.3 ... Llama 3.2" among **native** formats ([function-calling.md at pin](https://github.com/ggml-org/llama.cpp/blob/b86d2f07542b29ab099aed34fd6b6d1b2fd4b81c/docs/function-calling.md)) | Well-supported, but the licence cost stands |
| Command-R7B | n/a | llama.cpp lists "Command R7B" as a native format | Good support, CC-BY-NC blocks shipping |
| RP fine-tunes (Cydonia, Anubis, Magnum, Fimbulvetr, Stheno, Euryale) | **no claim on any card read** | absent | Assume none. These are prose models |

On the runtime side, llama.cpp at the pin has both the legacy handler list and the newer template-differential autoparser: [`docs/autoparser.md`](https://github.com/ggml-org/llama.cpp/blob/b86d2f07542b29ab099aed34fd6b6d1b2fd4b81c/docs/autoparser.md) exists at that revision and describes deriving tool-call structure from template comparison rather than a hardcoded model list. That means the *absence* of Qwen3.5 / Gemma 4 / Ministral-3 from the `function-calling.md` native list is weaker evidence than it looks — the list predates those models, and the autoparser may still handle their templates. **I did not execute inference to confirm which parser is selected for any of the three new templates.** That is a one-command check (`llama-completion --jinja -v`) and should be done before relying on it.

## Backend availability and quantisation cliffs

**GGUF.** Published officially by Mistral (Ministral-3, all three sizes) and Google (Gemma 4 QAT q4_0, all sizes). Published by unsloth, not Qwen, for Qwen3.5/3.8. Llama 3.x and Command-R7B have no first-party GGUF in the repositories examined. Official publication is worth preferring because the release pipeline can pin a publisher revision and digest instead of owning a conversion provenance argument — which is exactly what the existing candidate notes in this repo do ([`PULSAR_LOCAL_ASSISTANT_RUNTIME_RESEARCH_2026-10-10.md`](phase-b/PULSAR_LOCAL_ASSISTANT_RUNTIME_RESEARCH_2026-10-10.md)).

**MLX.** Community conversions exist for every tier that matters: `mlx-community/Qwen3.5-2B-OptiQ-4bit`, `Qwen3.5-4B-MLX-4bit`, `Qwen3.5-9B-4bit`, `gemma-4-12B-4bit`, `gemma-4-26B-A4B-it-qat-4bit`. For a Rust binary the integration path is [`mlx-c`](https://github.com/ml-explore/mlx-c), the MIT-licensed C API for MLX (253 stars, last push 2026-10-26), consumable via FFI. Two caveats: MLX is Apple-Silicon-only, so it is an *additional* backend rather than a replacement for llama.cpp; and llama.cpp's own Metal backend already gives GPU acceleration on Apple Silicon without a second runtime. MLX therefore buys nothing Pulsar needs unless Apple-Silicon throughput is measured to be materially better, which I did not measure.

**Ollama.** `qwen3.5`, `qwen3.8`, `gemma4` and `ministral-3` all resolve in the Ollama library (HTTP 200 on each library page, checked 2026-11-04). This is useful as *evidence that a GGUF packaging exists and is widely consumed*, not as an integration path: Ollama runs as a service, which the "no separate inference service" constraint excludes.

**vLLM.** Relevant only to a cloud or power-user path. Both Qwen3.5 (`qwen3_coder`) and Ministral-3 (`mistral`) document their tool parsers there.

**Quantisation cliffs.** I found no primary source publishing a quality-vs-quantisation curve for any of these models, so any specific "below Q4 it degrades" threshold in circulation is unsourced. What the publishers actually offer is the evidence: unsloth ships down to UD-IQ2_XXS (0.77 GB for 2B, 3.19 GB for 9B) with an imatrix; Mistral ships *nothing below Q4_K_M* for Ministral-3, which is a publisher declining to distribute sub-Q4 artefacts for its own model. I would treat the absence of IQ2/IQ3 in the official Mistral GGUF tree as a meaningful signal and the unsloth IQ2/IQ3 files as unvalidated. The one hard data point this repo owns points the same way as the general folklore but for a different reason: Qwen3-1.7B Q8_0 scored *worse* than Qwen3-0.6B Q8_0 (4/7 vs 5/7) at identical settings, i.e. more parameters at higher precision did not buy intent accuracy ([`PULSAR_CREATIVE_MODEL_COMPARISON.md`](../docs/PULSAR_CREATIVE_MODEL_COMPARISON.md)). That result argues against assuming a monotonic size→quality curve, not against low-bit quantisation specifically.

## Latency: what throughput Pulsar actually needs

There is no measured throughput for any candidate on Pulsar's runtime. What can be stated with rigour is the *requirement*, derived from the interaction targets, and a *ceiling*, derived from memory bandwidth.

**Requirement.** For a 40–80 token reply to feel interactive:

- time-to-first-token well under 1 s with a ~1,500-token prompt at 2,048 context implies **prefill ≳ 1,500 tok/s**;
- a 40-token reply consumed in ~1.5 s implies **decode ≳ 25 tok/s**; 80 tokens in ~3 s implies the same 25 tok/s. Below ~15 tok/s the text arrives slower than it can be read and the interaction feels broken rather than merely slow.

**Ceiling.** Decode on CPU is bounded by memory bandwidth, because every token reads the whole weight set once. Taking 40 GB/s as a practical streaming figure for mid-range dual-channel DDR5 (peak for DDR5-5600 dual-channel is ~89.6 GB/s; practical streaming is well below peak), the ceiling is `40 / model_bytes_GB` tokens per second:

| Candidate | Q4-class weights | Bandwidth-bound decode ceiling | Meets 25 tok/s? |
| --- | ---: | ---: | --- |
| Qwen3.5-2B | 1.28 GB | ~31 tok/s | Marginal |
| Ministral-3-3B | 2.15 GB | ~19 tok/s | No |
| Qwen3.5-4B | 2.74 GB | ~15 tok/s | No |
| Ministral-3-8B | 5.20 GB | ~7.7 tok/s | No — needs GPU offload |
| Qwen3.5-9B | 5.68 GB | ~7.0 tok/s | No — needs GPU offload |
| Gemma-4-12B | 6.98 GB | ~5.7 tok/s | No — needs GPU offload |
| Ministral-3-14B | 8.24 GB | ~4.9 tok/s | No — needs GPU offload |
| Qwen3.5-27B | 16.74 GB | ~2.4 tok/s | No — needs GPU offload |

These are arithmetic ceilings, not measurements, and they are optimistic: they ignore KV reads, sampling, and batch handling. The conclusion they support is nonetheless robust — **on CPU-only decode, only the 2B tier has any chance of an interactive streaming rate, and everything at 8B and above requires GPU offload to be usable at all.** That is a stronger statement than "slower": at 5–8 tok/s an 80-token roleplay turn takes 10–16 seconds, and no amount of prompt engineering fixes it.

**The build configuration is a larger blocker than the model.** Pulsar's runtime is built with `-DGGML_NATIVE=OFF -DGGML_OPENMP=OFF -DGGML_SSE42=OFF -DGGML_AVX=OFF -DGGML_AVX2=OFF -DGGML_BMI2=OFF -DGGML_FMA=OFF -DGGML_F16C=OFF` and every AVX-512 variant off ([`build-linux-assistant-runtime.sh`](../scripts/build-linux-assistant-runtime.sh)). That is a deliberately lowest-common-denominator scalar build. On such a build the bandwidth ceiling above is unreachable by a wide margin, because dequantisation is not vectorised and nearly all of the per-token work *is* dequantisation. The repo's own measured result is consistent with this: a 1.7B model at Q8_0 blew a 45-second deadline on six-axis requests, and successful single-axis proposals took 40–44 seconds ([`PULSAR_QWEN3_17B_CANDIDATE.md`](../docs/PULSAR_QWEN3_17B_CANDIDATE.md)).

The fix is not to pick a smaller model. It is to keep `GGML_NATIVE=OFF` for portability while building ISA-specific CPU variants and dispatching at runtime: llama.cpp at the pin exposes `GGML_CPU_ALL_VARIANTS` ("build all variants of the CPU backend (requires GGML_BACKEND_DL)") and `GGML_BACKEND_DL` ([`ggml/CMakeLists.txt` at pin, lines 86 and 183](https://github.com/ggml-org/llama.cpp/blob/b86d2f07542b29ab099aed34fd6b6d1b2fd4b81c/ggml/CMakeLists.txt#L183)). That combination ships one binary that is portable *and* vectorised, at the cost of shared-library packaging. Until that is done, every model in this report will look too slow, and the measurement will be measuring the build, not the model.

## Memory budget implication for Pulsar

Verified bounds, from source rather than from prior notes:

| Bound | Value | Location |
| --- | --- | --- |
| model artifact bytes | ≤ 2 GiB | `self.model.byte_len > 2 * 1024 * 1024 * 1024` in [`assistant.rs`](../crates/pulsar-protocol/src/assistant.rs) |
| runtime artifact bytes | ≤ 128 MiB | same validation block |
| `worker_memory_bytes` | 512 MiB ..= 8 GiB | `AssistantProfile::validate`, [`assistant.rs`](../crates/pulsar-protocol/src/assistant.rs) |
| context | `-c 2048`, hardcoded | [`assistant_worker.rs`](../crates/pulsar-engine/src/assistant_worker.rs) |
| generated tokens | `-n 256`, or 1024 for creative scope | same |
| wall deadline | ≤ 120,000 ms accepted; 45,000 ms in practice | `wall_time_ms` bound; harness default |
| GPU offload | `-ngl 0` | same |

The key structural finding: **memory is not the binding constraint; artifact size is.** An 8 GiB per-child budget is already expressible and would comfortably host a 9B at Q4 (5.68 GB weights + 0.25 GiB KV + overhead ≈ 6.5 GB). What blocks the 8B–14B tier is the 2 GiB model-artifact ceiling, not the memory ceiling.

What each tier requires:

- **Stay inside 2 GiB (no protocol change):** Qwen3.5-2B up to Q8_0 (2.01 GB); Qwen3.5-4B only at IQ2/Q3 (1.76–2.11 GB); Ministral-3-3B at Q4_K_M (2,147,023,008 bytes). Nothing at 8B or above, and no Gemma 4 variant at all.
- **Reach 8B–9B at Q4 (5.2–5.7 GB):** raise the model-artifact bound to **8 GiB** and keep `worker_memory_bytes` at 8 GiB. No other protocol change needed.
- **Reach Gemma 4 12B at QAT q4_0 (6.98 GB):** artifact bound ≥ 8 GiB, and `worker_memory_bytes` raised to **12 GiB** — 6.98 GB weights + 0.77 GiB KV at 2,048 + graph/scratch/allocator overhead will not sit inside 8 GiB reliably.
- **Reach 27B at Q4 (15.4–16.7 GB):** artifact bound ~20 GiB and memory ~24 GiB; this is not a 16 GB machine and not compatible with a resident video-inference worker.
- **Keep the current 3 GiB virtual-address-space diagnostic cap:** it is a diagnostic, not a product budget, and it already fails to host a 1.7B at Q8 (1.83 GB weights + graph) within the deadline. It should not be treated as a usable ceiling for anything above 2B.

**Two changes matter more than the memory numbers.** First, `-c 2048` caps roleplay history at roughly 1,500 words of prose plus a system prompt — for an in-character assistant with a character card and accumulated scene, that is two or three exchanges. Every candidate here offers 128k–262k declared context, and the KV arithmetic above shows 8,192 context is affordable at every tier (0.25–3.0 GiB). The context constant should rise to at least 8,192 before any model comparison is meaningful. Second, `--seed 42 --temp 0` is greedy decoding. It is correct for a reproducible structured proposal and wrong for prose: at temperature 0 a roleplay model repeats its highest-probability continuation, which is what the 40–44 s runs in [`PULSAR_QWEN3_17B_CANDIDATE.md`](../docs/PULSAR_QWEN3_17B_CANDIDATE.md) were already showing. Roleplay needs two profiles: a deterministic tool-call profile (temperature 0, schema-constrained, short generation) and a sampled roleplay profile (temperature ~0.7–1.0, no schema, longer generation, its own deadline).

## Cloud options

Cloud is permitted only as an explicitly consented per-session exception, so the relevant question is not "which provider is cheapest" but "which providers permit adult fictional roleplay under their own stated policies, and at what price and context".

**Providers that prohibit it outright.** These are excluded by their own documentation, not by inference about enforcement:

- **Anthropic.** The Usage Policy states "Do Not Generate Sexually Explicit Content", defining this as "Depict or request sexual intercourse or sex acts" and "Generate content related to sexual fetishes or fantasies" ([Anthropic AUP](https://www.anthropic.com/legal/aup)). It separately prohibits sexualising minors "including in fictional settings or via roleplay" — the explicit naming of roleplay as non-exempting is worth mirroring in Pulsar's own consent UX.
- **Google.** The Generative AI Prohibited Use Policy states "Do not engage in sexually explicit, violent, hateful, or harmful activities", listing "Sexually explicit content -- for example, content created for the purpose of pornography or sexual gratification" ([Google GenAI Prohibited Use Policy](https://policies.google.com/terms/generative-ai/use-policy)).

**OpenAI.** I could not verify. `openai.com/policies/usage-policies/` returned HTTP 403 to every fetch attempt in this session, and I will not paraphrase their policy from secondary sources. This is listed in the unverified section and should be checked by a human with browser access before anyone writes OpenAI into a consent flow.

**OpenRouter (recommended).** Two facts, both primary:

1. The aggregator publishes adult-capable roleplay models with prices and context through its own models API (`https://openrouter.ai/api/v1/models`, 454 models on 2026-11-04). Sampled entries: `thedrummer/cydonia-24b-v4.1` at 131,072 context, $0.30/M in and $0.50/M out; `cognitivecomputations/dolphin-mistral-24b-venice-edition` at 128,000 context, $0.20/M in and $0.90/M out; `anthracite-org/magnum-v4-72b` at 32,768, $2.50/M and $5.00/M; `sao10k/l3.1-euryale-70b` at 131,072, $0.85/M flat; `mistralai/mistral-nemo` at 131,072, $0.029/M and $0.03/M.
2. The Terms of Service contains **no blanket sexual-content prohibition**. It prohibits content that is "unlawful, or is not in compliance with the Terms of Service for the Model or Provider you are using", and CSAM specifically ([OpenRouter Terms](https://openrouter.ai/terms)). The operative restriction is therefore the *underlying provider's* terms, which is why routing matters and why "OpenRouter permits it" is the wrong way to state it. The correct statement is: OpenRouter's own terms do not forbid it, and it will route to providers whose terms may.

That last point is a real product requirement. If Pulsar uses OpenRouter, the consent screen has to name the upstream provider, because the provider is the party whose terms govern the request — and OpenRouter documents that providers differ on logging and training use, which Pulsar's privacy model also cares about.

**Self-hostable APIs.** vLLM and llama.cpp `llama-server` both expose OpenAI-compatible endpoints, so a power user running their own instance is a third path. On the model side this reuses the Apache-2.0 local candidates above, so the licence analysis does not change.

## Safety, consent and prompt-injection features

Pulsar feeds untrusted media text — filenames, labels, retrieved captions — into the model's context, and the repo already treats that as untrusted data ([`PULSAR_LOCAL_ASSISTANT_RUNTIME_RESEARCH_2026-10-10.md`](phase-b/PULSAR_LOCAL_ASSISTANT_RUNTIME_RESEARCH_2026-10-10.md)). Three layers are available, in decreasing order of trustworthiness:

1. **Grammar and schema constraint (already shipped).** `--json-schema-file` makes syntactically invalid output impossible and, combined with the existing scope enum, makes out-of-scope action variants unreachable. This is the only layer that is a *guarantee* rather than a probability, and it is already in place. Note the repo's existing caution is correct: GBNF constrains syntax, and semantic validity — real object IDs, real revisions, allowed variants — must still be checked in Rust.
2. **A dedicated classifier model.** Candidates, all inspected for licence and packaging:
   - [`Qwen/Qwen3Guard-Stream-0.6B`](https://huggingface.co/Qwen/Qwen3Guard-Stream-0.6B) and `-4B`, `-8B` — Apache-2.0, ungated, published 2026-09-27. **No GGUF publication found** in a search of the Hub, so using it means owning a conversion.
   - [`mistralai/Shieldstral-1.0-3B`](https://huggingface.co/mistralai/Shieldstral-1.0-3B) — Apache-2.0, ungated. Community GGUF exists (e.g. `noctrex/Shieldstral-1.0-3B-GGUF`, `Abiray/Shieldstral-1.0-3B-GGUF`), which is the practical choice if a classifier ships inside the worker.
   - [`meta-llama/Llama-Prompt-Guard-2-86M`](https://huggingface.co/meta-llama/Llama-Prompt-Guard-2-86M) — `license: other`, `gated: manual`. Purpose-built for injection detection and tiny, but the licence is not permissive and no GGUF was found.
   
   At 0.6B–3B, a classifier competes for the same memory as the assistant itself. On a 16 GB machine with a video worker, a 3B classifier is not free; a 0.6B or 86M one is.
3. **Prompt-level instruction.** The weakest layer, and the one every model card implicitly relies on. Worth having, worth never trusting.

For consent features specifically, two model-independent requirements follow from the policies above and should be designed in regardless of backend: the app must treat "adult content" as an explicit, per-session, revocable consent state, and it must implement its own hard blocks for the categories every provider treats as non-negotiable (minors, including in fiction and roleplay; non-consent; real-person sexualisation). Those are not model properties and cannot be delegated to a fine-tune's "uncensored" training.

## What I could not verify

- **No throughput, latency or memory measurement exists for any candidate in this report.** The tokens-per-second figures are bandwidth arithmetic with a stated assumption (40 GB/s), not benchmarks, and they assume a vectorised build that Pulsar does not currently produce.
- **OpenAI's current usage policy.** Every fetch of `openai.com/policies/usage-policies/` returned HTTP 403. Their position on adult content is unverified here; secondary write-ups disagree with each other and I have not cited any.
- **Which llama.cpp chat parser is selected for the Qwen3.5, Gemma 4 and Ministral-3 templates at the pinned revision.** The `function-calling.md` native list at that pin does not mention them, and `autoparser.md` describes template-driven detection that may cover them. Not tested.
- **Whether `--json-schema-file` plus `--jinja` produces stable output on the new templates.** The grammar is model-agnostic, but the interaction between grammar prefill and a template that prefills a reasoning or tool block is untested here.
- **Gemma 4's real KV cost under `sliding_window: 1024`.** The 384 KiB/token figure assumes full attention on every layer and is an upper bound.
- **Any quality-versus-quantisation curve.** No publisher in this set publishes one. The "below Q4 degrades" claim is not sourced in anything I read.
- **Actual roleplay quality of any candidate.** No evaluation was run. Download counts are cited as adoption signals only, and adoption is not quality.
- **Whether Qwen3.5-2B and Qwen3.5-4B share the tool-calling competence the family card claims.** The claim is made at family level; per-size capability was not published in the cards I read.
- **The full text of the Qwen and Ministral `LICENSE` files.** I verified `license: apache-2.0` from publisher card metadata and `license_link`, but the licence files themselves returned "Entry not found" at the paths I tried, so I did not read the file bytes. Confirming that the linked `LICENSE` is unmodified Apache-2.0 text is a release-pipeline step, not something I completed.
- **Whether `mlx-c` is usable from Rust without a C++ shim.** The repository is MIT and is a C API, but I did not attempt a build.
- **Claude/Gemini/OpenAI model *versions* and pricing.** Only Anthropic's and Google's policy text was read; no pricing was fetched for either, because their policies already exclude the use case.

## Method and cleanup

Publisher metadata was read from the Hugging Face API (`/api/models/{repo}` and `?blobs=true`), model cards and `config.json` files at their current revisions on 2026-11-04; llama.cpp facts from the pinned revision `b86d2f07542b29ab099aed34fd6b6d1b2fd4b81c` (committed 2026-10-07); provider policies from the providers' own policy pages. KV and throughput arithmetic was computed in a throwaway script under `/mnt/4tb/tmp/pulsar-rp-research/`, which is scratch and is removed after this note. No model was downloaded, no inference was run, no service was started and no project file outside this document was changed.

Open decisions this note does not settle: whether roleplay is a shipped feature or a user-supplied-model feature; whether Pulsar ever redistributes a model at all versus directing users to fetch from the publisher; the acceptable installer/pack size; and the quality bar a candidate must clear, which needs the same kind of authored, held-out campaign this repo already uses for creative proposals.
