package model

import (
	"errors"
	"net/http"

	"charm.land/fantasy"
	"golang.org/x/net/http2"
)

func http2ProtocolFailure(err error) bool {
	var provider *fantasy.ProviderError
	if errors.As(err, &provider) {
		if len(provider.ResponseBody) > 0 {
			return false
		}
		err = provider.Cause
	}
	// Go's bundled HTTP/2 StreamError supports errors.As into x/net/http2.
	stream, ok := errors.AsType[http2.StreamError](err)
	return ok && stream.Code == http2.ErrCodeProtocol
}

func http2ConnectionLost(err error) bool {
	var provider *fantasy.ProviderError
	if errors.As(err, &provider) {
		if len(provider.ResponseBody) != 0 {
			return false
		}
		err = provider.Cause
	}
	// Go's HTTP/2 transport creates a fresh, unexported plain error when its
	// connection health check fails. Match the cause, not a provider's prose.
	for ; err != nil; err = errors.Unwrap(err) {
		if err.Error() == "http2: client connection lost" {
			return true
		}
	}
	return false
}

func (model *sdkModel) http1Fallback() (*sdkModel, func()) {
	transport := model.client.Transport
	vertex, cloud := transport.(vertexTransport)
	if cloud {
		transport = vertex.base
	}
	claude, oauth := transport.(claudeTransport)
	if oauth {
		transport = claude.base
	}
	response, ok := transport.(responseTransport)
	if !ok {
		return nil, nil
	}
	base, ok := response.base.(*http.Transport)
	if !ok {
		return nil, nil
	}
	fallback := base.Clone()
	fallback.Protocols = &http.Protocols{}
	fallback.Protocols.SetHTTP1(true)
	if fallback.TLSClientConfig != nil {
		// Clone retains the ALPN offers installed when the original transport negotiated h2.
		fallback.TLSClientConfig.NextProtos = []string{"http/1.1"}
	}
	response.base = fallback
	transport = response
	if oauth {
		claude.base = transport
		transport = claude
	}
	if cloud {
		vertex.base = transport
		transport = vertex
	}
	client := *model.client
	client.Transport = transport
	copy := *model
	copy.client = &client
	return &copy, fallback.CloseIdleConnections
}
