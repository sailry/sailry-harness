package authorization

import (
	"context"
	"crypto/ecdsa"
	"crypto/ed25519"
	"crypto/elliptic"
	"crypto/rsa"
	"encoding/base64"
	"encoding/json"
	"io"
	"math/big"
	"net/http"
	"net/url"
	"slices"
	"strings"
	"time"

	"github.com/golang-jwt/jwt/v5"
)

type signingKey struct {
	Kind       string   `json:"kty"`
	ID         string   `json:"kid"`
	Algorithm  string   `json:"alg"`
	Use        string   `json:"use"`
	Operations []string `json:"key_ops"`
	Modulus    string   `json:"n"`
	Exponent   string   `json:"e"`
	Curve      string   `json:"crv"`
	X          string   `json:"x"`
	Y          string   `json:"y"`
}

func identityDocument(ctx context.Context, client *http.Client, endpoint string, output any) error {
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return invalid()
	}
	response, err := client.Do(request)
	if err != nil {
		return unavailable()
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, BodyLimit+1))
	if err != nil || len(body) > BodyLimit || response.StatusCode < 200 || response.StatusCode >= 300 || json.Unmarshal(body, output) != nil {
		return invalid()
	}
	return nil
}

func VerifyIdentity(ctx context.Context, client *http.Client, issuer, token, audience string, nonce *string, now func() time.Time) (map[string]any, error) {
	var discovery struct {
		Issuer string `json:"issuer"`
		Keys   string `json:"jwks_uri"`
	}
	if err := identityDocument(ctx, client, strings.TrimRight(issuer, "/")+"/.well-known/openid-configuration", &discovery); err != nil {
		return nil, err
	}
	origin, err := url.Parse(issuer)
	if err != nil {
		return nil, invalid()
	}
	keysURL, err := url.Parse(discovery.Keys)
	if err != nil || discovery.Issuer != issuer || origin.Scheme != "https" || keysURL.Scheme != origin.Scheme || keysURL.Host != origin.Host || keysURL.User != nil || keysURL.RawQuery != "" || keysURL.Fragment != "" {
		return nil, invalid()
	}
	var keys struct {
		Keys []signingKey `json:"keys"`
	}
	if err := identityDocument(ctx, client, discovery.Keys, &keys); err != nil {
		return nil, err
	}
	claims := jwt.MapClaims{}
	parsed, err := jwt.ParseWithClaims(token, claims, func(token *jwt.Token) (any, error) {
		id, ok := token.Header["kid"].(string)
		if !ok || id == "" {
			return nil, invalid()
		}
		for _, key := range keys.Keys {
			if key.ID == id {
				if key.Algorithm != "" && key.Algorithm != token.Method.Alg() || key.Use != "" && key.Use != "sig" || key.Operations != nil && !slices.Contains(key.Operations, "verify") {
					return nil, invalid()
				}
				return key.public(token.Method.Alg())
			}
		}
		return nil, invalid()
	}, jwt.WithValidMethods([]string{"RS256", "ES256", "EdDSA"}), jwt.WithExpirationRequired(), jwt.WithIssuer(issuer), jwt.WithAudience(audience), jwt.WithLeeway(5*time.Second), jwt.WithTimeFunc(now))
	if err != nil || !parsed.Valid {
		return nil, invalid()
	}
	subject, err := claims.GetSubject()
	if err != nil || !bounded(subject, 2048) {
		return nil, invalid()
	}
	issued, err := claims.GetIssuedAt()
	if err != nil || issued == nil || issued.Time.After(now().Add(5*time.Second)) {
		return nil, invalid()
	}
	if nonce != nil && claims["nonce"] != *nonce {
		return nil, invalid()
	}
	audiences, err := claims.GetAudience()
	if err != nil || claims["azp"] != nil && claims["azp"] != audience || len(audiences) > 1 && claims["azp"] != audience {
		return nil, invalid()
	}
	if email, exists := claims["email"]; exists {
		value, ok := email.(string)
		if !ok || !bounded(value, 1024) {
			return nil, invalid()
		}
	}
	return map[string]any(claims), nil
}

func (key signingKey) public(algorithm string) (any, error) {
	decode := base64.RawURLEncoding.DecodeString
	switch {
	case key.Kind == "RSA" && algorithm == "RS256":
		modulus, err := decode(key.Modulus)
		if err != nil {
			return nil, invalid()
		}
		exponent, err := decode(key.Exponent)
		if err != nil || len(exponent) == 0 || len(exponent) > 4 {
			return nil, invalid()
		}
		power := 0
		for _, value := range exponent {
			power = power<<8 | int(value)
		}
		if power < 2 || len(modulus) == 0 {
			return nil, invalid()
		}
		return &rsa.PublicKey{N: new(big.Int).SetBytes(modulus), E: power}, nil
	case key.Kind == "EC" && key.Curve == "P-256" && algorithm == "ES256":
		horizontal, err := decode(key.X)
		if err != nil {
			return nil, invalid()
		}
		vertical, err := decode(key.Y)
		if err != nil || len(horizontal) != 32 || len(vertical) != 32 {
			return nil, invalid()
		}
		point := &ecdsa.PublicKey{Curve: elliptic.P256(), X: new(big.Int).SetBytes(horizontal), Y: new(big.Int).SetBytes(vertical)}
		if !point.Curve.IsOnCurve(point.X, point.Y) {
			return nil, invalid()
		}
		return point, nil
	case key.Kind == "OKP" && key.Curve == "Ed25519" && algorithm == "EdDSA":
		value, err := decode(key.X)
		if err != nil || len(value) != ed25519.PublicKeySize {
			return nil, invalid()
		}
		return ed25519.PublicKey(value), nil
	}
	return nil, invalid()
}
