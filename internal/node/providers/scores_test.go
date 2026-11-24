package providers

import "testing"

func TestNormalizeScoreNameAndEffortHint(t *testing.T) {
	t.Parallel()
	for input, want := range map[string]string{
		"vendor/Model_A-2026-01-02":            "model-a",
		"proxy/google/gemini-4-neon":           "gemini-4-neon",
		"Gemini 3.8 Flash":                     "gemini-3-8-flash",
		"Claude Sonnet 5.5":                    "claude-sonnet-5-5",
		"Claude Haiku 5.5":                     "claude-haiku-5-5",
		"baidu/ernie-5.5-flash-preview:free":   "ernie-5-5-flash-preview",
		"agy-cli/claude-sonnet-5-5-high":       "claude-sonnet-5-5-high",
		"agy-cli/gemini-3.8-flash-latest":      "gemini-3-8-flash",
		"doubao-seed-2-1-pro-260915":           "doubao-seed-2-1-pro",
		"  Kimi   K3.1   Pro  ":                "kimi-k3-1-pro",
		"anthropic/claude-sonnet-5.5-20261119": "claude-sonnet-5-5",
		"DeepSeek V4.1 Flash (1017)":           "deepseek-v4-1-flash-(1017)",
		"qwen4-max":                            "qwen4-max",
	} {
		if got := NormalizeScoreName(input); got != want {
			t.Errorf("NormalizeScoreName(%q) = %q, want %q", input, got, want)
		}
	}
	for input, want := range map[string]string{
		"agy-cli/claude-sonnet-5-5-low":     "low",
		"agy-cli/claude-sonnet-5-5-medium":  "medium",
		"agy-cli/claude-sonnet-5-5-high":    "high",
		"agy-cli/claude-sonnet-5-5-xhigh":   "xhigh",
		"agy-cli/claude-sonnet-5-5-minimal": "minimal",
		"qwen4-max":                         "",
		"agy-cli/gemini-4-neon":             "",
	} {
		if got := ScoreEffortHint(input); got != want {
			t.Errorf("ScoreEffortHint(%q) = %q, want %q", input, got, want)
		}
	}
}

func TestResolveScoreAliasesAndEffortSuffixes(t *testing.T) {
	t.Parallel()
	models := []string{
		"anthropic/claude-sonnet-5.5",
		"anthropic/haiku-5.5",
		"baidu/ernie-5.5-flash-preview:free",
		"gemini-3.8-flash-high",
		"google/gemini-4-neon",
		"qwen4",
	}
	// Include legacy DB aliases that still contain spaces to confirm read-time normalization.
	aliases := map[string]string{
		"claude sonnet 5-5": "anthropic/claude-sonnet-5.5",
		"claude haiku 5-5":  "anthropic/haiku-5.5",
		"gemini 3-8 flash":  "gemini-3.8-flash-high",
		"gemini 4 neon":     "google/gemini-4-neon",
	}
	for candidate, want := range map[string]string{
		"agy-cli/gemini-4-neon":            "google/gemini-4-neon",
		"proxy/google/gemini-4-neon":       "google/gemini-4-neon",
		"agy-cli/gemini-3.8-flash":         "gemini-3.8-flash-high",
		"agy-cli/claude-sonnet-5-5-high":   "anthropic/claude-sonnet-5.5",
		"agy-cli/claude-sonnet-5-5-low":    "anthropic/claude-sonnet-5.5",
		"agy-cli/claude-sonnet-5-5-medium": "anthropic/claude-sonnet-5.5",
		"claude-haiku-5-5":                 "anthropic/haiku-5.5",
		"claude-haiku-5-5-high":            "anthropic/haiku-5.5",
		"ernie-5.5-flash-preview":          "baidu/ernie-5.5-flash-preview:free",
		"agy-cli/gpt-oss-120b":             "",
		"agy-cli/gpt-oss-120b-medium":      "",
		// "-max" is often a model tier rather than an effort suffix and must not fall back to "qwen4".
		"qwen4-max": "",
	} {
		got := ResolveScore([]string{candidate}, models, aliases)
		gotStr := ""
		if got != nil {
			gotStr = *got
		}
		if gotStr != want {
			t.Errorf("ResolveScore(%q) = %q, want %q", candidate, gotStr, want)
		}
	}
}

func TestParseScoresRespectsListingReplaces(t *testing.T) {
	t.Parallel()
	// Candidate "z-replacement" sorts after "a-old", yet must win via listing_replaces.
	raw := []byte(`{
		"snapshot": {
			"batch_id": "b1",
			"published_at": "2026-11-23T00:00:00Z",
			"entries": [
				{"candidate_id": "a-old", "model_id": "gemini-4-carbon", "reasoning_effort": "high", "overall_score": 64.98, "backend_score": 55, "frontend_score": 71, "knowledge_score": 95, "aesthetic_score": 41, "maintainability_score": 51},
				{"candidate_id": "z-replacement", "model_id": "gemini-4-carbon", "reasoning_effort": "high", "listing_replaces": "a-old", "overall_score": 84.03, "backend_score": 91, "frontend_score": 72, "knowledge_score": 95, "aesthetic_score": 78, "maintainability_score": 68}
			]
		},
		"displayNames": {"gemini-4-carbon": "Gemini 4 Carbon"}
	}`)
	dataset, err := ParseScores(raw)
	if err != nil {
		t.Fatal(err)
	}
	if len(dataset.Entries) != 1 || dataset.Entries[0].Score.Overall != 84.03 {
		t.Fatalf("unexpected entries: %+v", dataset.Entries)
	}
	if got := dataset.Aliases["gemini-4-carbon"]; got != "gemini-4-carbon" {
		t.Fatalf("normalized alias = %q, want gemini-4-carbon", got)
	}
}
