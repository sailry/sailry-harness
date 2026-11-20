package wire

import (
	"encoding/json"
	"errors"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const NodeALPN = "sailry/node/1"
const PairALPN = "sailry/pair/1"

type CallKind string

const (
	CallInspect         CallKind = "inspect"
	CallOpenStream      CallKind = "open_stream"
	CallCommand         CallKind = "command"
	CallSubscribe       CallKind = "subscribe"
	CallResource        CallKind = "resource"
	CallScopedKnowledge CallKind = "scoped_knowledge"
)

type Call struct {
	Kind            CallKind
	Stream          *protocol.StreamID
	Request         *protocol.Request
	Topic           *protocol.Topic
	Resource        *protocol.ResourceCall
	ScopedKnowledge *protocol.ScopedKnowledgeCall
}

var callVariants = map[string]func() any{
	"inspect":          nil,
	"open_stream":      func() any { return new(protocol.StreamID) },
	"command":          func() any { return new(protocol.Request) },
	"subscribe":        func() any { return new(protocol.Topic) },
	"resource":         func() any { return new(protocol.ResourceCall) },
	"scoped_knowledge": func() any { return new(protocol.ScopedKnowledgeCall) },
}

func (call Call) MarshalJSON() ([]byte, error) {
	var payload any
	switch call.Kind {
	case CallOpenStream:
		payload = call.Stream
	case CallCommand:
		payload = call.Request
	case CallSubscribe:
		payload = call.Topic
	case CallResource:
		payload = call.Resource
	case CallScopedKnowledge:
		payload = call.ScopedKnowledge
	}
	return protocol.MarshalUnion("kind", "data", string(call.Kind), payload, callVariants)
}

func (call *Call) UnmarshalJSON(data []byte) error { return protocol.Decode(data, call) }

func (call *Call) DecodeJSON(decoder *protocol.Decoder) error {
	kind, payload, err := decoder.Union("kind", "data", callVariants, false)
	if err != nil {
		return err
	}
	decoded := Call{Kind: CallKind(kind)}
	switch decoded.Kind {
	case CallOpenStream:
		decoded.Stream = payload.(*protocol.StreamID)
	case CallCommand:
		decoded.Request = payload.(*protocol.Request)
	case CallSubscribe:
		decoded.Topic = payload.(*protocol.Topic)
	case CallResource:
		decoded.Resource = payload.(*protocol.ResourceCall)
	case CallScopedKnowledge:
		decoded.ScopedKnowledge = payload.(*protocol.ScopedKnowledgeCall)
	}
	*call = decoded
	return nil
}

type Envelope struct {
	SoftwareVersion *string
	Protocol        *uint16
	Zstd            bool
	Call            Call
	HasNode         *bool
	Target          *protocol.NodeID
	// RawData preserves a relayed business payload, including peer fields this
	// build does not know. Only transport metadata is rewritten at the next hop.
	RawData json.RawMessage
	// Undecoded is set by decoding alone: the identifier of a command request
	// whose version, identifier and target decoded and whose command did not,
	// because this build does not declare its kind or cannot read its
	// payload. Call is then a command without a Request. The receiver can
	// answer such a request on its stream; it cannot run it.
	Undecoded *protocol.RequestID
}

func (envelope Envelope) MarshalJSON() ([]byte, error) {
	metadata, err := json.Marshal(struct {
		SoftwareVersion *string          `json:"software_version"`
		Protocol        *uint16          `json:"protocol"`
		Zstd            bool             `json:"zstd"`
		HasNode         *bool            `json:"has_node,omitempty"`
		Target          *protocol.NodeID `json:"target,omitempty"`
	}{envelope.SoftwareVersion, envelope.Protocol, envelope.Zstd, envelope.HasNode, envelope.Target})
	if err != nil {
		return nil, err
	}
	var call []byte
	if envelope.RawData != nil {
		call, err = json.Marshal(struct {
			Kind CallKind        `json:"kind"`
			Data json.RawMessage `json:"data"`
		}{envelope.Call.Kind, envelope.RawData})
		if err != nil {
			return nil, err
		}
	} else {
		call, err = json.Marshal(envelope.Call)
		if err != nil {
			return nil, err
		}
	}
	flattened := append(metadata[:len(metadata)-1], ',')
	return append(flattened, call[1:]...), nil
}

func (envelope *Envelope) UnmarshalJSON(data []byte) error {
	var metadata struct {
		SoftwareVersion *string          `json:"software_version"`
		Protocol        *uint16          `json:"protocol"`
		Zstd            bool             `json:"zstd"`
		HasNode         *bool            `json:"has_node,omitempty"`
		Target          *protocol.NodeID `json:"target,omitempty"`
		Data            json.RawMessage  `json:"data"`
	}
	if err := protocol.DecodeObject(data, &metadata, nil, false); err != nil {
		return err
	}
	var call Call
	var undecoded *protocol.RequestID
	if err := call.UnmarshalJSON(data); err != nil {
		id, found := undecodedCommand(data)
		if !found {
			return err
		}
		call, undecoded = Call{Kind: CallCommand}, &id
	}
	*envelope = Envelope{
		SoftwareVersion: metadata.SoftwareVersion, Protocol: metadata.Protocol,
		Zstd: metadata.Zstd, Call: call, HasNode: metadata.HasNode,
		Target: metadata.Target, RawData: append(json.RawMessage(nil), metadata.Data...),
		Undecoded: undecoded,
	}
	return nil
}

// undecodedCommand reads the identity of a command call that failed to
// decode. It succeeds only when everything but the command is valid, checked
// exactly, so the failure lies in the command itself.
func undecodedCommand(data []byte) (protocol.RequestID, bool) {
	var call struct {
		Kind string          `json:"kind"`
		Data json.RawMessage `json:"data"`
	}
	if protocol.DecodeObject(data, &call, []string{"kind", "data"}, false) != nil || call.Kind != string(CallCommand) {
		return protocol.RequestID{}, false
	}
	var request struct {
		Version uint16                  `json:"version"`
		ID      protocol.RequestID      `json:"id"`
		Target  protocol.NodeID         `json:"target"`
		Command json.RawMessage         `json:"command"`
		Plugin  *protocol.PluginContext `json:"plugin"`
	}
	if protocol.DecodeObject(call.Data, &request, []string{"version", "id", "target", "command"}, false) != nil {
		return protocol.RequestID{}, false
	}
	return request.ID, true
}

type ReplyKind string

const (
	ReplyEndpoint      ReplyKind = "endpoint"
	ReplyStreamOpened  ReplyKind = "stream_opened"
	ReplySubscribed    ReplyKind = "subscribed"
	ReplyReceipt       ReplyKind = "receipt"
	ReplyRejected      ReplyKind = "rejected"
	ReplyCompleted     ReplyKind = "completed"
	ReplyUpdate        ReplyKind = "update"
	ReplySnapshotStart ReplyKind = "snapshot_start"
	ReplySnapshotPage  ReplyKind = "snapshot_page"
)

const MaxLANListeners = 8

type lanListeners []string

// This optional hint must not turn a large array into an unbounded allocation
// or prevent an otherwise valid inspection from completing.
func (listeners *lanListeners) DecodeJSON(decoder *protocol.Decoder) error {
	var raw json.RawMessage
	if err := decoder.Value(&raw); err != nil {
		return err
	}
	*listeners = nil
	if len(raw) <= 4096 {
		var values []string
		if json.Unmarshal(raw, &values) == nil && len(values) <= MaxLANListeners {
			*listeners = values
		}
	}
	return nil
}

type Endpoint struct {
	SoftwareVersion *string      `json:"software_version"`
	Protocol        *uint16      `json:"protocol"`
	Zstd            bool         `json:"zstd"`
	Execution       bool         `json:"execution"`
	Name            *string      `json:"name"`
	Platform        string       `json:"platform"`
	HasNode         *bool        `json:"has_node,omitempty"`
	LANListeners    lanListeners `json:"lan_listeners,omitempty"`
}

func (endpoint *Endpoint) UnmarshalJSON(data []byte) error {
	type endpointData Endpoint
	var decoded endpointData
	if err := protocol.DecodeObject(data, &decoded, []string{"execution", "platform"}, false); err != nil {
		return err
	}
	*endpoint = Endpoint(decoded)
	return nil
}

type Response struct {
	Output *protocol.Output
	Fault  *protocol.Fault
}

var responseVariants = map[string]func() any{
	"Ok":  func() any { return new(protocol.Output) },
	"Err": func() any { return new(protocol.Fault) },
}

func (response Response) MarshalJSON() ([]byte, error) {
	if (response.Output == nil) == (response.Fault == nil) {
		return nil, errors.New("expected exactly one response result")
	}
	if response.Fault != nil {
		return protocol.MarshalUnion("", "", "Err", response.Fault, responseVariants)
	}
	return protocol.MarshalUnion("", "", "Ok", response.Output, responseVariants)
}

func (response *Response) UnmarshalJSON(data []byte) error {
	kind, payload, err := protocol.UnmarshalUnion(data, "", "", responseVariants, false)
	if err != nil {
		return err
	}
	decoded := Response{}
	switch kind {
	case "Ok":
		decoded.Output = payload.(*protocol.Output)
	case "Err":
		decoded.Fault = payload.(*protocol.Fault)
	default:
		// A result that is neither is not one this build can deliver.
		return invalid()
	}
	*response = decoded
	return nil
}

type Reply struct {
	Kind          ReplyKind
	Endpoint      *Endpoint
	Receipt       *protocol.Receipt
	Rejected      *protocol.Fault
	Completed     *Response
	Update        *protocol.Update
	SnapshotStart *SnapshotHeader
	SnapshotPage  *SnapshotPage
}

var replyVariants = map[string]func() any{
	"endpoint":       func() any { return new(Endpoint) },
	"stream_opened":  nil,
	"subscribed":     nil,
	"receipt":        func() any { return new(protocol.Receipt) },
	"rejected":       func() any { return new(protocol.Fault) },
	"completed":      func() any { return new(Response) },
	"update":         func() any { return new(protocol.Update) },
	"snapshot_start": func() any { return new(SnapshotHeader) },
	"snapshot_page":  func() any { return new(SnapshotPage) },
}

func (reply Reply) MarshalJSON() ([]byte, error) {
	var payload any
	switch reply.Kind {
	case ReplyEndpoint:
		payload = reply.Endpoint
	case ReplyReceipt:
		payload = reply.Receipt
	case ReplyRejected:
		payload = reply.Rejected
	case ReplyCompleted:
		payload = reply.Completed
	case ReplyUpdate:
		payload = reply.Update
	case ReplySnapshotStart:
		payload = reply.SnapshotStart
	case ReplySnapshotPage:
		payload = reply.SnapshotPage
	}
	return protocol.MarshalUnion("kind", "data", string(reply.Kind), payload, replyVariants)
}

func (reply *Reply) UnmarshalJSON(data []byte) error {
	kind, payload, err := protocol.UnmarshalUnion(data, "kind", "data", replyVariants, false)
	if err != nil {
		return err
	}
	decoded := Reply{Kind: ReplyKind(kind)}
	switch decoded.Kind {
	case ReplyEndpoint:
		decoded.Endpoint = payload.(*Endpoint)
	case ReplyReceipt:
		decoded.Receipt = payload.(*protocol.Receipt)
	case ReplyRejected:
		decoded.Rejected = payload.(*protocol.Fault)
	case ReplyCompleted:
		decoded.Completed = payload.(*Response)
	case ReplyUpdate:
		decoded.Update = payload.(*protocol.Update)
	case ReplySnapshotStart:
		decoded.SnapshotStart = payload.(*SnapshotHeader)
	case ReplySnapshotPage:
		decoded.SnapshotPage = payload.(*SnapshotPage)
	}
	*reply = decoded
	return nil
}
