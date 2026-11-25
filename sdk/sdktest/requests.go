package sdktest

import (
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	native "github.com/sailry/sailry-harness/sdk/native/node"
)

type LinkAdmission = link.Admission

type NetworkScope = link.NetworkScope

type RequestState = client.Settled

const (
	RequestFailed  = client.RequestFailed
	RequestUnknown = client.RequestUnknown
)

func NewLocalClient(node *native.Node) *client.Client {
	return client.New(link.NewLocal(node.ID(), node.ID(), node))
}
