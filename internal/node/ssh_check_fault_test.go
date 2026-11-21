package node

import (
	"net"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCheckSSHReportsHandshakeFailure(t *testing.T) {
	node := nodeFixture(t)
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	done := make(chan struct{})
	t.Cleanup(func() {
		_ = listener.Close()
		<-done
	})
	go func() {
		defer close(done)
		if connection, err := listener.Accept(); err == nil {
			_ = connection.Close()
		}
	}()
	profile := protocol.SshProfile{
		ID: protocol.NewSshID(), Name: "Fixture", Host: "127.0.0.1", Port: uint16(listener.Addr().(*net.TCPAddr).Port),
		Username: "fixture", Authentication: protocol.SshAuthenticationPassword,
	}
	credential := protocol.SshCredential{Kind: "password", Data: protocol.SshCredentialPasswordPayload{Password: protocol.NewSecret("fixture")}}
	saved := commandFixture(t, node, protocol.Command{Kind: "save_ssh", Data: protocol.CommandSaveSshPayload{Profile: profile, Credential: &credential}})
	profile, ok := dispatchPayload[protocol.SshProfile](saved.Data)
	if !ok {
		t.Fatalf("SSH save output = %v", saved.Kind)
	}
	request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "check_ssh", Data: protocol.CommandCheckSshPayload{Profile: profile.ID, ExpectedRevision: profile.Revision}})
	result := dispatchFixture(t, node, node.ID(), request)
	if result.Fault == nil || result.Fault.Code != protocol.ErrorCodeUnavailable || result.Fault.SSHReason != protocol.SSHHandshakeFailed {
		t.Fatalf("check_ssh = %+v, fault = %+v", result.Output, result.Fault)
	}
}
