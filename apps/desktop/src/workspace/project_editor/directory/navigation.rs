use super::*;

#[derive(Clone, Copy)]
pub(super) enum Visit {
    Push,
    History(usize),
    Refresh,
}

impl Picker {
    pub(super) fn navigate(
        &mut self,
        path: Option<String>,
        visit: Visit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            return;
        }
        if let Some(client) = self.client.clone() {
            let runtime = cx.global::<crate::backend::Services>().runtime.clone();
            let job = runtime.spawn(async move {
                let mut listing: Option<Listing> = None;
                let mut after = None;
                loop {
                    let command = Command::BrowseFiles {
                        directory: path.clone(),
                        after,
                    };
                    let Output::FileListing(page) = client.execute(client.prepare(command)).await?
                    else {
                        return Err(sailry_protocol::Fault::new(
                            sailry_protocol::ErrorCode::Internal,
                            "unexpected directory response",
                        ));
                    };
                    after = page.directory.next.clone();
                    if let Some(listing) = &mut listing {
                        listing.directory.entries.extend(page.directory.entries);
                        listing.directory.next = after.clone();
                        listing.directory.truncated = page.directory.truncated;
                    } else {
                        listing = Some(page);
                    }
                    if after.is_none() {
                        return Ok(listing.unwrap());
                    }
                }
            });
            self.busy = true;
            self.task = Some(cx.spawn_in(window, async move |picker, cx| {
                let result = job.await;
                let _ = picker.update_in(cx, |picker, window, cx| {
                    picker.busy = false;
                    match result {
                        Ok(Ok(listing)) => picker.arrive(listing, visit, window, cx),
                        _ => picker.fail("files_directory_failed", window, cx),
                    }
                });
            }));
        } else {
            let path = path.unwrap_or_else(|| "/preview".into());
            match self.catalog.list(&path) {
                Ok(listing) => self.arrive(listing, visit, window, cx),
                Err(error) => self.fail(error, window, cx),
            }
        }
        cx.notify();
    }

    fn arrive(
        &mut self,
        listing: Listing,
        visit: Visit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = &listing.directory.path;
        match visit {
            Visit::Push if self.history.get(self.cursor) != Some(path) => {
                self.history.truncate(self.cursor + 1);
                self.history.push(path.clone());
                self.cursor = self.history.len() - 1;
            }
            Visit::History(cursor) => self.cursor = cursor,
            _ => {}
        }
        self.address
            .update(cx, |input, cx| input.set_value(path.clone(), window, cx));
        self.listing = Some(listing);
        self.selected.clear();
        self.focused = None;
        self.menu_directory = None;
        self.error = None;
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        self.focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn step(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let cursor = if forward {
            self.cursor.checked_add(1)
        } else {
            self.cursor.checked_sub(1)
        };
        if let Some(cursor) = cursor
            && let Some(path) = self.history.get(cursor)
        {
            self.navigate(Some(path.clone()), Visit::History(cursor), window, cx);
        }
    }

    pub(super) fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let path = self
            .listing
            .as_ref()
            .map(|listing| listing.directory.path.clone());
        self.navigate(path, Visit::Refresh, window, cx);
    }

    pub(super) fn up(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self
            .listing
            .as_ref()
            .and_then(|listing| listing.parent.clone())
        {
            self.navigate(Some(path), Visit::Push, window, cx);
        }
    }

    pub(super) fn choose(
        &mut self,
        index: usize,
        additive: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy
            || self
                .entries()
                .get(index)
                .is_none_or(|entry| !matches!(entry.kind, EntryKind::Directory | EntryKind::File))
        {
            return;
        }
        self.focus.focus(window, cx);
        self.focused = Some(index);
        self.error = None;
        self.menu_directory = None;
        if !additive {
            self.selected.clear();
        }
        if !self.selected.insert(index) && additive {
            self.selected.remove(&index);
        }
        cx.notify();
    }

    pub(super) fn activate(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .entries()
            .get(index)
            .is_some_and(|entry| entry.kind == EntryKind::Directory)
        {
            self.navigate(Some(self.entry_path(index)), Visit::Push, window, cx);
        }
    }

    // Kit has no icon-grid selection control. Keep grid navigation here while
    // retaining Kit buttons, focus, input editing and virtual-list scrolling.
    pub(super) fn key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = &event.keystroke;
        if (key.modifiers.platform || key.modifiers.control) && key.key == "l" {
            self.address.update(cx, |input, cx| input.focus(window, cx));
        } else if key.key == "f5" {
            self.refresh(window, cx);
        } else if !self.focus.is_focused(window) || self.busy {
            return;
        } else if key.modifiers.platform || key.modifiers.control {
            match key.key.as_str() {
                "c" => self.copy(false),
                "x" => self.copy(true),
                "v" => {
                    self.menu_directory = None;
                    self.paste(Conflict::Stop, window, cx);
                }
                "a" => {
                    self.selected = self
                        .entries()
                        .iter()
                        .enumerate()
                        .filter_map(|(index, entry)| {
                            matches!(entry.kind, EntryKind::Directory | EntryKind::File)
                                .then_some(index)
                        })
                        .collect();
                }
                _ => return,
            }
        } else {
            match key.key.as_str() {
                "backspace" => self.up(window, cx),
                "enter" => {
                    if let Some(index) = self.focused {
                        self.activate(index, window, cx);
                    }
                }
                "left" | "right" | "up" | "down" if !self.entries().is_empty() => {
                    let delta = match key.key.as_str() {
                        "left" => -1,
                        "right" => 1,
                        "up" => -(self.columns as isize),
                        _ => self.columns as isize,
                    };
                    let index = self
                        .focused
                        .map(|index| {
                            index
                                .saturating_add_signed(delta)
                                .min(self.entries().len() - 1)
                        })
                        .unwrap_or(0);
                    self.choose(index, key.modifiers.shift, window, cx);
                    self.scroll
                        .scroll_to_item(index / self.columns, ScrollStrategy::Top);
                }
                _ => return,
            }
        }
        cx.stop_propagation();
        cx.notify();
    }
}
