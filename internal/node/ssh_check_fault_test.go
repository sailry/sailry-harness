package node

import (
	"context"
	"errors"
	"sync/atomic"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCheckSSHReportsHandshakeFailure(t *testing.T) {
	t.Parallel()
	var calls atomic.Int32
	var request protocol.Request
	failure := &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture SSH handshake failed", SSHReason: protocol.SSHHandshakeFailed}
	assembly := fixtureModuleAssembly("ssh")
	assembly.SSH = func(options moduleapi.SSHServiceOptions) moduleapi.SSHService {
		return &serviceTestSSH{execute: func(ctx context.Context, caller protocol.NodeID, received protocol.Request) (protocol.Output, error) {
			calls.Add(1)
			if caller != request.Target || received.ID != request.ID || received.Command.Kind != "check_ssh" {
				return protocol.Output{}, errors.New("SSH service received another command or caller")
			}
			input := received.Command.Data.(*protocol.CommandCheckSshPayload)
			profile, credential, err := options.ReadSSH(ctx, input.Profile, input.ExpectedRevision)
			if err != nil {
				return protocol.Output{}, err
			}
			password, ok := dispatchPayload[protocol.SshCredentialPasswordPayload](credential.Data)
			if profile.ID != input.Profile || profile.Revision != input.ExpectedRevision || !ok || password.Password.Expose() != "fixture" {
				return protocol.Output{}, errors.New("SSH service did not retain the saved profile and credential")
			}
			return protocol.Output{}, failure
		}}
	}
	node := serviceTestNode(t, assembly)
	profile := protocol.SshProfile{
		ID: protocol.NewSshID(), Name: "Fixture", Host: "127.0.0.1", Port: 22,
		Username: "fixture", Authentication: protocol.SshAuthenticationPassword,
	}
	credential := protocol.SshCredential{Kind: "password", Data: protocol.SshCredentialPasswordPayload{Password: protocol.NewSecret("fixture")}}
	saved := commandFixture(t, node, protocol.Command{Kind: "save_ssh", Data: protocol.CommandSaveSshPayload{Profile: profile, Credential: &credential}})
	profile, ok := dispatchPayload[protocol.SshProfile](saved.Data)
	if !ok {
		t.Fatalf("SSH save output = %v", saved.Kind)
	}
	request = protocol.NewRequest(node.ID(), protocol.Command{Kind: "check_ssh", Data: protocol.CommandCheckSshPayload{Profile: profile.ID, ExpectedRevision: profile.Revision}})
	for range 2 {
		result := dispatchFixture(t, node, node.ID(), request)
		if result.Fault == nil || *result.Fault != *failure {
			t.Fatalf("check_ssh = %+v, fault = %+v", result.Output, result.Fault)
		}
	}
	if calls.Load() != 1 {
		t.Fatalf("SSH check replay executed the service %d times", calls.Load())
	}
}
