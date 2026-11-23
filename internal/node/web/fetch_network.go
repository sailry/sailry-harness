package web

import (
	"context"
	"net"
	"net/http"
	"net/netip"
	"net/url"
	"slices"
	"strings"
	"time"
)

// NewPublicHTTPSClient fetches automatic public resources under the policy of
// model web reads, through the system proxy when one applies.
func NewPublicHTTPSClient(timeout time.Duration, maxRedirects int) *http.Client {
	return newPublicHTTPSClient(NewFetcher(FetchOptions{}), timeout, maxRedirects)
}

func newPublicHTTPSClient(fetcher *Fetcher, timeout time.Duration, maxRedirects int) *http.Client {
	return &http.Client{
		Transport: publicHTTPS{fetcher},
		Timeout:   timeout,
		CheckRedirect: func(_ *http.Request, via []*http.Request) error {
			if len(via) > maxRedirects {
				return webUnavailable("web redirect limit exceeded")
			}
			return nil
		},
	}
}

// publicHTTPS checks every request, redirects included, before it leaves.
// A direct connection is checked at dial time against the address it uses.
// A proxy resolves the target itself, so the target is resolved here and
// refused when it names a private or local address; as for model web reads,
// a target only the proxy can resolve, or that resolves only to a TUN proxy's
// placeholder range, is handed to the proxy, which then owns the final address.
type publicHTTPS struct{ fetcher *Fetcher }

func (transport publicHTTPS) RoundTrip(request *http.Request) (*http.Response, error) {
	if request.URL.Scheme != "https" || request.URL.User != nil {
		return nil, webInvalid("web URL must use HTTPS without credentials")
	}
	target, err := publicURL(request.URL.String())
	if err != nil {
		return nil, err
	}
	request = request.Clone(request.Context())
	request.URL, request.Host = target, target.Host
	proxy, err := transport.fetcher.options.Proxy(request)
	if err != nil {
		return nil, webUnavailable("unsupported web proxy")
	}
	if proxy != nil {
		if err = transport.fetcher.validateDestination(request.Context(), target, true); err != nil {
			return nil, err
		}
	}
	next, err := transport.fetcher.transport(proxy)
	if err != nil {
		return nil, err
	}
	return next.RoundTrip(request)
}

func (transport publicHTTPS) CloseIdleConnections() { transport.fetcher.Close() }

func (fetcher *Fetcher) validateDestination(ctx context.Context, parsed *url.URL, proxied bool) error {
	if proxied {
		var cancel context.CancelFunc
		ctx, cancel = context.WithTimeout(ctx, 500*time.Millisecond)
		defer cancel()
	}
	addresses, err := fetcher.options.Resolve(ctx, parsed.Hostname())
	if err != nil {
		if proxied {
			return nil
		}
		return err
	}
	if proxied {
		addresses = slices.DeleteFunc(slices.Clone(addresses), func(address netip.Addr) bool {
			if address.Is4() {
				v := address.As4()
				return v[0] == 198 && (v[1] == 18 || v[1] == 19)
			}
			return false
		})
		if len(addresses) == 0 {
			return nil
		}
	}
	local, err := fetcher.options.LocalAddresses()
	if err != nil {
		return err
	}
	return checkAddresses(addresses, local)
}

func (fetcher *Fetcher) transport(proxy *url.URL) (http.RoundTripper, error) {
	if fetcher.options.Transport != nil {
		return fetcher.options.Transport, nil
	}
	key := ""
	if proxy != nil {
		key = proxy.String()
		if proxy.Scheme != "http" && proxy.Scheme != "https" && proxy.Scheme != "socks5" && proxy.Scheme != "socks5h" {
			return nil, webUnavailable("unsupported web proxy")
		}
	}
	fetcher.mu.Lock()
	defer fetcher.mu.Unlock()
	for index, entry := range fetcher.clients {
		if entry.key == key {
			fetcher.clients = append(append(fetcher.clients[:index], fetcher.clients[index+1:]...), entry)
			return entry.transport, nil
		}
	}
	dial := fetcher.options.DialContext
	if proxy == nil {
		dial = fetcher.dialPublic
	}
	transport := fetchTransport(proxy, dial)
	if len(fetcher.clients) == 16 {
		fetcher.clients[0].transport.CloseIdleConnections()
		fetcher.clients = fetcher.clients[1:]
	}
	fetcher.clients = append(fetcher.clients, fetchClient{key: key, transport: transport})
	return transport, nil
}

func fetchTransport(proxy *url.URL, dial func(context.Context, string, string) (net.Conn, error)) *http.Transport {
	return &http.Transport{
		Proxy:                 http.ProxyURL(proxy),
		DialContext:           dial,
		ForceAttemptHTTP2:     true,
		TLSHandshakeTimeout:   10 * time.Second,
		ResponseHeaderTimeout: 10 * time.Second,
		IdleConnTimeout:       30 * time.Second,
	}
}

func (fetcher *Fetcher) dialPublic(ctx context.Context, network, address string) (net.Conn, error) {
	host, port, err := net.SplitHostPort(address)
	if err != nil {
		return nil, webInvalid("invalid web destination")
	}
	addresses, err := fetcher.options.Resolve(ctx, strings.Trim(host, "[]"))
	if err != nil {
		return nil, err
	}
	local, err := fetcher.options.LocalAddresses()
	if err != nil {
		return nil, err
	}
	if err = checkAddresses(addresses, local); err != nil {
		return nil, err
	}
	for _, address := range addresses {
		connection, failure := fetcher.options.DialContext(ctx, network, net.JoinHostPort(address.String(), port))
		if failure == nil {
			return connection, nil
		}
		err = failure
		if ctx.Err() != nil {
			break
		}
	}
	return nil, err
}

func (fetcher *Fetcher) Close() error {
	fetcher.mu.Lock()
	defer fetcher.mu.Unlock()
	for _, entry := range fetcher.clients {
		entry.transport.CloseIdleConnections()
	}
	fetcher.clients = nil
	return nil
}
