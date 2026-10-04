//! Kit/Wry expose no standalone cookie-store manager. This macOS adapter shares
//! WebKit's persistent store with tabs and uses completion callbacks without blocking GPUI.
use gpui_kit::{App, Global};
use objc2::{ClassType, MainThreadMarker, rc::Retained, runtime::AnyObject};
use objc2_foundation::*;
use objc2_web_kit::WKWebsiteDataStore;
use std::{cell::RefCell, rc::Rc};

pub(crate) fn persistent_available() -> bool {
    WKWebsiteDataStore::class()
        .class_method(objc2::sel!(dataStoreForIdentifier:))
        .is_some()
}

struct Store {
    directory: Option<std::path::PathBuf>,
    persistent: bool,
    value: Retained<WKWebsiteDataStore>,
}
impl Global for Store {}

pub(crate) fn persistent(cx: &App) -> bool {
    cx.try_global::<crate::preferences::Preferences>()
        .and_then(|preferences| preferences.data.browser_persistent)
        .unwrap_or(true)
}

pub(crate) fn store(cx: &mut App) -> Retained<WKWebsiteDataStore> {
    let directory = cx
        .try_global::<crate::preferences::Preferences>()
        .and_then(|preferences| preferences.directory())
        .map(std::path::Path::to_path_buf);
    let persistent = persistent(cx);
    if let Some(store) = cx.try_global::<Store>()
        && store.directory == directory
        && store.persistent == persistent
    {
        return store.value.clone();
    }
    let value = create(cx);
    cx.set_global(Store {
        directory,
        persistent,
        value: value.clone(),
    });
    value
}

fn create(cx: &App) -> Retained<WKWebsiteDataStore> {
    let thread = MainThreadMarker::new().expect("browser runs on the UI thread");
    let directory = cx
        .try_global::<crate::preferences::Preferences>()
        .and_then(|preferences| preferences.directory());
    unsafe {
        if let Some(directory) = directory.filter(|_| persistent(cx) && persistent_available()) {
            let hash = blake3::hash(directory.as_os_str().as_encoded_bytes());
            let mut bytes: [u8; 16] = hash.as_bytes()[..16].try_into().unwrap();
            bytes[6] = (bytes[6] & 0x0f) | 0x50;
            bytes[8] = (bytes[8] & 0x3f) | 0x80;
            WKWebsiteDataStore::dataStoreForIdentifier(&NSUUID::from_bytes(bytes), thread)
        } else {
            WKWebsiteDataStore::nonPersistentDataStore(thread)
        }
    }
}

pub(crate) fn install(
    store: &WKWebsiteDataStore,
    cookies: &[super::chrome::Cookie],
) -> tokio::sync::oneshot::Receiver<usize> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let values: Vec<_> = cookies.iter().filter_map(cookie).collect();
    let count = values.len();
    if count == 0 {
        let _ = sender.send(0);
        return receiver;
    }
    let pending = Rc::new(RefCell::new((count, Some(sender))));
    let jar = unsafe { store.httpCookieStore() };
    for value in &values {
        let pending = pending.clone();
        unsafe {
            jar.setCookie_completionHandler(
                value,
                Some(&block2::RcBlock::new(move || {
                    let mut state = pending.borrow_mut();
                    state.0 -= 1;
                    if state.0 == 0
                        && let Some(sender) = state.1.take()
                    {
                        let _ = sender.send(count);
                    }
                })),
            );
        }
    }
    receiver
}

/// Verify once after all batches, rather than re-reading the entire store per batch.
pub(crate) fn verify(
    store: &WKWebsiteDataStore,
    cookies: &[super::chrome::Cookie],
) -> tokio::sync::oneshot::Receiver<usize> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let sender = RefCell::new(Some(sender));
    let expected: Vec<_> = cookies.iter().filter_map(cookie).collect();
    unsafe {
        store.httpCookieStore().getAllCookies(&block2::RcBlock::new(
            move |stored: std::ptr::NonNull<NSArray<NSHTTPCookie>>| {
                let stored: std::collections::HashMap<_, _> = stored
                    .as_ref()
                    .iter()
                    .map(|cookie| {
                        (
                            (
                                cookie.domain().to_string(),
                                cookie.path().to_string(),
                                cookie.name().to_string(),
                            ),
                            cookie,
                        )
                    })
                    .collect();
                let count = expected
                    .iter()
                    .filter(|cookie| {
                        stored
                            .get(&(
                                cookie.domain().to_string(),
                                cookie.path().to_string(),
                                cookie.name().to_string(),
                            ))
                            .is_some_and(|saved| {
                                saved.value() == cookie.value()
                                    && saved.isSecure() == cookie.isSecure()
                                    && saved.isHTTPOnly() == cookie.isHTTPOnly()
                            })
                    })
                    .count();
                if let Some(sender) = sender.borrow_mut().take() {
                    let _ = sender.send(count);
                }
            },
        ));
    }
    receiver
}

fn cookie(cookie: &super::chrome::Cookie) -> Option<Retained<NSHTTPCookie>> {
    unsafe {
        let properties: Retained<NSMutableDictionary<NSString, AnyObject>> =
            NSMutableDictionary::new();
        for (key, value) in [
            (NSHTTPCookieName, cookie.name.as_str()),
            (NSHTTPCookieValue, cookie.value.as_str()),
            (NSHTTPCookieDomain, cookie.domain.as_str()),
            (NSHTTPCookiePath, cookie.path.as_str()),
        ] {
            properties.insert(key, &*NSString::from_str(value));
        }
        if cookie.secure {
            properties.insert(NSHTTPCookieSecure, ns_string!("TRUE"));
        }
        if cookie.http_only {
            properties.insert(ns_string!("HttpOnly"), ns_string!("TRUE"));
        }
        if let Some(expires) = cookie.expires {
            properties.insert(
                NSHTTPCookieExpires,
                &*NSDate::dateWithTimeIntervalSince1970(expires as f64),
            );
        }
        match cookie.same_site {
            0 => {
                properties.insert(NSHTTPCookieSameSitePolicy, ns_string!("None"));
            }
            1 => {
                properties.insert(NSHTTPCookieSameSitePolicy, NSHTTPCookieSameSiteLax);
            }
            2 => {
                properties.insert(NSHTTPCookieSameSitePolicy, NSHTTPCookieSameSiteStrict);
            }
            _ => {}
        }
        NSHTTPCookie::cookieWithProperties(&properties)
    }
}
