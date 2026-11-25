package desktop

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/sdk/desktop/native"
)

func registerLinks(assembly *native.Assembly) {
	assembly.RegisterLinkHandler(native.LinkRoute{Kind: native.LinkReference, Name: "plugin"}, openPackageReference)
}

func openPackageReference(c *ui.Context, target native.LinkTarget, actions native.LinkActions) error {
	name, ok := protocol.DataAs[string](target.Reference.Target.Data)
	if !ok || name == "" || actions.OpenPackageDetails == nil {
		return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "package details are unavailable"}
	}
	actions.OpenPackageDetails(c, name)
	return nil
}
