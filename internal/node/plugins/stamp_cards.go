package plugins

import (
	"encoding/json"
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func StampCards(result json.RawMessage, node protocol.NodeID, scope protocol.PluginContext, info protocol.PluginInfo, call protocol.RequestID) json.RawMessage {
	var object map[string]json.RawMessage
	if json.Unmarshal(result, &object) != nil || object == nil {
		return result
	}
	var content map[string]json.RawMessage
	if json.Unmarshal(object["sailry_content"], &content) != nil || content == nil {
		return result
	}
	var blocks []json.RawMessage
	if json.Unmarshal(content["blocks"], &blocks) != nil || blocks == nil {
		return result
	}
	changed := false
	for index, block := range blocks {
		var fields map[string]json.RawMessage
		if json.Unmarshal(block, &fields) != nil {
			continue
		}
		var kind string
		if json.Unmarshal(fields["kind"], &kind) != nil || kind != "card" {
			continue
		}
		delete(fields, "kind")
		body, err := json.Marshal(fields)
		var draft protocol.PluginCardDraft
		valid := err == nil && json.Unmarshal(body, &draft) == nil && index < MaxCards &&
			scope.Session != nil && scope.Turn != nil && scope.Worktree != nil && info.Extension != nil &&
			draft.Fallback.Valid()
		if valid {
			valid = slices.ContainsFunc(info.Extension.Cards, func(declaration protocol.PluginCardDeclaration) bool {
				return declaration.Key == draft.Key && CardAccepts(declaration, draft.Props)
			})
		}
		if !valid {
			delete(object, "sailry_content")
			body, _ := json.Marshal(object)
			return body
		}
		summary := protocol.PluginCardSummary{
			Receipt: protocol.PluginCardReceipt{Session: *scope.Session, Turn: *scope.Turn, Call: call, Block: uint8(index)},
			Node:    node, Worktree: *scope.Worktree, Package: scope.Package, Key: draft.Key, Props: draft.Props, Fallback: draft.Fallback,
		}
		if !CardSummaryValid(summary) {
			delete(object, "sailry_content")
			body, _ := json.Marshal(object)
			return body
		}
		blocks[index], _ = json.Marshal(protocol.ToolContentBlock{Kind: "card", Data: &summary})
		changed = true
	}
	if !changed {
		return result
	}
	content["blocks"], _ = json.Marshal(blocks)
	object["sailry_content"], _ = json.Marshal(content)
	body, _ := json.Marshal(object)
	return body
}
