package web

import (
	"net/http"
	"net/url"
	"os"
	"strings"

	"golang.org/x/net/http/httpproxy"
)

type proxySettings struct{ http, https, bypass string }

func systemProxy(request *http.Request) (*url.URL, error) {
	settings := nativeProxySettings()
	value := func(upper, lower, fallback string) string {
		if value, ok := os.LookupEnv(upper); ok {
			return value
		}
		if value, ok := os.LookupEnv(lower); ok {
			return value
		}
		return fallback
	}
	all := value("ALL_PROXY", "all_proxy", "")
	httpFallback, httpsFallback := settings.http, settings.https
	if _, upper := os.LookupEnv("ALL_PROXY"); upper {
		httpFallback, httpsFallback = all, all
	} else if _, lower := os.LookupEnv("all_proxy"); lower {
		httpFallback, httpsFallback = all, all
	}
	settings.http = value("HTTP_PROXY", "http_proxy", httpFallback)
	settings.https = value("HTTPS_PROXY", "https_proxy", httpsFallback)
	settings.bypass = value("NO_PROXY", "no_proxy", settings.bypass)
	return selectProxy(request.URL, settings)
}

func selectProxy(destination *url.URL, settings proxySettings) (*url.URL, error) {
	selected := settings.http
	if destination.Scheme == "https" {
		selected = settings.https
	}
	if selected != "" {
		if !strings.Contains(selected, "://") {
			selected = "http://" + selected
		}
		parsed, err := url.Parse(selected)
		if err != nil || parsed.Hostname() == "" || (parsed.Scheme != "http" && parsed.Scheme != "https" && parsed.Scheme != "socks5" && parsed.Scheme != "socks5h") {
			return nil, webUnavailable("unsupported web proxy")
		}
	}
	return (&httpproxy.Config{HTTPProxy: settings.http, HTTPSProxy: settings.https, NoProxy: settings.bypass}).ProxyFunc()(destination)
}
