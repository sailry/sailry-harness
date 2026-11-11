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
	CallInspect    CallKind = "inspect"
	CallOpenStream CallKind = "open_stream"
	CallCommand    CallKind = "command"
	CallSubscribe  CallKind = "subscribe"
)

type Call struct {
	Kind    CallKind
	Stream  *protocol.StreamID
	Request *protocol.Request
	Topic   *protocol.Topic
}

var callVariants = map[string]func() any{
	"inspect":     nil,
	"open_stream": func() any { return new(protocol.StreamID) },
	"command":     func() any { return new(protocol.Request) },
	"subscribe":   func() any { return new(protocol.Topic) },
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
	}
	*call = decoded
	return nil
}

type Envelope struct {
	SoftwareVersion *string
	Protocol        *uint16
	Zstd            bool
	Call            Call
}

func (envelope Envelope) MarshalJSON() ([]byte, error) {
	metadata, err := json.Marshal(struct {
		SoftwareVersion *string `json:"software_version"`
		Protocol        *uint16 `json:"protocol"`
		Zstd            bool    `json:"zstd"`
	}{envelope.SoftwareVersion, envelope.Protocol, envelope.Zstd})
	if err != nil {
		return nil, err
	}
	call, err := json.Marshal(envelope.Call)
	if err != nil {
		return nil, err
	}
	flattened := append(metadata[:len(metadata)-1], ',')
	return append(flattened, call[1:]...), nil
}

func (envelope *Envelope) UnmarshalJSON(data []byte) error {
	var metadata struct {
		SoftwareVersion *string `json:"software_version"`
		Protocol        *uint16 `json:"protocol"`
		Zstd            bool    `json:"zstd"`
	}
	if err := protocol.DecodeObject(data, &metadata, nil, false); err != nil {
		return err
	}
	var call Call
	if err := json.Unmarshal(data, &call); err != nil {
		return err
	}
	*envelope = Envelope{metadata.SoftwareVersion, metadata.Protocol, metadata.Zstd, call}
	return nil
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

type Endpoint struct {
	SoftwareVersion *string `json:"software_version"`
	Protocol        *uint16 `json:"protocol"`
	Zstd            bool    `json:"zstd"`
	Execution       bool    `json:"execution"`
	Name            *string `json:"name"`
	Platform        string  `json:"platform"`
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
	if kind == "Ok" {
		decoded.Output = payload.(*protocol.Output)
	} else {
		decoded.Fault = payload.(*protocol.Fault)
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
