package link

import (
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol/wire"
)

func TestControllerUsesInstalledMobileVersion(t *testing.T) {
	controller, err := OpenControllerWithVersion(t.Context(), filepath.Join(t.TempDir(), "controller"), NetworkScope{Bind: "127.0.0.1:0"}, "0.2.14")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = controller.Close() })
	request := controller.envelope(wire.Call{})
	reply := controller.endpointReply()
	if request.SoftwareVersion == nil || *request.SoftwareVersion != "0.2.14" || reply.SoftwareVersion == nil || *reply.SoftwareVersion != "0.2.14" {
		t.Fatalf("controller advertised another product build: request %+v, reply %+v", request.SoftwareVersion, reply.SoftwareVersion)
	}
}
