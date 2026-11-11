package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"slices"
	"strings"
	"unicode"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func approvalPrefixWords(command string) []string {
	if command == "" {
		return nil
	}
	for _, c := range command {
		if unicode.IsControl(c) && c != '\t' || unicode.IsSpace(c) && c != ' ' && c != '\t' || strings.ContainsRune(";&|<>$`\\'\"(){}[]*?~#!", c) {
			return nil
		}
	}
	words := strings.FieldsFunc(command, func(c rune) bool { return c == ' ' || c == '\t' })
	if len(words) == 0 || strings.ContainsRune(words[0], '=') || strings.HasPrefix(words[0], "-") {
		return nil
	}
	return words
}
func approvalPrefixMatches(command, prefix string) bool {
	words, leading := approvalPrefixWords(command), approvalPrefixWords(prefix)
	return len(words) > 0 && len(leading) > 0 && len(words) >= len(leading) && slices.Equal(words[:len(leading)], leading)
}
func approvalCommand(binding *ApprovalBinding, arguments json.RawMessage) *string {
	if binding == nil || binding.Operation == nil || *binding.Operation != protocol.ToolOperationRunCommand {
		return nil
	}
	var value struct {
		Command *string `json:"command"`
	}
	if json.Unmarshal(arguments, &value) != nil {
		return nil
	}
	return value.Command
}
func readApprovalBinding(ctx context.Context, db *sql.Conn, id protocol.ApprovalID) (*ApprovalBinding, error) {
	var body []byte
	if err := db.QueryRowContext(ctx, "SELECT binding FROM agent_approvals WHERE id=?1", id.String()).Scan(&body); err != nil {
		return nil, err
	}
	var binding *ApprovalBinding
	err := json.Unmarshal(body, &binding)
	return binding, err
}
func approvalRequest(ctx context.Context, db *sql.Conn, value protocol.ConversationApproval) (*canonical.ToolRequest, error) {
	record, err := readBody[canonical.Record](ctx, db, "SELECT body FROM agent_events WHERE turn=?1 AND id=?2", value.Turn.String(), value.Entry)
	if err != nil {
		return nil, err
	}
	if record != nil {
		if m := record.Message(); m != nil {
			for _, block := range m.Content {
				if block.Index == value.Index {
					if call, ok := block.Data.(*canonical.ToolRequest); ok {
						return call, nil
					}
				}
			}
		}
	}
	return nil, invalid("approval has no canonical tool call")
}
func applySessionApproval(ctx context.Context, db *sql.Conn, source protocol.ConversationRun, binding ApprovalBinding, resource *protocol.ConnectionResource) error {
	bound, err := encode(binding)
	if err != nil {
		return err
	}
	approvals, err := readBodies[protocol.ConversationApproval](ctx, db, "SELECT body FROM agent_approvals WHERE json_extract(body,'$.session')=?1 AND binding=?2 AND json_extract(body,'$.state')='pending' ORDER BY rowid", source.Session.String(), bound)
	if err != nil {
		return err
	}
	for _, value := range approvals {
		if !value.CanRemember || value.Source != protocol.ConversationApprovalSourceUser || value.Startup != nil || value.Ssh != nil {
			continue
		}
		run, e := ReadAgentRun(ctx, db, value.Turn)
		if e != nil {
			return e
		}
		if run.Kind != protocol.ConversationRunKindTask || run.Status != protocol.ConversationStatusRunning || run.Session != source.Session || run.Worktree != source.Worktree {
			continue
		}
		capture, e := readTurnCapture(ctx, db, value.Turn)
		if e != nil {
			return e
		}
		if !sameAssistant(capture.Config.Resource, resource) {
			continue
		}
		call, e := approvalRequest(ctx, db, value)
		if e != nil {
			return e
		}
		if binding.Tool != call.Name || !approvalOwnerAdmitted(capture.Plugins, binding.Owner) {
			return fault(protocol.ErrorCodePermissionDenied, "approval owner is outside the admitted tool catalog")
		}
		value.State = protocol.ConversationApprovalStateApproved
		value.Source = protocol.ConversationApprovalSourceSessionRule
		value.Choice = &protocol.ConversationApprovalChoice{Kind: "session"}
		if err = putApproval(ctx, db, value); err != nil {
			return err
		}
	}
	return nil
}
