package model

import (
	"bytes"
	"context"
	"io"
	"net"
	"net/http"
	"net/url"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/net/http/httpguts"
)

func pluginAddress(value string) (*url.URL, error) {
	address, err := url.Parse(value)
	if err != nil || address.Host == "" || address.User != nil || address.Fragment != "" {
		return nil, &Failure{Kind: InvalidRequest, Message: "model request has an invalid URL or header"}
	}
	if address.Scheme == "https" {
		return address, nil
	}
	if address.Scheme == "http" && (address.Hostname() == "localhost" || net.ParseIP(address.Hostname()).IsLoopback()) {
		return address, nil
	}
	return nil, &Failure{Kind: InvalidRequest, Message: "model request has an invalid URL or header"}
}

func pluginOrigin(address *url.URL) string {
	port := address.Port()
	if address.Scheme == "https" && port == "443" || address.Scheme == "http" && port == "80" {
		port = ""
	}
	return strings.ToLower(address.Scheme) + "://" + strings.ToLower(address.Hostname()) + ":" + port
}

func validPluginHeader(name, value string) bool {
	if !httpguts.ValidHeaderFieldName(name) || !httpguts.ValidHeaderFieldValue(value) {
		return false
	}
	switch strings.ToLower(name) {
	case "host", "connection", "content-length", "transfer-encoding", "proxy-authorization", "proxy-authenticate", "upgrade", "trailer":
		return false
	}
	return true
}

func (model *pluginModel) send(ctx context.Context, prepared protocol.PluginModelAdapterWirePrepared, budget int) (*http.Response, error) {
	endpoint, err := pluginAddress(model.binding.Provider.Endpoint)
	if err != nil {
		return nil, err
	}
	target, err := pluginAddress(prepared.Url)
	if err != nil {
		return nil, err
	}
	sameOrigin := pluginOrigin(endpoint) == pluginOrigin(target)
	declared := sameOrigin
	for _, origin := range model.capture.Declaration.Origins {
		candidate, err := pluginAddress(origin)
		if err == nil && pluginOrigin(candidate) == pluginOrigin(target) {
			declared = true
		}
	}
	if !declared {
		return nil, &Failure{Kind: InvalidRequest, Message: "model request origin was not declared"}
	}
	headers := http.Header{}
	for name, value := range prepared.Headers {
		if !validPluginHeader(name, value) {
			return nil, &Failure{Kind: InvalidRequest, Message: "model request has an invalid URL or header"}
		}
		headers.Set(name, value)
	}
	if sameOrigin {
		for name, values := range model.capture.Headers {
			for _, value := range values {
				if !validPluginHeader(name, value) {
					return nil, &Failure{Kind: InvalidRequest, Message: "model credential header is invalid"}
				}
			}
			headers[name] = append([]string(nil), values...)
		}
	}
	if injection := prepared.Credential; injection != nil {
		if !sameOrigin {
			return nil, &Failure{Kind: InvalidRequest, Message: "model credentials cannot leave the configured origin"}
		}
		value := injection.Template
		for _, slot := range []struct {
			token  string
			secret *protocol.Secret
		}{{"{access}", model.capture.Access}, {"{key}", model.capture.Key}} {
			if strings.Contains(value, slot.token) {
				if slot.secret == nil {
					return nil, &Failure{Kind: NotConfigured, Message: "model credential is unavailable"}
				}
				value = strings.ReplaceAll(value, slot.token, slot.secret.Expose())
			}
		}
		if strings.ContainsAny(value, "{}") || !validPluginHeader(injection.Header, value) {
			return nil, &Failure{Kind: InvalidRequest, Message: "model credential header is invalid"}
		}
		headers.Set(injection.Header, value)
	}
	body := ""
	if prepared.Body != nil {
		body = *prepared.Body
	}
	size := len(prepared.Url) + len(body)
	for name, values := range headers {
		for _, value := range values {
			size += len(name) + len(value)
		}
	}
	if size > min(budget, pluginRequestBytes) {
		return nil, pluginResourceFailure()
	}
	method := ""
	switch prepared.Method {
	case protocol.PluginModelAuthMethodGet:
		method = http.MethodGet
	case protocol.PluginModelAuthMethodPost:
		method = http.MethodPost
	default:
		return nil, &Failure{Kind: InvalidRequest, Message: "model request method is invalid"}
	}
	owner, cancel := context.WithCancel(ctx)
	timer := time.AfterFunc(600*time.Second, cancel)
	request, err := http.NewRequestWithContext(owner, method, target.String(), bytes.NewBufferString(body))
	if err != nil {
		timer.Stop()
		cancel()
		return nil, &Failure{Kind: InvalidRequest, Message: "model request is invalid"}
	}
	request.Header = headers
	response, err := model.client.Do(request)
	if err != nil {
		timer.Stop()
		cancel()
		if ctx.Err() != nil {
			return nil, ctx.Err()
		}
		return nil, &Failure{Kind: Network, Message: "model connection failed"}
	}
	response.Body = &pluginResponseBody{ReadCloser: response.Body, timer: timer, cancel: cancel}
	return response, nil
}

type pluginResponseBody struct {
	io.ReadCloser
	timer  *time.Timer
	cancel context.CancelFunc
}

func (body *pluginResponseBody) Read(buffer []byte) (int, error) {
	body.timer.Reset(600 * time.Second)
	count, err := body.ReadCloser.Read(buffer)
	if count > 0 {
		body.timer.Reset(600 * time.Second)
	}
	return count, err
}

func (body *pluginResponseBody) Close() error {
	body.timer.Stop()
	body.cancel()
	return body.ReadCloser.Close()
}

func (model *pluginModel) public(value string) error {
	for _, secret := range model.capture.Secrets {
		if secret.Expose() != "" && strings.Contains(value, secret.Expose()) {
			return &Failure{Kind: InvalidResponse, Message: "model response exposed a private credential"}
		}
	}
	return nil
}

func (model *pluginModel) publicHeaders(headers http.Header) (map[string][]string, error) {
	result := map[string][]string{}
	for _, name := range []string{"content-type", "retry-after"} {
		for _, value := range headers.Values(name) {
			if err := model.public(value); err != nil {
				return nil, err
			}
			result[name] = append(result[name], value)
		}
	}
	return result, nil
}

func pluginHTTPClass(status int) FailureKind {
	switch {
	case status == 401 || status == 403:
		return Authentication
	case status == 429:
		return RateLimited
	case status == 408 || status >= 500 && status <= 599:
		return Unavailable
	case status >= 400 && status <= 499:
		return InvalidRequest
	default:
		return Unavailable
	}
}
