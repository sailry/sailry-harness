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
	transport := &http.Transport{Proxy: http.ProxyURL(proxy), DialContext: dial, ForceAttemptHTTP2: true, TLSHandshakeTimeout: 10 * time.Second, ResponseHeaderTimeout: 10 * time.Second}
	if len(fetcher.clients) == 16 {
		fetcher.clients[0].transport.CloseIdleConnections()
		fetcher.clients = fetcher.clients[1:]
	}
	fetcher.clients = append(fetcher.clients, fetchClient{key: key, transport: transport})
	return transport, nil
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
			return &webConnection{Conn: connection}, nil
		}
		err = failure
		if ctx.Err() != nil {
			break
		}
	}
	return nil, err
}

type webConnection struct{ net.Conn }

func (connection *webConnection) Read(value []byte) (int, error) {
	if err := connection.Conn.SetReadDeadline(time.Now().Add(10 * time.Second)); err != nil {
		return 0, err
	}
	return connection.Conn.Read(value)
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
