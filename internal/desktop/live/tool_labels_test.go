package live

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestConversationLocalizesCapturedToolLabels(t *testing.T) {
	labels := []struct {
		name, english, chinese string
	}{
		{"files_read_file", "Read file", "读取文件"},
		{"files_list_directory", "Browse directory", "浏览目录"},
		{"code_map", "Code map", "代码地图"},
		{"code_review_scope", "Review scope", "审查范围"},
		{"git_status", "Git status", "Git 状态"},
		{"code_outline", "File structure", "文件结构"},
		{"read_skill_resource", "Read skill resource", "读取技能资源"},
	}
	turn := protocol.TurnID{1}
	page := protocol.DefaultConversationPage()
	page.Runs = []protocol.ConversationRun{{Turn: turn, Kind: protocol.ConversationRunKindTask, Status: protocol.ConversationStatusCompleted}}
	for _, label := range labels {
		id := label.name
		summary := &protocol.ToolDetailSummary{Resolved: &protocol.ToolDetailResolved{
			Label: &protocol.ToolResultLocalText{Label: label.english, Locales: map[string]string{"zh-CN": label.chinese}},
		}}
		page.Entries = append(page.Entries,
			protocol.ConversationEntry{ID: id + "/call", Turn: turn, Author: "assistant", Parts: []protocol.ConversationPart{{
				Kind: "tool_call", Data: protocol.ConversationPartToolCallPayload{ID: &id, Name: id, Summary: summary},
			}}},
			protocol.ConversationEntry{ID: id + "/result", Turn: turn, Author: "tool", Parts: []protocol.ConversationPart{{
				Kind: "tool_result", Data: protocol.ConversationPartToolResultPayload{ID: &id, Name: id, Summary: summary},
			}}},
		)
	}
	state := client.ConversationState{Connected: true, Snapshot: protocol.ConversationSnapshot{Page: page}}
	for _, locale := range []string{"zh-CN", "zh_Hans_CN", "en", "fr"} {
		t.Run(locale, func(t *testing.T) {
			turns, _, _, _ := Conversation(state, locale)
			calls := map[string]conversation.Call{}
			for _, turn := range turns {
				for _, block := range turn.Reply.Blocks {
					for _, call := range block.Calls {
						calls[call.Name] = call
					}
				}
			}
			if len(calls) != len(labels) {
				t.Fatalf("got %d tool rows, want %d", len(calls), len(labels))
			}
			for _, label := range labels {
				want := label.english
				if locale == "zh-CN" || locale == "zh_Hans_CN" {
					want = label.chinese
				}
				if got := calls[label.name].Label; got != want {
					t.Errorf("%s label = %q, want %q", label.name, got, want)
				}
			}
		})
	}
}

func TestLoadedToolLabelTracksLocaleWithoutMutatingSource(t *testing.T) {
	label := &protocol.ToolResultLocalText{Label: "Read file", Locales: map[string]string{"zh-CN": "读取文件", "zh-TW": "讀取檔案", "fr": ""}}
	loaded := protocol.DefaultToolDetail()
	loaded.Resolved = &protocol.ToolDetailResolved{Label: label}
	call := conversation.Call{ID: "read", Name: "files_read_file"}
	for _, test := range []struct{ locale, want string }{
		{"zh-CN", "读取文件"},
		{"zh_Hant_HK", "讀取檔案"},
		{"fr", "Read file"},
		{"en", "Read file"},
	} {
		ProjectToolDetail(&call, nil, loaded, test.locale)
		if call.Label != test.want {
			t.Errorf("%s loaded label = %q, want %q", test.locale, call.Label, test.want)
		}
	}
	if label.Label != "Read file" || label.Locales["zh-CN"] != "读取文件" || label.Locales["zh-TW"] != "讀取檔案" {
		t.Fatal("tool label localization mutated the captured text")
	}
}
