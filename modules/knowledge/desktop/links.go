package desktop

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/sdk/desktop/native"
)

func registerLinks(assembly *native.Assembly) {
	assembly.RegisterLinkHandler(native.LinkRoute{Kind: native.LinkScheme, Name: "sailry-knowledge"}, openCitation)
}

func openCitation(c *ui.Context, target native.LinkTarget, actions native.LinkActions) error {
	source, ok := shell.ParseSource(target.URL)
	if !ok {
		kit.Toast(c, kit.Failed, i18n.T(c, "knowledge_citation_invalid"))
		return nil
	}
	if actions.OpenKnowledge == nil {
		return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "knowledge viewer is unavailable"}
	}
	actions.OpenKnowledge(c, source)
	return nil
}
