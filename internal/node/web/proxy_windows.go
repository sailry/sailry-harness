package web

import (
	"strings"

	"golang.org/x/sys/windows/registry"
)

func nativeProxySettings() proxySettings {
	key, err := registry.OpenKey(registry.CURRENT_USER, `Software\Microsoft\Windows\CurrentVersion\Internet Settings`, registry.QUERY_VALUE)
	if err != nil {
		return proxySettings{}
	}
	defer key.Close()
	enabled, _, _ := key.GetIntegerValue("ProxyEnable")
	if enabled != 1 {
		return proxySettings{}
	}
	proxy, _, _ := key.GetStringValue("ProxyServer")
	settings := proxySettings{}
	if strings.Contains(proxy, "=") {
		for _, entry := range strings.Split(proxy, ";") {
			scheme, endpoint, ok := strings.Cut(strings.TrimSpace(entry), "=")
			if ok {
				switch scheme {
				case "http":
					settings.http = endpoint
				case "https":
					settings.https = endpoint
				}
			}
		}
	} else {
		settings.http, settings.https = proxy, proxy
	}
	bypass, _, _ := key.GetStringValue("ProxyOverride")
	entries := []string{}
	for _, entry := range strings.Split(bypass, ";") {
		entry = strings.TrimPrefix(strings.TrimSpace(entry), "*.")
		if entry != "" && entry != "<local>" {
			entries = append(entries, entry)
		}
	}
	settings.bypass = strings.Join(entries, ",")
	return settings
}
