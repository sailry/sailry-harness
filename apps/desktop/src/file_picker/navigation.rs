use super::*;

#[derive(Clone, Copy)]
pub(crate) enum Visit {
    Push,
    History(usize),
    Refresh,
}

impl Picker {
    pub(crate) fn navigate(
        &mut self,
        path: Option<String>,
        visit: Visit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.closed || self.busy || !(self.valid)(cx) {
            return;
        }
        self.cancel_children();
        self.generation += 1;
        let generation = self.generation;
        if let Some(stop) = self.read_stop.take() {
            stop.cancel();
        }
        if let Some(client) = self.client.clone() {
            let stop = sailry_link::CancellationToken::new();
            self.read_stop = Some(stop.clone());
            let runtime = cx.global::<crate::backend::Services>().runtime.clone();
            let job = runtime.spawn(read(client, path, stop));
            self.busy = true;
            self.task = Some(cx.spawn_in(window, async move |picker, cx| {
                let result = job.await;
                let _ = picker.update_in(cx, |picker, window, cx| {
                    if picker.closed || picker.generation != generation || !(picker.valid)(cx) {
                        return;
                    }
                    picker.busy = false;
                    picker.read_stop = None;
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
        let changed = self
            .listing
            .as_ref()
            .is_none_or(|value| value.directory.path != listing.directory.path);
        let selected: BTreeSet<_> = self
            .selected
            .iter()
            .filter_map(|index| self.entries().get(*index).map(|entry| entry.name.clone()))
            .collect();
        let focused = self
            .focused
            .and_then(|index| self.entries().get(index).map(|entry| entry.name.clone()));
        let anchor = self
            .anchor
            .and_then(|index| self.entries().get(index).map(|entry| entry.name.clone()));
        self.address
            .update(cx, |input, cx| input.set_value(path.clone(), window, cx));
        self.listing = Some(listing);
        if changed {
            self.filter
                .update(cx, |input, cx| input.set_value("", window, cx));
            self.selected.clear();
            self.focused = None;
            self.anchor = None;
        } else {
            self.selected = self
                .entries()
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| selected.contains(&entry.name).then_some(index))
                .collect();
            self.focused = self
                .entries()
                .iter()
                .position(|entry| Some(&entry.name) == focused.as_ref());
            self.anchor = self
                .entries()
                .iter()
                .position(|entry| Some(&entry.name) == anchor.as_ref());
        }
        self.editing_address = false;
        self.rebuild(cx);
        self.menu_directory = None;
        self.error = None;
        if changed {
            self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        }
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
        if self.closed
            || self.busy
            || !(self.valid)(cx)
            || !self.visible.contains(&index)
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
        let additive = additive
            && !matches!(
                self.mode,
                Mode::Directory | Mode::Files { multiple: false } | Mode::Save { .. }
            );
        if !additive {
            self.selected.clear();
        }
        self.anchor = Some(index);
        if matches!(self.mode, Mode::Save { .. }) && self.entries()[index].kind == EntryKind::File {
            let name = self.entries()[index].name.clone();
            self.filename
                .update(cx, |input, cx| input.set_value(name, window, cx));
        }
        if !self.selected.insert(index) && additive {
            self.selected.remove(&index);
        }
        self.sync_table(cx);
        cx.notify();
    }

    pub(super) fn select(
        &mut self,
        index: usize,
        modifiers: Modifiers,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.closed || self.busy || !(self.valid)(cx) || !self.visible.contains(&index) {
            return;
        }
        if modifiers.shift
            && !matches!(
                self.mode,
                Mode::Directory | Mode::Files { multiple: false } | Mode::Save { .. }
            )
        {
            if let Some((anchor, position)) = self
                .anchor
                .and_then(|anchor| self.visible.iter().position(|value| *value == anchor))
                .zip(self.visible.iter().position(|value| *value == index))
            {
                if !modifiers.platform && !modifiers.control {
                    self.selected.clear();
                }
                let range: Vec<_> = self.visible[anchor.min(position)..=anchor.max(position)]
                    .iter()
                    .copied()
                    .filter(|index| {
                        matches!(
                            self.entries()[*index].kind,
                            EntryKind::Directory | EntryKind::File
                        )
                    })
                    .collect();
                self.selected.extend(range);
                self.focused = Some(index);
                self.focus.focus(window, cx);
                self.error = None;
                self.sync_table(cx);
                cx.notify();
                return;
            }
        }
        self.choose(index, modifiers.platform || modifiers.control, window, cx);
    }

    pub(super) fn activate(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .entries()
            .get(index)
            .is_some_and(|entry| entry.kind == EntryKind::Directory)
        {
            self.navigate(Some(self.entry_path(index)), Visit::Push, window, cx);
        } else if self
            .entries()
            .get(index)
            .is_some_and(|entry| entry.kind == EntryKind::File)
        {
            self.choose(index, false, window, cx);
            if matches!(self.mode, Mode::Files { .. }) {
                self.confirm(window, cx);
            }
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
        if key.key == "escape" && self.editing_address {
            self.editing_address = false;
            self.focus.focus(window, cx);
        } else if (key.modifiers.platform || key.modifiers.control) && key.key == "l" {
            self.editing_address = true;
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
                    if matches!(self.mode, Mode::Directory | Mode::Files { multiple: false }) {
                        cx.stop_propagation();
                        return;
                    }
                    self.selected = self
                        .visible
                        .iter()
                        .copied()
                        .filter(|index| {
                            let entry = &self.entries()[*index];
                            matches!(entry.kind, EntryKind::Directory | EntryKind::File)
                        })
                        .collect();
                    self.sync_table(cx);
                    if matches!(self.mode, Mode::Save { .. }) {
                        self.selected = self
                            .selected
                            .iter()
                            .copied()
                            .find(|index| self.entries()[*index].kind == EntryKind::File)
                            .into_iter()
                            .collect();
                        self.sync_table(cx);
                    }
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
                "left" | "right" | "up" | "down" if !self.visible.is_empty() => {
                    let delta = match key.key.as_str() {
                        "left" => -1,
                        "right" => 1,
                        "up" => -(self.columns as isize),
                        _ => self.columns as isize,
                    };
                    let position = self.focused.and_then(|index| {
                        self.visible.iter().position(|visible| *visible == index)
                    });
                    let row = position
                        .map(|index| {
                            index
                                .saturating_add_signed(delta)
                                .min(self.visible.len() - 1)
                        })
                        .unwrap_or(0);
                    let index = self.visible[row];
                    self.select(index, key.modifiers, window, cx);
                    if self.grid {
                        self.scroll
                            .scroll_to_item(row / self.columns, ScrollStrategy::Top);
                    } else {
                        self.table
                            .update(cx, |table, cx| table.scroll_to_row(row, cx));
                    }
                }
                _ => return,
            }
        }
        cx.stop_propagation();
        cx.notify();
    }
}

pub(super) async fn read(
    client: Arc<Client>,
    path: Option<String>,
    stop: sailry_link::CancellationToken,
) -> Result<Listing, sailry_protocol::Fault> {
    let mut listing: Option<Listing> = None;
    let mut after = None;
    let mut cursors = Vec::new();
    loop {
        let command = Command::BrowseFiles {
            directory: path.clone(),
            after,
        };
        let output = tokio::select! {
            _ = stop.cancelled() => return Err(sailry_protocol::Fault::new(sailry_protocol::ErrorCode::Cancelled, "directory request cancelled")),
            result = client.execute(client.prepare(command)) => result?,
        };
        let Output::FileListing(page) = output else {
            return Err(sailry_protocol::Fault::new(
                sailry_protocol::ErrorCode::Internal,
                "unexpected directory response",
            ));
        };
        after = page.directory.next.clone();
        if let Some(listing) = &mut listing {
            if listing.directory.path != page.directory.path
                || listing.separator != page.separator
                || listing
                    .directory
                    .entries
                    .last()
                    .zip(page.directory.entries.first())
                    .is_some_and(|(last, first)| last.sort_key() >= first.sort_key())
            {
                return Err(sailry_protocol::Fault::new(
                    sailry_protocol::ErrorCode::Internal,
                    "invalid directory continuation",
                ));
            }
            listing.directory.entries.extend(page.directory.entries);
            listing.directory.next = after.clone();
            listing.directory.truncated = page.directory.truncated;
        } else {
            listing = Some(page);
        }
        if after.is_none() {
            return Ok(listing.unwrap());
        }
        if cursors.contains(&after) {
            return Err(sailry_protocol::Fault::new(
                sailry_protocol::ErrorCode::Internal,
                "repeated directory continuation",
            ));
        }
        cursors.push(after.clone());
    }
}
