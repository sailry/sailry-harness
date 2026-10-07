//! OAuth result presentation belongs to Desktop, not the shared Client transport.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use sailry_client::oauth::Reply;
use std::time::Duration;
use tokio::sync::oneshot;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Outcome {
    Connected,
    Failed,
    Cancelled,
    Pending,
}

pub(crate) fn response(
    receiver: oneshot::Receiver<Outcome>,
) -> impl std::future::Future<Output = Reply> + Send + 'static {
    let page = Page::new();
    async move {
        let outcome = tokio::time::timeout(Duration::from_secs(120), receiver)
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or(Outcome::Pending);
        Reply::Html(page.render(outcome))
    }
}

struct Page {
    language: String,
    titles: [String; 4],
    return_label: String,
    close_label: String,
}

impl Page {
    fn new() -> Self {
        Self {
            language: rust_i18n::locale().to_string(),
            titles: [
                crate::tr("oauth_success_title").to_string(),
                crate::tr("oauth_failed_title").to_string(),
                crate::tr("oauth_cancelled_title").to_string(),
                crate::tr("oauth_pending_title").to_string(),
            ],
            return_label: crate::tr("oauth_return").to_string(),
            close_label: crate::tr("oauth_close_page").to_string(),
        }
    }

    fn render(&self, outcome: Outcome) -> String {
        let (index, state) = match outcome {
            Outcome::Connected => (0, "connected"),
            Outcome::Failed => (1, "failed"),
            Outcome::Cancelled => (2, "cancelled"),
            Outcome::Pending => (3, "pending"),
        };
        let title = escape(&self.titles[index]);
        let language = escape(&self.language);
        let return_label = escape(&self.return_label);
        let close_label = escape(&self.close_label);
        let logo = STANDARD.encode(include_bytes!(
            "../../../../assets/branding/sailry-mark.svg"
        ));
        let return_url = super::RETURN_URL;
        format!(
            r##"<!doctype html>
<html lang="{language}">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta name="referrer" content="no-referrer">
  <meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src data:; style-src 'unsafe-inline'; script-src 'unsafe-inline'; base-uri 'none'; form-action 'none'">
  <title>{title} · Sailry</title>
  <style>
    :root {{ color-scheme: light dark; --background: #111315; --text: #f4f5f6; --muted: #8b9097; --button: #f4f5f6; --button-text: #151719; --focus: #79c9f1; }}
    * {{ box-sizing: border-box; }}
    body {{ margin: 0; background: var(--background); color: var(--text); font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; -webkit-font-smoothing: antialiased; }}
    .page {{ min-height: 100vh; min-height: 100svh; display: grid; place-items: center; padding: 32px 24px; background: radial-gradient(ellipse at 50% 38%, rgba(40,116,218,.07), transparent 55%); }}
    .content {{ width: min(100%, 480px); text-align: center; }}
    .logo {{ display: block; width: 88px; height: 88px; margin: 0 auto 28px; }}
    .brand {{ margin: 0 0 16px; color: var(--muted); font-size: 14px; font-weight: 500; letter-spacing: .04em; }}
    h1 {{ margin: 0; font-size: clamp(24px, 4vw, 30px); font-weight: 600; letter-spacing: -.025em; line-height: 1.35; text-wrap: balance; }}
    .return {{ display: inline-flex; align-items: center; justify-content: center; min-height: 46px; margin-top: 32px; padding: 12px 26px; border-radius: 12px; background: var(--button); color: var(--button-text); font-size: 15px; font-weight: 600; text-decoration: none; transition: opacity .15s ease; }}
    .return:hover {{ opacity: .87; }}
    .return:focus-visible {{ outline: 3px solid var(--focus); outline-offset: 5px; }}
    .close {{ min-height: 20px; margin: 24px 0 0; color: var(--muted); font-size: 13px; line-height: 1.5; }}
    @media (prefers-color-scheme: light) {{ :root {{ --background: #fafbfc; --text: #1b2025; --muted: #737b85; --button: #1b2025; --button-text: #fafbfc; }} }}
    @media (prefers-reduced-motion: reduce) {{ .return {{ transition: none; }} }}
  </style>
</head>
<body>
  <main class="page" data-outcome="{state}">
    <section class="content" aria-labelledby="result-title">
      <img class="logo" src="data:image/svg+xml;base64,{logo}" alt="Sailry" width="88" height="88">
      <p class="brand">Sailry</p>
      <h1 id="result-title">{title}</h1>
      <a class="return" id="return" href="{return_url}" rel="noreferrer">{return_label}</a>
      <p class="close" id="close" aria-live="polite" hidden>{close_label}</p>
      <noscript><p class="close">{close_label}</p></noscript>
    </section>
  </main>
  <script>
    history.replaceState(null, '', location.pathname);
    document.getElementById('return').addEventListener('click', () => {{
      document.getElementById('close').hidden = false;
      setTimeout(() => {{ try {{ window.close(); }} catch (_) {{}} }}, 300);
    }});
  </script>
</body>
</html>"##
        )
    }
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests;
