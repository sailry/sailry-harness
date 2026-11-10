package store

import (
	"context"
	"database/sql"
	"errors"
	"slices"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func CapturedPluginCard(ctx context.Context, db *sql.Conn, receipt protocol.PluginCardReceipt) (protocol.PluginInfo, protocol.PluginCardSummary, error) {
	denied := func() (protocol.PluginInfo, protocol.PluginCardSummary, error) {
		return protocol.PluginInfo{}, protocol.PluginCardSummary{}, fault(protocol.ErrorCodePermissionDenied, "card is no longer available")
	}
	session, err := ReadSession(ctx, db, receipt.Session)
	if err != nil {
		return protocol.PluginInfo{}, protocol.PluginCardSummary{}, err
	}
	if session.Delegation != nil {
		return denied()
	}
	run, err := visibleAgentRun(ctx, db, receipt.Session, receipt.Turn)
	if err != nil {
		return protocol.PluginInfo{}, protocol.PluginCardSummary{}, err
	}
	if run.Origin != nil || run.Session != receipt.Session ||
		!slices.Contains([]protocol.ConversationStatus{protocol.ConversationStatusRunning, protocol.ConversationStatusCompleted, protocol.ConversationStatusFailed}, run.Status) {
		return denied()
	}
	var latest string
	if err := db.QueryRowContext(ctx, "SELECT h.turn FROM conversation_turns h JOIN turns t ON t.id=h.turn WHERE h.session=?1 ORDER BY t.rowid DESC LIMIT 1", receipt.Session.String()).Scan(&latest); err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return denied()
		}
		return protocol.PluginInfo{}, protocol.PluginCardSummary{}, err
	}
	if latest != receipt.Turn.String() {
		return denied()
	}
	capture, err := readTurnCapture(ctx, db, receipt.Turn)
	if err != nil {
		return protocol.PluginInfo{}, protocol.PluginCardSummary{}, err
	}
	records, err := readBodies[canonical.Record](ctx, db, "SELECT e.body FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 AND e.turn=?2 ORDER BY e.sequence", receipt.Session.String(), receipt.Turn.String())
	if err != nil {
		return protocol.PluginInfo{}, protocol.PluginCardSummary{}, err
	}
	type callKey struct{ branch, id, name string }
	calls := map[callKey]bool{}
	for _, record := range records {
		message := record.Message()
		if message == nil {
			continue
		}
		for _, block := range message.Content {
			switch content := block.Data.(type) {
			case *canonical.ToolRequest:
				calls[callKey{record.Provenance.Branch, content.ID, content.Name}] = true
			case *canonical.ToolResponse:
				if !calls[callKey{record.Provenance.Branch, content.ID, content.Name}] {
					continue
				}
				body, err := HydrateCanonicalPayload(ctx, db, content.Result)
				if err != nil {
					return protocol.PluginInfo{}, protocol.PluginCardSummary{}, err
				}
				raw := rawAgentTool{Result: &body, Format: content.Format, Presentation: protocol.ToolPresentationContent}
				presentation := raw.content()
				if presentation == nil || int(receipt.Block) >= len(presentation.Blocks) {
					continue
				}
				card := presentation.Blocks[receipt.Block]
				if card.Kind != "card" {
					continue
				}
				summary, ok := card.Data.(*protocol.PluginCardSummary)
				if !ok || !plugins.CardSummaryValid(*summary) || summary.Receipt != receipt || summary.Worktree != run.Worktree || !slices.Contains(capture.Plugins, summary.Package) {
					continue
				}
				info, err := ReadPluginVersion(ctx, db, summary.Package)
				if err != nil {
					return protocol.PluginInfo{}, protocol.PluginCardSummary{}, err
				}
				if info.Extension == nil || !slices.ContainsFunc(info.Extension.Tools, func(tool protocol.ToolDeclaration) bool {
					return tool.Handler != nil && plugins.ToolAlias(info.Summary.Name, tool.Name) == content.Name
				}) || plugins.ToolCallIdentity(receipt.Turn, summary.Package, content.Name, content.ID) != receipt.Call ||
					!slices.ContainsFunc(info.Extension.Cards, func(declaration protocol.PluginCardDeclaration) bool {
						return declaration.Key == summary.Key && plugins.CardAccepts(declaration, summary.Props)
					}) {
					continue
				}
				current, err := ReadPlugin(ctx, db, summary.Package.Name)
				if err != nil {
					return protocol.PluginInfo{}, protocol.PluginCardSummary{}, err
				}
				if current == nil || !current.Summary.Enabled || current.Summary.Compatibility.Kind != "compatible" || current.Summary.Digest != summary.Package.Digest {
					return denied()
				}
				return *current, *summary, nil
			}
		}
	}
	return denied()
}
