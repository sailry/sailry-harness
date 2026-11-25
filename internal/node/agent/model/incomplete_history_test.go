package model

import (
	"encoding/json"
	"reflect"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestAnthropicHistoryKeepsOnlySerializableThinking(t *testing.T) {
	t.Parallel()
	binding := replayFixtureBinding()
	turn := protocol.NewTurnID()
	user := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, "Initial request")), time.Time{})
	initial := Request{System: "Stable instructions", Records: []canonical.Record{user}}
	first, err := BuildCall(binding, initial)
	if err != nil {
		t.Fatal(err)
	}
	signed, empty := "signed-thinking", ""
	for _, finish := range []canonical.Finish{canonical.Completed, canonical.Failed, canonical.Interrupted} {
		for _, signature := range []*string{nil, &empty, &signed} {
			for _, withText := range []bool{false, true} {
				name := string(finish) + "/unsigned"
				if signature != nil {
					name = string(finish) + "/signature_" + *signature
				}
				if withText {
					name += "/partial_text"
				}
				t.Run(name, func(t *testing.T) {
					message := canonical.NewMessage(canonical.Assistant, canonical.Block{Index: 0, Kind: "thinking", Data: &canonical.Thinking{Text: "Partial reasoning", Signature: signature}})
					message.Finish = &finish
					if withText {
						message.Content = append(message.Content, canonical.Text(1, "Keep this partial answer"))
					}
					assistant := canonical.NewRecord(turn, "message", message, time.Time{})
					assistant.Metadata.Runtime = &canonical.Runtime{AgentVisible: true, Inference: &canonical.Inference{Provider: "sailry:" + binding.Provider.ID.String(), RequestedModel: binding.Config.Model, ReplayPrefix: ReplayPrefix(first)}}
					next := canonical.NewRecord(protocol.NewTurnID(), "message", canonical.NewMessage(canonical.User, canonical.Text(0, "Continue")), time.Time{})
					records := []canonical.Record{user, assistant, next}
					before, _ := json.Marshal(records)
					for _, build := range []func(Binding, Request) (fantasy.Call, error){BuildCall, BuildProjectedCall, BuildVisibleProjectedCall} {
						request := initial
						request.Records = VisibleAnsweredRecords(records)
						call, err := build(binding, request)
						if err != nil {
							t.Fatal(err)
						}
						body, err := requestJSON(transportConfig{Kind: "anthropic", Model: binding.Config.Model}, call)
						if err != nil {
							t.Fatal(err)
						}
						var reasoning, texts int
						for _, raw := range wireArray(body["messages"]) {
							message := wireObject(raw)
							content := wireArray(message["content"])
							if len(content) == 0 {
								t.Fatal("history produced an empty wire message")
							}
							for _, raw := range content {
								block := wireObject(raw)
								if block["type"] == "thinking" {
									reasoning++
								}
								if block["text"] == "Keep this partial answer" {
									texts++
								}
							}
						}
						wantReasoning, wantTexts := 0, 0
						if signature != nil && *signature != "" {
							wantReasoning = 1
						}
						if withText {
							wantTexts = 1
						}
						if reasoning != wantReasoning || texts != wantTexts {
							t.Fatalf("projection lost valid history: reasoning=%d text=%d", reasoning, texts)
						}
					}
					after, _ := json.Marshal(records)
					if !reflect.DeepEqual(before, after) {
						t.Fatal("projection changed canonical history")
					}
				})
			}
		}
	}
}
