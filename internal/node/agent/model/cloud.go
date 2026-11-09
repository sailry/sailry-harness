// Foundry routing follows Goose 14741e9a. Apache-2.0; see LICENSE.goose.
package model

import (
	"context"
	"encoding/json"
	"net/http"
	"net/url"
	"strings"
	"time"

	"github.com/egoist/mygo/netproxy"
)

func foundryBinding(ctx context.Context, binding Binding, config transportConfig) (transportConfig, error) {
	u, err := endpoint(binding.Provider.Endpoint)
	if err != nil {
		return config, err
	}
	project := strings.Contains(u.Path, "/api/projects/")
	resource := strings.HasSuffix(u.Hostname(), ".services.ai.azure.com")
	if !project && !resource {
		u.Path = strings.TrimSuffix(strings.TrimSuffix(strings.TrimRight(u.Path, "/"), "/chat/completions"), "/v1") + "/v1"
		config.Query["api-version"] = "2024-05-01-preview"
		config.Endpoint = strings.TrimRight(u.String(), "/")
		return config, nil
	}
	underlying, publisher := config.Model, ""
	if project {
		lookup, cancel := context.WithTimeout(ctx, 5*time.Second)
		name, vendor := lookupDeployment(lookup, u, config)
		cancel()
		if ctx.Err() != nil {
			return config, Classify(ctx.Err())
		}
		if name != "" {
			underlying, publisher = name, vendor
		}
	}
	anthropic := strings.HasPrefix(strings.ToLower(underlying), "claude")
	openai := responsesModel.MatchString(underlying)
	if publisher != "" {
		anthropic = strings.EqualFold(publisher, "anthropic")
		openai = strings.EqualFold(publisher, "openai")
	}
	if anthropic {
		root := strings.Split(u.Path, "/api/projects/")[0]
		u.Path = strings.TrimRight(root, "/") + "/anthropic"
		config.Kind = "anthropic"
	} else {
		u.Path = strings.TrimRight(u.Path, "/") + "/openai/v1"
		config.Kind = "chat"
		if openai && responsesModel.MatchString(underlying) {
			config.Kind = "responses"
		}
	}
	config.Endpoint = strings.TrimRight(u.String(), "/")
	config.Model = underlying
	if underlying != binding.Config.Model {
		config.JSON["model"] = binding.Config.Model
	}
	return config, nil
}
func lookupDeployment(ctx context.Context, root *url.URL, config transportConfig) (string, string) {
	client := &http.Client{Transport: netproxy.Transport(), CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}
	next := *root
	next.Path = strings.TrimRight(next.Path, "/") + "/deployments"
	next.RawQuery = "api-version=v1"
	for {
		if next.Scheme != root.Scheme || next.Host != root.Host {
			return "", ""
		}
		request, err := http.NewRequestWithContext(ctx, http.MethodGet, next.String(), nil)
		if err != nil {
			return "", ""
		}
		for key, value := range config.Headers {
			request.Header.Set(key, value)
		}
		response, err := client.Do(request)
		if err != nil {
			return "", ""
		}
		if response.StatusCode < 200 || response.StatusCode >= 300 {
			response.Body.Close()
			return "", ""
		}
		var body struct {
			Value    []struct{ Name, ModelName, ModelPublisher string }
			NextLink string
		}
		err = json.NewDecoder(response.Body).Decode(&body)
		response.Body.Close()
		if err != nil {
			return "", ""
		}
		for _, value := range body.Value {
			if value.Name == config.Model {
				name := value.ModelName
				if name == "" {
					name = config.Model
				}
				return name, value.ModelPublisher
			}
		}
		if body.NextLink == "" {
			return "", ""
		}
		url, err := next.Parse(body.NextLink)
		if err != nil {
			return "", ""
		}
		next = *url
		query := next.Query()
		if !query.Has("api-version") {
			query.Set("api-version", "v1")
		}
		next.RawQuery = query.Encode()
	}
}
