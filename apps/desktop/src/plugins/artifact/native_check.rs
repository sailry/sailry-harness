//! Real offline package rendering and controls in a native WebKit event loop.
use super::*;
use gpui_kit::component::{Root, Theme, ThemeMode};
use sailry_protocol::{SessionId, TurnId, artifact, plugin};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    path::Path,
    rc::Rc,
    time::{Duration, Instant},
};

const SOURCE: &str = r##"<section id="architecture" data-title="Architecture"><h1>Captured resources</h1>
<p class="lead">Local and remote ownership 中文 🙂 <a href="#architecture">Architecture</a></p>
<pre class="mermaid">flowchart LR
 A["Node"] --> B["Client"]</pre>
<pre class="math">P(A\mid B)=\frac{P(B\mid A)P(A)}{P(B)}</pre>
<table><tr><th>Owner</th><th>Resource</th></tr><tr><td>Node</td><td>Storage</td></tr></table>
<div id="alignment-fixture">
<label><span>Radio</span><input type="radio"></label>
<label><span>Checkbox</span><input type="checkbox"></label>
<label><span>Input</span><input type="text"></label>
<label><span>Select</span><select><option>Option</option></select></label></div>
<form data-name="decisions"><fieldset data-name="storage" data-required><legend>Storage</legend>
<label><input type="radio" name="storage" value="node">Execution Node</label>
<label><input type="radio" name="storage" value="project">Project</label></fieldset>
<fieldset data-name="note"><legend>Note</legend><textarea name="note"></textarea></fieldset>
<button type="submit">Send</button></form></section>"##;

fn fixture() -> Content {
    let root = std::env::var_os("SAILRY_WORKLOAD_PACKAGE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/brief"));
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(root.join("plugin.json")).unwrap()).unwrap();
    let view: artifact::View = serde_json::from_value(
        manifest["extensions"]["dev.sailry.platform"]["artifact_view"].clone(),
    )
    .unwrap();
    let asset = |path: &str| std::fs::read_to_string(root.join(path)).unwrap();
    let mut html = SOURCE.to_owned();
    for kind in ["line", "bar", "area", "pie", "radar"] {
        let chart = json!({"type":kind,"title":kind,"data":[{"label":"Local","value":3},{"label":"Remote","value":4},{"label":"Recovered","value":2}]});
        html.push_str(&format!("<pre class=\"chart\">{chart}</pre>"));
    }
    for chart in [
        json!({"type":"candlestick","title":"Values","data":[{"label":"A","open":3,"high":5,"low":1,"close":4}]}),
        json!({"type":"sankey","title":"Ownership","nodes":[{"id":"node","label":"Node"},{"id":"client","label":"Client"}],"links":[{"source":"node","target":"client","value":4}]}),
    ] {
        html.push_str(&format!("<pre class=\"chart\">{chart}</pre>"));
    }
    Content {
        artifact: Summary {
            id: ArtifactId::new(),
            session: SessionId::new(),
            turn: TurnId::new(),
            package: plugin::Reference {
                name: "fixture".into(),
                digest: "frozen".into(),
                settings_revision: 0,
            },
            title: "HTML brief".into(),
            summary: "Native fixture".into(),
            revision: 1,
            size: html.len() as u64,
            created_at: 1,
            updated_at: 1,
        },
        latest_revision: 1,
        html,
        warnings: vec![],
        view: Some(artifact::ViewContent {
            template: asset(&view.template),
            styles: view.styles.iter().map(|path| asset(path)).collect(),
            scripts: view.scripts.iter().map(|path| asset(path)).collect(),
            libraries: view.libraries,
        }),
    }
}

struct Harness {
    native: Rc<crate::content::files::html::surface::Native>,
    events: Rc<RefCell<Vec<Value>>>,
}
impl Render for Harness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::content::files::html::surface::render(self.native.clone(), window, cx)
    }
}

pub(in crate::plugins) fn run(output: &Path) {
    let output = output.to_owned();
    std::fs::create_dir_all(&output).unwrap();
    gpui_kit::application().with_assets(crate::assets::Assets).run(move|cx| {
        gpui_kit::init(cx);crate::theme::init(cx);rust_i18n::set_locale("en");cx.activate(true);
        let output=output.clone();
        cx.spawn(async move|cx| {
            let _ = crate::workload::check(async {
            let content=fixture();let mounted=Rc::new(RefCell::new(None));let capture=mounted.clone();
            let initial=content.clone();
            let window=cx.open_window(WindowOptions::default(),move|window,cx| {
                window.set_window_title("Sailry — HTML document fixture");window.activate_window();
                Theme::change(ThemeMode::Light,Some(window),cx);
                let html=page::document(&initial,page::theme(cx),page::labels(),true,vec![]).unwrap();
                let events=Rc::new(RefCell::new(Vec::<Value>::new()));let posts=events.clone();let loaded=events.clone();
                let builder=wry::WebViewBuilder::new().with_html(html)
                    .with_background_throttling(wry::BackgroundThrottlingPolicy::Disabled)
                    .with_initialization_script("window.requestAnimationFrame=callback=>setTimeout(()=>callback(performance.now()),16);window.cancelAnimationFrame=id=>clearTimeout(id);")
                    .with_navigation_handler(|url|url=="about:blank")
                    .with_on_page_load_handler(move|event,_| {
                        if matches!(event,wry::PageLoadEvent::Finished) {loaded.borrow_mut().push(json!({"kind":"loaded"}));}
                    })
                    .with_ipc_handler(move|request| {
                        if let Ok(event)=serde_json::from_str(request.body()){posts.borrow_mut().push(event);}
                    });
                let native=Rc::new(crate::content::files::html::surface::Native::build(builder,window).unwrap());
                let view=cx.new(|_|Harness{native,events});*capture.borrow_mut()=Some(view.clone());
                cx.new(|cx|Root::new(view,window,cx))
            }).unwrap();
            let view=mounted.borrow_mut().take().unwrap();
            let task=window.update(cx,|_,window,cx|window.spawn(cx,async move|cx| {
                let _ = crate::workload::check(async {
                verify(&view,&content,cx).await;
                let fallback=cx.update(|window,cx|page::capture(content.clone(),window,cx)).unwrap().await.unwrap();
                assert!(fallback.contains("<svg"));
                evaluate(&view,&page::serialize_capture(Some(1)),cx).await;
                let deadline=Instant::now()+Duration::from_secs(60);
                let export=loop {
                    if let Some(event)=events(&view,cx).into_iter().find(|event|event["kind"]=="export" && event["capture"]==1) {
                        break event["html"].as_str().unwrap().to_owned();
                    }
                    assert!(Instant::now()<deadline,"live export deadline");
                    pause(cx).await;
                };
                assert!(export.contains("<svg"));assert!(!export.contains("<script"));assert!(!export.contains("<form"));
                assert!(export.contains("Retained answer 中文 🙂"));
                assert!(export.contains("sailry.dev"));assert!(export.matches("data:image/svg+xml;base64,").count()>=2);
                std::fs::write(output.join("brief.html"),&export).unwrap();
                let directory=tempfile::tempdir().unwrap();
                crate::live::menus::export::image::assets(directory.path()).unwrap();
                let png=cx.update(|window,cx|crate::live::menus::export::image::render(
                    super::export::image_document(export),"assets".into(),directory.path().to_owned(),window,cx,
                )).unwrap().await.unwrap();
                let bitmap=::image::load_from_memory(&png).unwrap();
                assert_eq!(bitmap.width(),960);assert!(bitmap.height()>7*440);
                std::fs::write(output.join("brief.png"),png).unwrap();
                std::fs::write(output.join("brief-native.json"),"{\"webkit\":true,\"offline\":true,\"charts\":7,\"mermaid\":true,\"mathjax\":true,\"form_capture\":true,\"pending_preserved\":true,\"theme_preserved\":true,\"readonly_export\":true,\"png_export\":true,\"branding\":true}\n").unwrap();
                println!("HTML document: local charts, Mermaid, MathJax, captured forms, pending answers, theme changes and branded export verified");
                cx.update(|window,_|window.remove_window()).unwrap();
                }).await;
            })).unwrap();
            task.await;
            }).await;
            cx.update(|cx|cx.quit());
        }).detach();
    });
}

async fn verify(view: &Entity<Harness>, content: &Content, cx: &mut AsyncWindowContext) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !events(view, cx)
        .iter()
        .any(|event| event["kind"] == "loaded")
    {
        assert!(Instant::now() < deadline, "native document load deadline");
        pause(cx).await;
    }
    until(view,"document.querySelectorAll('.document-chart svg').length===7 && !!document.querySelector('.document-diagram svg') && !!document.querySelector('.document-math svg')",cx).await;
    assert_eq!(
        evaluate(view,"getComputedStyle(document.body).fontFamily.includes('system-ui') && getComputedStyle(document.body).fontFamily.includes('sans-serif') && CSS.supports('font-family',getComputedStyle(document.documentElement).getPropertyValue('--document-mono-font-family'))",cx).await,
        "true"
    );
    assert_eq!(
        evaluate(
            view,
            "document.querySelectorAll('script[src],link[href],iframe').length",
            cx
        )
        .await,
        "0"
    );
    assert_eq!(
        evaluate(
            view,
            "document.querySelector('meta[http-equiv]').content.includes(\"connect-src 'none'\")",
            cx
        )
        .await,
        "true"
    );
    assert_eq!(
        evaluate(
            view,
            "document.querySelectorAll('#document-toc a').length",
            cx
        )
        .await,
        "1"
    );
    assert_eq!(
        evaluate(view, "document.querySelector('section[data-title]').id", cx).await,
        "\"architecture\""
    );
    assert_eq!(
        evaluate(view, "window.fixtureAnchorReached=false;document.getElementById('architecture').scrollIntoView=()=>{window.fixtureAnchorReached=true};document.querySelector('#brief a[href=\"#architecture\"]').click();window.fixtureAnchorReached", cx).await,
        "true"
    );
    assert_eq!(
        evaluate(view, r#"(() => {
const fixture=document.getElementById('alignment-fixture');
try {
  const labels=[...fixture.querySelectorAll('label')];
  return labels.length===4 && labels.every(label=>{
    const style=getComputedStyle(label),text=label.querySelector('span').getBoundingClientRect(),control=label.querySelector('input,select').getBoundingClientRect();
    return label.classList.contains('document-control') && style.display==='flex' && style.alignItems==='center' && Math.abs(text.top+text.height/2-control.top-control.height/2)<=1;
  });
} finally {fixture.remove();}
})()"#, cx).await,
        "true"
    );
    evaluate(
        view,
        "document.querySelector('form').requestSubmit();true",
        cx,
    )
    .await;
    pause(cx).await;
    assert!(
        events(view, cx)
            .iter()
            .any(|event| event["kind"] == "required")
    );
    evaluate(view,"document.querySelector('input[value=node]').checked=true;let note=document.querySelector('textarea');note.value='Retained answer 中文 🙂';note.dispatchEvent(new Event('input',{bubbles:true}));document.querySelector('form').requestSubmit();true",cx).await;
    pause(cx).await;
    let captured = events(view, cx)
        .into_iter()
        .find(|event| event["kind"] == "submit")
        .unwrap();
    assert_eq!(captured["artifact"], json!(content.artifact.id));
    assert_eq!(captured["session"], json!(content.artifact.session));
    assert_eq!(captured["revision"], 1);
    assert_eq!(captured["fields"][0]["values"][0]["value"], "node");
    assert_eq!(
        captured["fields"][1]["values"][0]["value"],
        "Retained answer 中文 🙂"
    );
    assert_eq!(
        evaluate(view, "document.querySelector('textarea').disabled", cx).await,
        "true"
    );
    let light = evaluate(
        view,
        "getComputedStyle(document.documentElement).getPropertyValue('--document-background')",
        cx,
    )
    .await;
    for mode in [ThemeMode::Dark, ThemeMode::Light] {
        let theme = cx
            .update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                page::theme(cx)
            })
            .unwrap();
        evaluate(view,&format!("window.fixtureRetheme=false;window.sailryDocument.retheme({theme}).then(()=>window.fixtureRetheme=true);true"),cx).await;
        until(view, "window.fixtureRetheme", cx).await;
        assert_eq!(
            evaluate(view, "document.querySelector('textarea').value", cx).await,
            "\"Retained answer 中文 🙂\""
        );
        assert_eq!(
            evaluate(view, "document.querySelector('textarea').disabled", cx).await,
            "true"
        );
        if matches!(mode, ThemeMode::Dark) {
            assert_ne!(evaluate(view,"getComputedStyle(document.documentElement).getPropertyValue('--document-background')",cx).await,light);
        }
    }
    evaluate(
        view,
        "window.sailryDocument.submitted('decisions',false);true",
        cx,
    )
    .await;
    assert_eq!(
        evaluate(view, "document.querySelector('textarea').disabled", cx).await,
        "false"
    );
    evaluate(view,"document.querySelector('form').requestSubmit();window.sailryDocument.submitted('decisions',true);true",cx).await;
    assert_eq!(
        evaluate(view, "document.querySelector('textarea').disabled", cx).await,
        "true"
    );
    evaluate(view,"window.fixtureExport=null;window.sailryDocument.readonlyExport().then(html=>window.fixtureExport=html);true",cx).await;
    until(view, "typeof window.fixtureExport==='string'", cx).await;
    assert_eq!(evaluate(view,"window.fixtureExport.includes('Retained answer 中文 🙂') && !window.fixtureExport.includes('<form') && window.fixtureExport.includes('<svg')",cx).await,"true");
    assert!(
        !events(view, cx)
            .iter()
            .any(|event| event["kind"] == "render_error")
    );
}

fn events(view: &Entity<Harness>, cx: &mut AsyncWindowContext) -> Vec<Value> {
    cx.update(|_, cx| view.read(cx).events.borrow().clone())
        .unwrap()
}
async fn evaluate(view: &Entity<Harness>, script: &str, cx: &mut AsyncWindowContext) -> String {
    evaluate_result(view, script, cx)
        .await
        .expect("native JavaScript evaluation must complete")
}
async fn evaluate_result(
    view: &Entity<Harness>,
    script: &str,
    cx: &mut AsyncWindowContext,
) -> anyhow::Result<String> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let sender = std::sync::Mutex::new(Some(sender));
    cx.update(|_, cx| {
        view.read(cx)
            .native
            .page
            .evaluate_script_with_callback(script, move |value| {
                if let Some(sender) = sender.lock().unwrap().take() {
                    let _ = sender.send(value);
                }
            })
    })??;
    receiver
        .await
        .map_err(|error| anyhow::anyhow!("native JavaScript callback closed: {error}"))
}
async fn until(view: &Entity<Harness>, script: &str, cx: &mut AsyncWindowContext) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if evaluate(view, script, cx).await == "true" {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "native document state deadline: {script}, events: {:?}",
            events(view, cx)
        );
        pause(cx).await;
    }
}
async fn pause(cx: &mut AsyncWindowContext) {
    cx.background_executor()
        .timer(Duration::from_millis(100))
        .await;
    cx.update(|window, cx| window.draw(cx).clear(cx)).unwrap();
}
