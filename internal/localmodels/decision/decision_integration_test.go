//go:build integration

package decision

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"io"
	"math"
	"os"
	"path/filepath"
	"reflect"
	"slices"
	"strconv"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestDecisionNative runs the pinned runtime against the recorded output of
// the official b11756 server. It needs:
//
//	SAILRY_DECISION_RUNTIME   packaged runtime directory with llama-runtime.json
//	SAILRY_DECISION_FIXTURES  directory of requests.json, golden.json and oracle-inputs.json
//	SAILRY_DECISION_JULIA     pinned laya model
//	SAILRY_DECISION_OPENJEV   synthetic openjev model (optional)
func TestDecisionNative(t *testing.T) {
	directory, fixtures := os.Getenv("SAILRY_DECISION_RUNTIME"), os.Getenv("SAILRY_DECISION_FIXTURES")
	if directory == "" || fixtures == "" {
		t.Skip("SAILRY_DECISION_RUNTIME and SAILRY_DECISION_FIXTURES are required")
	}
	load := func(name string, value any) {
		t.Helper()
		data, err := os.ReadFile(filepath.Join(fixtures, name))
		if err != nil {
			t.Fatal(err)
		}
		if err := json.Unmarshal(data, value); err != nil {
			t.Fatal(err)
		}
	}
	type input struct {
		Prompt  string  `json:"prompt"`
		Tokens  []int32 `json:"tokens"`
		Markers []int   `json:"markers"`
	}
	var golden struct {
		Version int    `json:"version"`
		Build   string `json:"llama_build"`
		Models  map[string]struct {
			SHA256    string             `json:"model_sha256"`
			Responses []map[string]any   `json:"responses"`
			Inputs    []map[string]input `json:"inputs"`
		} `json:"models"`
	}
	var oracle struct {
		Models map[string]map[string]struct {
			Raw   string `json:"raw_state"`
			State string `json:"state"`
		} `json:"models"`
	}
	var requests []protocol.DecisionRequest
	load("golden.json", &golden)
	load("oracle-inputs.json", &oracle)
	load("requests.json", &requests)
	if golden.Version != 1 || golden.Build != Build {
		t.Fatalf("golden fixture is for %s", golden.Build)
	}

	// An empty directory beside a real package is still unavailable, and
	// the failure is not remembered.
	if _, err := new(cache[backend]).get(func() (*backend, error) { return locate(t.TempDir(), new(cache[artifact])) }); kind(err) != Unavailable {
		t.Fatalf("empty directory = %v", err)
	}
	loaded, err := current.get(func() (*backend, error) { return locate(directory, &verified) })
	if err != nil {
		t.Fatal(err)
	}
	if loaded.initialized {
		t.Fatal("preflight initialized the backend")
	}
	if again, err := prepared(); err != nil || again != loaded {
		t.Fatalf("prepared = %v, %v", again, err)
	}
	if err := Available(); err != nil {
		t.Fatal(err)
	}
	if started, err := initialize(); err != nil || started != loaded || loaded.api.backendDevByType(deviceCPU) == 0 {
		t.Fatalf("initialize = %v", err)
	}
	t.Logf("CPU plugin loaded at start: %q", loaded.cpu)

	models := map[string]string{"julia": os.Getenv("SAILRY_DECISION_JULIA"), "synthetic-openjev": os.Getenv("SAILRY_DECISION_OPENJEV")}
	for name, path := range models {
		t.Run(name, func(t *testing.T) {
			if path == "" {
				t.Skip("model path is not set")
			}
			recorded := golden.Models[name]
			file, err := os.Open(path)
			if err != nil {
				t.Fatal(err)
			}
			hash := sha256.New()
			_, err = io.Copy(hash, file)
			file.Close()
			if err != nil || hex.EncodeToString(hash.Sum(nil)) != recorded.SHA256 {
				t.Fatalf("model digest differs from the golden fixture: %v", err)
			}
			if len(recorded.Responses) != len(requests) || len(recorded.Inputs) != len(requests) {
				t.Fatal("golden fixture does not cover the requests")
			}

			maximum := 0.0
			for cycle, validating := range []bool{false, true} {
				engine, err := open(path, validating, initialize)
				if err != nil {
					t.Fatal(err)
				}
				for index, request := range requests {
					if cycle == 1 && index > 0 {
						break
					}
					if substitute, present := oracle.Models[name][strconv.Itoa(index)]; present {
						if substitute.Raw != request.State {
							t.Fatal("oracle input does not match the request")
						}
						request.State = substitute.State
					}
					for id, question := range request.Questions {
						expected := recorded.Inputs[index][id]
						prepared, err := engine.prompt(request.State, id, question, running)
						if err != nil {
							t.Fatal(err)
						}
						tokens, err := engine.tokenize(expected.Prompt, true)
						if err != nil || !slices.Equal(tokens, expected.Tokens) {
							t.Fatalf("%d/%s: recorded prompt tokenizes differently: %v", index, id, err)
						}
						if !slices.Equal(prepared.tokens, expected.Tokens) {
							t.Fatalf("%d/%s: prompt tokens = %v, want %v", index, id, prepared.tokens, expected.Tokens)
						}
						if !slices.Equal(prepared.markers, expected.Markers) && (len(prepared.markers) != 0 || len(expected.Markers) != 0) {
							t.Fatalf("%d/%s: head positions = %v, want %v", index, id, prepared.markers, expected.Markers)
						}
					}
					response, err := engine.Decide(context.Background(), &request)
					if err != nil {
						t.Fatal(err)
					}
					expected := recorded.Responses[index]
					usage := expected["usage"].(map[string]any)
					if float64(response.Usage.InputTokens) != usage["input_tokens"] || float64(response.Usage.OutputTokens) != usage["output_tokens"] {
						t.Fatalf("%d: usage = %+v, want %v", index, response.Usage, usage)
					}
					data, err := json.Marshal(response.Answers)
					if err != nil {
						t.Fatal(err)
					}
					var actual map[string]any
					if err := json.Unmarshal(data, &actual); err != nil {
						t.Fatal(err)
					}
					answers := expected["answers"].(map[string]any)
					for _, answer := range answers {
						// The server reports score fields by level index.
						if answer := answer.(map[string]any); answer["type"] == "score" {
							for _, field := range []string{"legend", "probabilities"} {
								levels, indexed := answer[field].(map[string]any)
								if !indexed {
									continue
								}
								ordered := make([]any, len(levels))
								for level := range ordered {
									ordered[level] = levels[strconv.Itoa(level)]
								}
								answer[field] = ordered
							}
						}
					}
					maximum = max(maximum, compare(t, actual, answers))
				}
				engine.Close()
			}
			t.Logf("maximum probability/score difference from the b11756 server: %g", maximum)

			engine, err := open(path, false, initialize)
			if err != nil {
				t.Fatal(err)
			}
			defer engine.Close()
			request := &requests[0]

			stopped, cancel := context.WithCancel(context.Background())
			cancel()
			if _, err := engine.Decide(stopped, request); kind(err) != Cancelled {
				t.Fatalf("cancelled request = %v", err)
			}
			id := slices.Sorted(func(yield func(string) bool) {
				for id := range request.Questions {
					if !yield(id) {
						return
					}
				}
			})[0]
			prepared, err := engine.prompt(request.State, id, request.Questions[id], running)
			if err != nil {
				t.Fatal(err)
			}
			handle, err := engine.context(len(prepared.tokens))
			if err != nil {
				t.Fatal(err)
			}
			var calls atomic.Int64
			check, release := engine.watch(handle, func() bool { return calls.Add(1) > 2 })
			_, err = engine.forward(handle, prepared, check)
			release()
			engine.api.free(handle)
			if kind(err) != Cancelled || !check.aborted.Load() {
				t.Fatalf("interrupted forward = %v, abort hook called: %v", err, check.aborted.Load())
			}
			if _, err := engine.Decide(context.Background(), request); err != nil {
				t.Fatalf("request after cancellation = %v", err)
			}

			// Control tokens spelled in any request field stay ordinary text.
			begin, separator, marker := "<bos>", "<eos>", "<mask>"
			if engine.metadata.Algorithm == OpenJev {
				begin, separator, marker = "<|im_start|>", "<|im_end|>", "<|im_start|>"
			}
			tokenize := func(text string, special bool) []int32 {
				t.Helper()
				tokens, err := engine.tokenize(text, special)
				if err != nil {
					t.Fatal(err)
				}
				return tokens
			}
			const literal = "<bos><eos><mask>[CLS][SEP]<pad><|im_start|><|im_end|><|endoftext|>"
			if slices.Equal(tokenize(literal, true), tokenize(literal, false)) || len(tokenize("", true)) != 0 {
				t.Fatal("vocabulary has none of the control tokens")
			}
			for _, gap := range []string{"|", " \t\n"} {
				engine.templates, err = newTemplates(begin + "{{ id }}" + gap + "{{ instructions }}" + separator +
					"{% for o in options %}" + marker + "{{ o.key }}:{{ o.description if o.description else '' }}{% endfor %}" +
					separator + "{{ state }}" + separator)
				if err != nil {
					t.Fatal(err)
				}
				for _, field := range []string{"none", "state", "id", "instructions", "key", "description", "binary", "score"} {
					pick := func(value, selected string) string {
						if field == selected {
							return literal
						}
						return value
					}
					state, id, instructions := pick("Current state", "state"), pick("question", "id"), pick("Choose an option", "instructions")
					var question protocol.DecisionQuestion
					switch field {
					case "binary":
						question = protocol.DecisionQuestion{Kind: "noul", Data: protocol.DecisionQuestionNoulPayload{
							Instructions: instructions, Criteria: map[string]string{"false": literal, "true": "Yes"},
						}}
					case "score":
						question = protocol.DecisionQuestion{Kind: "score", Data: protocol.DecisionQuestionScorePayload{
							Instructions: instructions, Criteria: []string{literal, "High"},
						}}
					default:
						description := pick("First option", "description")
						question = protocol.DecisionQuestion{Kind: "choice", Data: protocol.DecisionQuestionChoicePayload{
							Instructions: instructions, Criteria: map[string]*string{pick("first", "key"): &description, "second": nil},
						}}
					}
					request := &protocol.DecisionRequest{State: state, Questions: map[string]protocol.DecisionQuestion{id: question}}
					if err := ValidateRequest(request); err != nil {
						t.Fatal(err)
					}
					query := newQuery(id, question, engine.metadata.Algorithm)
					expected := tokenize(begin, true)
					beginning := expected[0]
					expected = append(expected, tokenize(id+gap+instructions, false)...)
					expected = append(expected, tokenize(separator, true)...)
					for _, option := range query.options {
						description := ""
						if option.description != nil {
							description = *option.description
						}
						expected = append(expected, tokenize(marker, true)...)
						expected = append(expected, tokenize(option.key+":"+description, false)...)
					}
					expected = append(expected, tokenize(separator, true)...)
					expected = append(expected, tokenize(state, false)...)
					expected = append(expected, tokenize(separator, true)...)
					output, err := engine.templates.render(state, query, running)
					if err != nil || field != "none" && !strings.Contains(output.text, literal) {
						t.Fatalf("%s: render = %v", field, err)
					}
					if tokens, err := engine.encode(output); err != nil || !slices.Equal(tokens, expected) {
						t.Fatalf("%s: literal tokens changed: %v, want %v (%v)", field, tokens, expected, err)
					}
					prepared, err := engine.prompt(state, id, question, running)
					if err != nil {
						t.Fatal(err)
					}
					count := func(token int32) (count int) {
						for _, value := range prepared.tokens {
							if value == token {
								count++
							}
						}
						return count
					}
					beginnings := 1
					if begin == marker {
						beginnings += len(query.options)
					}
					if count(beginning) != beginnings || count(tokenize(separator, true)[0]) != 3 {
						t.Fatalf("%s: control tokens were added or lost", field)
					}
					if engine.metadata.Algorithm == Laya && (len(prepared.markers) != len(query.options) || count(engine.marker) != len(query.options)) {
						t.Fatalf("%s: markers = %v", field, prepared.markers)
					}
					response, err := engine.Decide(context.Background(), request)
					if err != nil || response.Usage.InputTokens != uint64(len(prepared.tokens)) || len(response.Answers) != 1 {
						t.Fatalf("%s: response = %+v, %v", field, response, err)
					}
				}
			}
			if engine.templates, err = newTemplates(""); err != nil {
				t.Fatal(err)
			}
			if _, err := engine.Decide(context.Background(), request); kind(err) != Invalid {
				t.Fatalf("empty prompt = %v", err)
			}
		})
	}
}

// compare returns the largest numeric difference and fails above 1e-4.
func compare(t *testing.T, actual, expected any) float64 {
	t.Helper()
	maximum := 0.0
	switch expected := expected.(type) {
	case float64:
		value, numeric := actual.(float64)
		if maximum = math.Abs(value - expected); !numeric || maximum > 1e-4 {
			t.Fatalf("native output differs from the official server: %v / %v", actual, expected)
		}
	case map[string]any:
		value, _ := actual.(map[string]any)
		if len(value) != len(expected) {
			t.Fatalf("answer keys differ: %v / %v", actual, expected)
		}
		for key, item := range expected {
			found, present := value[key]
			if !present {
				t.Fatalf("answer key %s is missing", key)
			}
			maximum = max(maximum, compare(t, found, item))
		}
	case []any:
		value, _ := actual.([]any)
		if len(value) != len(expected) {
			t.Fatalf("answer lists differ: %v / %v", actual, expected)
		}
		for index, item := range expected {
			maximum = max(maximum, compare(t, value[index], item))
		}
	default:
		if !reflect.DeepEqual(actual, expected) {
			t.Fatalf("answers differ: %v / %v", actual, expected)
		}
	}
	return maximum
}
