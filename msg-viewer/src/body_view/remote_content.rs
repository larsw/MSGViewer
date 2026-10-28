//! The content filter that keeps Remote content from loading (ADR-0004).
//!
//! WebKit applies it to every load the web view makes, below the Body's
//! Content Security Policy, so it holds even where a policy doesn't reach.

use std::cell::RefCell;

use gtk::{gio, glib};
use webkit6::{UserContentFilter, UserContentFilterStore};

use crate::config;

/// Names the compiled filter in the store. It is compiled afresh at every
/// start, so a filter stored by an older version is never used.
const FILTER_ID: &str = "block-remote-content";

/// WebKit content blocker rules: block every load, then let through the Body
/// itself (`about:`), what is embedded in it (`data:`, `blob:`) and Inline
/// attachments (`cid:`). WebKit's rules can't express "one of these
/// schemes", so each has a rule of its own.
const RULES: &str = r#"[
  {"trigger": {"url-filter": ".*"}, "action": {"type": "block"}},
  {"trigger": {"url-filter": "^about:"}, "action": {"type": "ignore-previous-rules"}},
  {"trigger": {"url-filter": "^data:"}, "action": {"type": "ignore-previous-rules"}},
  {"trigger": {"url-filter": "^blob:"}, "action": {"type": "ignore-previous-rules"}},
  {"trigger": {"url-filter": "^cid:"}, "action": {"type": "ignore-previous-rules"}}
]"#;

type Waiter = Box<dyn FnOnce(Option<UserContentFilter>)>;

enum State {
    NotCompiled,
    Compiling(Vec<Waiter>),
    Compiled(Option<UserContentFilter>),
}

thread_local! {
    static FILTER: RefCell<State> = const { RefCell::new(State::NotCompiled) };
}

/// Calls `ready` with the content filter blocking Remote content, once it
/// has been compiled. The filter is compiled once for the whole app, on the
/// first call; `ready` gets `None` if that failed.
pub fn with_blocking_filter(ready: impl FnOnce(Option<UserContentFilter>) + 'static) {
    let start = FILTER.with_borrow_mut(|state| match state {
        State::Compiled(filter) => {
            let filter = filter.clone();
            Some(Box::new(move || ready(filter)) as Box<dyn FnOnce()>)
        }
        State::Compiling(waiters) => {
            waiters.push(Box::new(ready));
            None
        }
        State::NotCompiled => {
            *state = State::Compiling(vec![Box::new(ready)]);
            Some(Box::new(compile) as Box<dyn FnOnce()>)
        }
    });
    // Called outside the borrow, since `ready` may ask for the filter again.
    if let Some(start) = start {
        start();
    }
}

fn compile() {
    let path = glib::user_cache_dir()
        .join(config::APP_ID)
        .join("content-filters");
    if let Err(error) = std::fs::create_dir_all(&path) {
        glib::g_warning!("msg-viewer", "Could not create {}: {error}", path.display());
    }
    let store = UserContentFilterStore::new(&path.to_string_lossy());
    let rules = glib::Bytes::from_static(RULES.as_bytes());
    store.save(FILTER_ID, &rules, gio::Cancellable::NONE, |result| {
        let filter = result
            .inspect_err(|error| {
                glib::g_warning!(
                    "msg-viewer",
                    "Could not compile the content filter for Remote content: {error}"
                );
            })
            .ok();
        let state = FILTER.replace(State::Compiled(filter.clone()));
        if let State::Compiling(waiters) = state {
            for ready in waiters {
                ready(filter.clone());
            }
        }
    });
}
