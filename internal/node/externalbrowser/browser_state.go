package externalbrowser

import (
	"context"

	"github.com/chromedp/cdproto/browser"
	"github.com/chromedp/cdproto/cdp"
	"github.com/chromedp/cdproto/network"
	"github.com/chromedp/cdproto/target"
	"github.com/chromedp/chromedp"
)

type browserBounds struct {
	state         string
	width, height int64
	x, y          *int64
}

func setBounds(ctx context.Context, id target.ID, bounds browserBounds) error {
	window, err := chromedp.CallBrowser(ctx, browser.GetWindowForTarget, browser.GetWindowForTargetParams{TargetID: id})
	if err != nil {
		return err
	}
	_, err = chromedp.CallBrowser(ctx, browser.SetWindowBounds, browser.SetWindowBoundsParams{WindowID: window.WindowID, Bounds: &browser.Bounds{
		Left: bounds.x, Top: bounds.y, Width: bounds.width, Height: bounds.height, WindowState: browser.WindowState(bounds.state),
	}})
	return err
}

// cookies reads the cookies that apply to the current page.
func (session *session) cookies(ctx context.Context) ([]*network.Cookie, error) {
	bound, cancel, err := session.top(ctx, scriptTimeout)
	if err != nil {
		return nil, err
	}
	defer cancel()
	result, err := chromedp.Call(bound, network.GetCookies, network.GetCookiesParams{})
	if err != nil {
		return nil, failed("Failed to get cookies", err)
	}
	return result.Cookies, nil
}

func cookieValue(cookie *network.Cookie) map[string]any {
	return map[string]any{"name": cookie.Name, "value": cookie.Value, "domain": cookie.Domain, "path": cookie.Path, "secure": cookie.Secure}
}

func getCookies(ctx context.Context, session *session, _ map[string]any) (map[string]any, error) {
	cookies, err := session.cookies(ctx)
	if err != nil {
		return nil, err
	}
	values := make([]any, 0, len(cookies))
	for _, cookie := range cookies {
		values = append(values, cookieValue(cookie))
	}
	return map[string]any{"success": true, "cookies": values, "count": len(values)}, nil
}

func getCookie(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	name, err := required(arguments, "name")
	if err != nil {
		return nil, err
	}
	cookies, err := session.cookies(ctx)
	if err != nil {
		return nil, err
	}
	for _, cookie := range cookies {
		if cookie.Name == name {
			return map[string]any{"success": true, "cookie": cookieValue(cookie)}, nil
		}
	}
	return nil, unavailable("Cookie not found: " + name)
}

func addCookie(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	name, err := required(arguments, "name")
	if err != nil {
		return nil, err
	}
	value, err := required(arguments, "value")
	if err != nil {
		return nil, err
	}
	address, _, err := session.location(ctx)
	if err != nil {
		return nil, failed("Failed to add cookie", err)
	}
	secure := boolean(arguments, "secure", false)
	params := network.SetCookieParams{Name: name, Value: value, Path: "/", Secure: &secure}
	if path, ok := argument[string](arguments, "path"); ok {
		params.Path = path
	}
	if domain, ok := argument[string](arguments, "domain"); ok {
		params.Domain = domain
	} else {
		// Without a domain the cookie belongs to the current page.
		params.URL = address
	}
	if expiry, ok := argument[float64](arguments, "expiry"); ok {
		params.Expires = cdp.TimeSinceEpoch(expiry)
	}
	bound, cancel, err := session.top(ctx, scriptTimeout)
	if err != nil {
		return nil, err
	}
	defer cancel()
	if _, err := chromedp.Call(bound, network.SetCookie, params); err != nil {
		return nil, failed("Failed to add cookie", err)
	}
	return map[string]any{"success": true, "added_cookie": name}, nil
}

func deleteCookie(ctx context.Context, session *session, arguments map[string]any) (map[string]any, error) {
	name, err := required(arguments, "name")
	if err != nil {
		return nil, err
	}
	if err := session.removeCookies(ctx, func(cookie *network.Cookie) bool { return cookie.Name == name }); err != nil {
		return nil, err
	}
	return map[string]any{"success": true, "deleted_cookie": name}, nil
}

func deleteAllCookies(ctx context.Context, session *session, _ map[string]any) (map[string]any, error) {
	if err := session.removeCookies(ctx, func(*network.Cookie) bool { return true }); err != nil {
		return nil, err
	}
	return map[string]any{"success": true, "message": "All cookies deleted"}, nil
}

// removeCookies deletes the matching cookies of the current page only.
func (session *session) removeCookies(ctx context.Context, match func(*network.Cookie) bool) error {
	cookies, err := session.cookies(ctx)
	if err != nil {
		return err
	}
	bound, cancel, err := session.top(ctx, scriptTimeout)
	if err != nil {
		return err
	}
	defer cancel()
	for _, cookie := range cookies {
		if !match(cookie) {
			continue
		}
		if _, err := chromedp.Call(bound, network.DeleteCookies, network.DeleteCookiesParams{Name: cookie.Name, Domain: cookie.Domain, Path: cookie.Path}); err != nil {
			return failed("Failed to delete cookie", err)
		}
	}
	return nil
}
