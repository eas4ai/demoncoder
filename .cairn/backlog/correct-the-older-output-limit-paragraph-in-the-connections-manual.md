# Correct the older output-limit paragraph in the connections manual

Surfaced from: SET-008
Captured: 2026-09-08T17:04:41.660Z

The older Connections and authentication paragraph still states that Anthropic always requests 4096 tokens and neither native output setting has a CLI override. The output-limit implementation and the CLI/manual tables already support discovered limits and --max-output-tokens. This predates provider-agent-settings and does not change its confirmed provider/role workflow; reconcile that older paragraph under the output-limit documentation scope.

Resolved in release-followups on 2026-09-09: the paragraph now describes model-discovered Anthropic limits, OpenAI provider defaults, saved max_output_tokens and the CLI override. Existing output-limit checks and final review verify the correction.
