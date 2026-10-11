// Modified by Longbridge for gpui-fast.
//! Telling scrolls and offset reads apart from other changes, and deciding
//! whether a frame only scrolled a layer (M4).
//!
//! A wheel scroll mutates a container's offset in place and notifies the view
//! that painted it, which looks like any other change. Here it is noted for
//! the container it moved ([`note_scrolled`]), and reads of an offset through
//! the scroll getters ([`note_offset_read`]) are recorded with what a view
//! read, so that a view whose output depends on an offset is told apart from
//! one that was only built again because it holds a scroll container.
//!
//! The wheel listener's notification of the view holding the container is
//! told apart from any other notification of it by counting both: the view
//! is dirty only through the scroll when it was notified no more often than
//! the wheel scrolled what it holds ([`OwnerWatch`]), and no container
//! inside the layer's content scrolled, which the count cannot tell apart.

use std::{cell::RefCell, ops::Range, rc::Rc};

use collections::{FxHashMap, FxHashSet};

use crate::fast::dependencies::{RenderDependencies, StateVersion};
use crate::fast::layers::COMPILED;
use crate::fast::layers::record::LayerRecord;
use crate::{App, EntityId, GlobalElementId, Interactivity, PrepaintStateIndex, Window};

/// Where a scroll container's offset lives, which scrolls and reads of it are
/// noted under.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ScrollSource {
    /// Scroll state shared outside the element — a [`crate::ScrollHandle`], a list's
    /// state — by the address of its version counter.
    Handle(usize),
    /// A container's own element state, which nothing outside it reads.
    Container(GlobalElementId),
}

impl ScrollSource {
    /// The source of the shared scroll state `version` counts changes of.
    pub(crate) fn of_state(version: &StateVersion) -> Self {
        ScrollSource::Handle(version.id())
    }
}

/// A scroll container as its wheel listener knows it: its id, where its
/// offset lives and the view that painted it, which the listener notifies.
/// Nothing where layers are not compiled. See [`painted_container`].
#[derive(Clone)]
pub(crate) struct ScrollContainer(Option<Container>);

#[derive(Clone)]
struct Container {
    id: GlobalElementId,
    source: ScrollSource,
    version: Option<StateVersion>,
    view: EntityId,
}

/// The scrolls of the frame being drawn, and the scroll containers painted.
#[derive(Default)]
pub(crate) struct ScrollLog {
    /// The scroll containers a wheel scrolled since the last frame was drawn.
    pub(crate) scrolled: FxHashSet<GlobalElementId>,
    /// Where the offsets a wheel moved since the last frame live: those of
    /// `scrolled`, and of lists, which have no id.
    scrolled_sources: FxHashSet<ScrollSource>,
    /// The scroll containers painted lately, by id.
    containers: FxHashMap<GlobalElementId, PaintedContainer>,
    /// How many times a wheel listener notified each view since the last
    /// frame was drawn, for having scrolled a container it painted.
    scroll_notifies: FxHashMap<EntityId, u64>,
    /// The views that asked for an animation frame while this frame was
    /// being drawn, and while the last one was. See [`note_animation_frame`].
    animation_frames: RefCell<FxHashSet<EntityId>>,
    animation_frames_before: FxHashSet<EntityId>,
    /// Where anchored elements were prepainted this frame, by the id of the
    /// element around them. See [`note_anchored`].
    pub(crate) anchored: Vec<GlobalElementId>,
}

/// What [`ScrollLog`] keeps of a scroll container it saw painted.
struct PaintedContainer {
    source: ScrollSource,
    /// The view that painted it, which a scroll of it notifies.
    view: EntityId,
    /// Its shared scroll state's version, and the version it was at when it
    /// was painted, if it has shared state.
    version: Option<(StateVersion, u64)>,
    /// The frame it was last painted in.
    frame: u64,
}

/// How long a scroll container that is not painted is remembered, in frames:
/// one inside a view drawn again from last frame is not painted either.
const FORGET_AFTER_FRAMES: u64 = 120;

impl ScrollLog {
    /// The scroll containers painted lately.
    #[cfg(test)]
    pub(crate) fn containers(&self) -> impl Iterator<Item = &GlobalElementId> {
        self.containers.keys()
    }

    /// Where the offset of the scroll container `id` lives, if it was
    /// painted lately.
    pub(crate) fn source(&self, id: &GlobalElementId) -> Option<ScrollSource> {
        self.containers
            .get(id)
            .map(|container| container.source.clone())
    }

    fn remember(&mut self, container: &Container, frame: u64) {
        let version = container
            .version
            .as_ref()
            .map(|version| (version.clone(), version.get()));
        self.containers.insert(
            container.id.clone(),
            PaintedContainer {
                source: container.source.clone(),
                view: container.view,
                version,
                frame,
            },
        );
    }

    /// Takes in the offsets code set since the last time (a scrollbar being
    /// dragged, a `scroll_to_*`), as a wheel's scroll of the containers
    /// whose offset they are: the view that painted each may be notified
    /// for it, as a wheel listener notifies it.
    pub(crate) fn take_offsets_set(&mut self) {
        let sources = OFFSETS_SET.with_borrow_mut(std::mem::take);
        for source in sources {
            for (id, container) in &self.containers {
                if container.source == source {
                    *self.scroll_notifies.entry(container.view).or_default() += 1;
                    self.scrolled.insert(id.clone());
                }
            }
            self.scrolled_sources.insert(source);
        }
    }

    /// Ends the frame `frame`: the scrolls before it are taken in, and scroll
    /// containers neither painted lately nor holding a layer are forgotten.
    pub(crate) fn finish_frame(&mut self, frame: u64, keep: impl Fn(&GlobalElementId) -> bool) {
        self.scrolled.clear();
        self.scrolled_sources.clear();
        self.scroll_notifies.clear();
        self.animation_frames_before = std::mem::take(self.animation_frames.get_mut());
        self.anchored.clear();
        self.containers
            .retain(|id, container| container.frame + FORGET_AFTER_FRAMES > frame || keep(id));
    }
}

/// The scroll container whose scroll listener is being painted: the element
/// at the top of the element id stack, `element` its interactivity, with
/// the scroll handle it tracks, if any.
/// It is remembered as painted, for scrolls and reads of it to be told apart.
pub(crate) fn painted_container(window: &mut Window, element: &Interactivity) -> ScrollContainer {
    if !COMPILED {
        return ScrollContainer(None);
    }
    let id = crate::fast::global_id::current(window);
    let (source, version) = match element.tracked_scroll_handle.as_ref() {
        Some(handle) => {
            let version = handle.0.borrow().version.clone();
            (ScrollSource::of_state(&version), Some(version))
        }
        None => (ScrollSource::Container(id.clone()), None),
    };
    let container = Container {
        id,
        source,
        version,
        view: window.current_view(),
    };
    let frame = window.fast_layers.frame;
    window.fast_layers.scrolls.remember(&container, frame);
    ScrollContainer(Some(container))
}

/// Notes that a wheel moved `container`'s offset, as its scroll listener
/// does before it notifies the view that painted the container, for the next
/// frame to tell the scroll apart from other changes.
pub(crate) fn note_scrolled(window: &mut Window, container: &ScrollContainer) {
    let Some(container) = &container.0 else {
        return;
    };
    let scrolls = &mut window.fast_layers.scrolls;
    *scrolls.scroll_notifies.entry(container.view).or_default() += 1;
    scrolls.scrolled.insert(container.id.clone());
    scrolls.scrolled_sources.insert(container.source.clone());
    if !scrolls.containers.contains_key(&container.id) {
        let frame = window.fast_layers.frame;
        window.fast_layers.scrolls.remember(container, frame);
    }
}

/// Notes that a wheel moved the list whose state `version` counts changes
/// of, as its scroll listener does before it notifies `view`, the view that
/// painted it. A list has no id; its offset is known by its state, and the
/// list by the id [`painted_list`] remembered it under, if it was painted.
pub(crate) fn note_list_scrolled(window: &mut Window, version: &StateVersion, view: EntityId) {
    if !COMPILED {
        return;
    }
    let scrolls = &mut window.fast_layers.scrolls;
    scrolls
        .scrolled_sources
        .insert(ScrollSource::of_state(version));
    let list = scrolls.containers.iter().find_map(|(id, container)| {
        container
            .version
            .as_ref()
            .is_some_and(|(painted, _)| painted.id() == version.id())
            .then(|| id.clone())
    });
    if let Some(id) = list {
        *scrolls.scroll_notifies.entry(view).or_default() += 1;
        scrolls.scrolled.insert(id);
    }
}

/// Remembers the list whose state `version` counts changes of as a scroll
/// container painted under the id `id`, which lists, having no id of their
/// own, are given (see [`crate::fast::layers::lists`]). Its offset lives in
/// its state, but a change of the state's version (a splice, a remeasure,
/// a programmatic scroll) is taken for a change of its content, not for a
/// scroll: the version counts both.
pub(crate) fn painted_list(window: &mut Window, id: &GlobalElementId, version: &StateVersion) {
    let container = Container {
        id: id.clone(),
        source: ScrollSource::Container(id.clone()),
        version: Some(version.clone()),
        view: window.current_view(),
    };
    let frame = window.fast_layers.frame;
    window.fast_layers.scrolls.remember(&container, frame);
}

/// Whether the scroll container `id` is a `list`, which has no id of its
/// own nor a scroll handle: its offset lives in its state (see
/// [`painted_list`]).
pub(crate) fn is_list(window: &Window, id: &GlobalElementId) -> bool {
    window
        .fast_layers
        .scrolls
        .containers
        .get(id)
        .is_some_and(|container| {
            matches!(container.source, ScrollSource::Container(_)) && container.version.is_some()
        })
}

/// Whether the scroll container `id` scrolled since the last frame was
/// drawn: a wheel moved it, or its shared scroll state changed, as
/// [`crate::ScrollHandle::set_offset`] and the `scroll_to_…` methods change it.
pub(crate) fn scrolled(window: &Window, id: &GlobalElementId) -> bool {
    let scrolls = &window.fast_layers.scrolls;
    scrolls.scrolled.contains(id)
        || scrolls.containers.get(id).is_some_and(|container| {
            container
                .version
                .as_ref()
                .is_some_and(|(version, painted_at)| version.get() != *painted_at)
        })
}

/// The offsets read through the scroll getters while dependencies are being
/// recorded, with the version of the state each was read at.
///
/// The getters take no context to record into, so the log is kept by the
/// thread, which is the one thread windows are drawn on, and opened and
/// closed with the app's recordings. See
/// [`crate::App::begin_recording_dependencies`].
#[derive(Default)]
struct OffsetReadLog {
    recordings: usize,
    reads: Vec<(StateVersion, u64)>,
}

thread_local! {
    static OFFSET_READS: RefCell<OffsetReadLog> = RefCell::new(OffsetReadLog::default());
    /// Where offsets that code set live, since [`ScrollLog::take_offsets_set`]
    /// last took them in. Offsets are set without a window to note them in.
    static OFFSETS_SET: RefCell<Vec<ScrollSource>> = const { RefCell::new(Vec::new()) };
}

/// Sets that the scroll state `version` counts changes of moved when
/// `moved`, as `StateVersion::bump_if` does, and notes it as a scroll: code
/// that sets an offset (a scrollbar being dragged) then notifies the view
/// holding the container, as a wheel listener does.
pub(crate) fn offset_set(version: &StateVersion, moved: bool) {
    crate::fast::dependencies::StateVersion::bump_if(version, moved);
    if COMPILED && moved {
        OFFSETS_SET.with_borrow_mut(|set| set.push(ScrollSource::of_state(version)));
    }
}

/// Records, for any recording that is open, that the offset of the scroll
/// state `version` counts changes of was read.
#[inline]
pub(crate) fn note_offset_read(version: &StateVersion) {
    if !COMPILED {
        return;
    }
    OFFSET_READS.with_borrow_mut(|log| {
        if log.recordings > 0 {
            log.reads.push((version.clone(), version.get()));
        }
    });
}

/// Opens a recording of offset reads, returning where in the log it starts.
pub(crate) fn begin_offset_reads() -> usize {
    OFFSET_READS.with_borrow_mut(|log| {
        log.recordings += 1;
        log.reads.len()
    })
}

/// Where the offset read log ends.
pub(crate) fn offset_reads_len() -> usize {
    OFFSET_READS.with_borrow(|log| log.reads.len())
}

/// The offsets read in `range` of the log, and those read in it outside the
/// stretches in `nested`, which lie within it, in order.
pub(crate) fn offset_reads_in<'a>(
    range: &Range<usize>,
    nested: impl Iterator<Item = &'a Range<usize>>,
) -> (OffsetReads, OffsetReads) {
    if range.is_empty() {
        return (OffsetReads::default(), OffsetReads::default());
    }
    OFFSET_READS.with_borrow(|log| {
        let all = OffsetReads::of(&log.reads[range.clone()]);
        let mut own = Vec::new();
        let mut cursor = range.start;
        for nested in nested {
            if nested.start > cursor {
                own.extend_from_slice(&log.reads[cursor..nested.start]);
            }
            cursor = cursor.max(nested.end);
        }
        if range.end > cursor {
            own.extend_from_slice(&log.reads[cursor..range.end]);
        }
        (all, OffsetReads::of(&own))
    })
}

/// Closes the innermost recording of offset reads, emptying the log once
/// none is open.
pub(crate) fn end_offset_reads() {
    OFFSET_READS.with_borrow_mut(|log| {
        log.recordings = log.recordings.saturating_sub(1);
        if log.recordings == 0 {
            log.reads.clear();
        }
    });
}

/// Tells any recording that is open that `reads` were read again, as they
/// are when a subtree built from them is reused, returning the stretch of
/// the log they took up.
pub(crate) fn replay_offset_reads(reads: &OffsetReads) -> Range<usize> {
    OFFSET_READS.with_borrow_mut(|log| {
        let start = log.reads.len();
        if log.recordings > 0
            && let Some(reads) = &reads.0
        {
            log.reads.extend(reads.iter().cloned());
        }
        start..log.reads.len()
    })
}

/// The scroll offsets a retained subtree read, once each, at the earliest
/// version read. Most subtrees read none, which takes no allocation.
#[derive(Clone, Default)]
pub(crate) struct OffsetReads(Option<Rc<[(StateVersion, u64)]>>);

impl OffsetReads {
    fn of(reads: &[(StateVersion, u64)]) -> Self {
        if reads.is_empty() {
            return OffsetReads(None);
        }
        let mut unique: Vec<(StateVersion, u64)> = Vec::with_capacity(reads.len());
        for read in reads {
            if !unique
                .iter()
                .any(|(version, _)| version.id() == read.0.id())
            {
                unique.push(read.clone());
            }
        }
        OffsetReads(Some(unique.into()))
    }

    fn iter(&self) -> impl Iterator<Item = &(StateVersion, u64)> {
        self.0.iter().flat_map(|reads| reads.iter())
    }

    /// Both sets of reads.
    pub(crate) fn union(&self, other: &Self) -> Self {
        match (&self.0, &other.0) {
            (_, None) => self.clone(),
            (None, _) => other.clone(),
            (Some(a), Some(b)) => {
                let mut reads = a.to_vec();
                reads.extend(b.iter().cloned());
                OffsetReads::of(&reads)
            }
        }
    }
}

/// Whether `dependencies` include a read of the offset that lives at
/// `source`.
pub(crate) fn render_read_offset(dependencies: &RenderDependencies, source: &ScrollSource) -> bool {
    match source {
        ScrollSource::Handle(id) => dependencies
            .offset_reads
            .iter()
            .any(|(version, _)| version.id() == *id),
        ScrollSource::Container(_) => false,
    }
}

/// Whether an offset `dependencies` include a read of has moved since: a
/// wheel scrolled it since the last frame, or its shared state changed.
/// A view that read an offset is built again when it scrolls, as it would
/// be for any other state it read.
pub(crate) fn offset_read_changed(window: &Window, dependencies: &RenderDependencies) -> bool {
    if !COMPILED {
        return false;
    }
    let scrolled = &window.fast_layers.scrolls.scrolled_sources;
    dependencies.offset_reads.iter().any(|(version, read_at)| {
        version.get() != *read_at
            || (!scrolled.is_empty() && scrolled.contains(&ScrollSource::of_state(version)))
    })
}

/// `dependencies` without the version of the scroll state at `source`, if
/// they hold it: a scroll of the container is what a layer is composited
/// for, not a change.
fn without_scroll_state(
    dependencies: &RenderDependencies,
    source: Option<&ScrollSource>,
) -> Option<RenderDependencies> {
    let Some(ScrollSource::Handle(id)) = source else {
        return None;
    };
    if !dependencies
        .states
        .iter()
        .any(|(version, _)| version.id() == *id)
    {
        return None;
    }
    let states: Vec<_> = dependencies
        .states
        .iter()
        .filter(|(version, _)| version.id() != *id)
        .cloned()
        .collect();
    Some(RenderDependencies {
        states: states.into(),
        ..dependencies.clone()
    })
}

/// The view whose element is being prepainted: the one holding the scroll
/// container asking.
fn owner(window: &Window) -> Option<&GlobalElementId> {
    window.retained_state.subtree_stack.last()
}

/// Whether `dependencies`, the scroll state at `source` aside, changed since
/// they were recorded, or name an entity notified since the last frame
/// other than the view holding the container, whose notification
/// [`owner_notified_otherwise`] accounts for.
fn changed(
    window: &Window,
    cx: &App,
    dependencies: &RenderDependencies,
    source: Option<&ScrollSource>,
) -> bool {
    let trimmed = without_scroll_state(dependencies, source);
    let dependencies = trimmed.as_ref().unwrap_or(dependencies);
    let notified = &window.retained_state.notified_entities;
    let owner = owner(window).and_then(crate::fast::splice::view_entity);
    cx.dependencies_changed(dependencies, window.inside_notified_view())
        || offset_read_changed(window, dependencies)
        || (!notified.is_empty()
            && dependencies
                .entities
                .iter()
                .any(|entity| Some(*entity) != owner && notified.contains(entity)))
}

/// `dependencies` without `entity`, if they name it.
pub(crate) fn without_entity(
    dependencies: &RenderDependencies,
    entity: Option<EntityId>,
) -> Option<RenderDependencies> {
    let entity = entity?;
    if !dependencies.entities.contains(&entity) {
        return None;
    }
    let entities: Vec<_> = dependencies
        .entities
        .iter()
        .copied()
        .filter(|other| *other != entity)
        .collect();
    Some(RenderDependencies {
        entities: entities.into(),
        ..dependencies.clone()
    })
}

/// The view holding the scroll container being prepainted.
pub(crate) fn owner_view(window: &Window) -> Option<EntityId> {
    owner(window).and_then(crate::fast::splice::view_entity)
}

/// Whether the view holding the scroll container `id` was notified since
/// the last frame for anything other than a wheel scroll: more often than
/// wheel listeners of containers it painted notified it. Without a count of
/// its notifications, any notification is taken for a change.
fn owner_notified_otherwise(window: &Window, id: &GlobalElementId) -> bool {
    let Some(owner) = owner_view(window) else {
        return false;
    };
    let notifies = window
        .fast_layers
        .layers
        .get(id)
        .and_then(|layer| layer.policy.owner_notifies_since(owner));
    match notifies {
        Some(notifies) => {
            let scrolls = &window.fast_layers.scrolls.scroll_notifies;
            notifies > scrolls.get(&owner).copied().unwrap_or(0)
        }
        None => window.retained_state.notified_entities.contains(&owner),
    }
}

/// Whether the view holding the scroll container `id`, or a view drawn
/// inside the container last frame, asked for an animation frame while this
/// frame or the last was drawn: what it animates changes every frame.
pub(crate) fn animation_frame_requested(window: &Window, id: &GlobalElementId) -> bool {
    let scrolls = &window.fast_layers.scrolls;
    let requested = scrolls.animation_frames.borrow();
    if requested.is_empty() && scrolls.animation_frames_before.is_empty() {
        return false;
    }
    let animating = |view: EntityId| {
        requested.contains(&view) || scrolls.animation_frames_before.contains(&view)
    };
    owner_view(window).is_some_and(animating)
        || any_content_view(window, id, animating)
        || crate::fast::layers::lists::any_held_view(window, id, animating)
}

/// Notes that the view `view` asked for an animation frame, as
/// [`Window::request_animation_frame`] does.
pub(crate) fn note_animation_frame(window: &Window, view: EntityId) {
    if COMPILED {
        window
            .fast_layers
            .scrolls
            .animation_frames
            .borrow_mut()
            .insert(view);
    }
}

/// Notes that an anchored element is being prepainted: it is placed against
/// the window's edges, where a layer composited at another offset would not
/// keep it.
/// Notes that the layer being painted, if any, has content that hands its
/// children's bounds to code outside it while it is prepainted (a
/// children-prepainted listener): skipping that on composited frames would
/// leave that code with stale bounds, so the container is kept off its
/// layer as for an anchored element.
pub(crate) fn note_uncarried(window: &mut Window) {
    if COMPILED && let Some(id) = window.fast_layers.painting.as_ref().map(|p| p.id.clone()) {
        window.fast_layers.scrolls.anchored.push(id);
    }
}

pub(crate) fn note_anchored(window: &mut Window) {
    if COMPILED && !window.fast_layers.layers.is_empty() {
        let id = crate::fast::global_id::current(window);
        // A list has no id its rows' ids start with (see
        // `lists::content_prefix`): the layer being painted is noted too.
        let painting = window.fast_layers.painting.as_ref().map(|p| p.id.clone());
        let scrolls = &mut window.fast_layers.scrolls;
        scrolls.anchored.push(id);
        scrolls.anchored.extend(painting);
    }
}

/// How often each view holding a layer's container was notified, by any
/// cause, counted for as long as an [`OwnerWatch`] of it lives. Views are
/// notified through the app, which has no window to count in.
struct OwnerNotifies {
    watches: u32,
    notifies: u64,
}

thread_local! {
    static OWNER_NOTIFIES: RefCell<FxHashMap<EntityId, OwnerNotifies>> =
        RefCell::new(FxHashMap::default());
}

/// Counts, while it lives, how often the view holding a layer's container
/// is notified.
pub(crate) struct OwnerWatch(EntityId);

impl OwnerWatch {
    pub(crate) fn new(view: EntityId) -> Self {
        OWNER_NOTIFIES.with_borrow_mut(|watched| {
            watched
                .entry(view)
                .or_insert(OwnerNotifies {
                    watches: 0,
                    notifies: 0,
                })
                .watches += 1;
        });
        OwnerWatch(view)
    }

    /// The view watched.
    pub(crate) fn view(&self) -> EntityId {
        self.0
    }

    /// How often the view has been notified since any watch of it began.
    pub(crate) fn notifies(&self) -> u64 {
        OWNER_NOTIFIES.with_borrow(|watched| watched.get(&self.0).map_or(0, |n| n.notifies))
    }
}

impl Drop for OwnerWatch {
    fn drop(&mut self) {
        let _ = OWNER_NOTIFIES.try_with(|watched| {
            let mut watched = watched.borrow_mut();
            if let Some(notifies) = watched.get_mut(&self.0) {
                notifies.watches -= 1;
                if notifies.watches == 0 {
                    watched.remove(&self.0);
                }
            }
        });
    }
}

/// Counts a notification of `entity`, if it is a watched view. See
/// [`crate::App::notify`].
#[inline]
pub(crate) fn note_notify(entity: EntityId) {
    if COMPILED {
        OWNER_NOTIFIES.with_borrow_mut(|watched| {
            if let Some(notifies) = watched.get_mut(&entity) {
                notifies.notifies += 1;
            }
        });
    }
}

/// Whether the frame being drawn only scrolled the layer of the scroll
/// container `id`, whose content `record` holds (spec §6.4):
///
/// - nothing the content read changed — no view nested in it is notified or
///   read anything that changed — and no hover it was painted by did;
/// - the view holding the container is clean, or dirty only because `id`
///   scrolled: nothing it read itself changed, it was notified only by the
///   scroll, and its render did not read `id`'s offset.
///
/// What the container itself is painted with — its bounds, content mask,
/// text style, opacity — is checked by [`crate::fast::layers::policy::decide`].
pub(crate) fn scroll_only(
    window: &Window,
    cx: &App,
    id: &GlobalElementId,
    record: &LayerRecord,
) -> bool {
    let source = window.fast_layers.scrolls.source(id);
    !changed(window, cx, &record.dependencies, source.as_ref())
        && window.hovers_unchanged(&record.hovers)
        && !nested_container_scrolled(window, id)
        && !content_view_notified(window, record)
        && owner_scrolled_only(window, cx, id, source.as_ref())
}

/// The views drawn inside a scroll container's content this frame, which
/// prepainting it, over `prepaint`, added to the dispatch tree: they render
/// while the view holding the container lays out, before its content is
/// recorded, so what they read is not the record's.
pub(crate) fn content_views(
    window: &Window,
    prepaint: &Range<PrepaintStateIndex>,
) -> Rc<[EntityId]> {
    let nodes = &window.next_frame.dispatch_tree.nodes;
    let start = prepaint.start.dispatch_tree_index.min(nodes.len());
    let end = prepaint.end.dispatch_tree_index.clamp(start, nodes.len());
    nodes[start..end]
        .iter()
        .filter_map(|node| node.view_id)
        .collect()
}

/// Whether a view drawn inside the content `record` holds was notified, or
/// is dirty, since the last frame. On frames that composite the layer those
/// views are neither prepainted nor painted, so the last frame's retained
/// views and dispatch tree do not hold them: the record remembers them.
fn content_view_notified(window: &Window, record: &LayerRecord) -> bool {
    let notified = &window.retained_state.notified_entities;
    record
        .views
        .iter()
        .any(|view| notified.contains(view) || window.dirty_views.contains(view))
}

/// Whether a scroll container inside the content of the scroll container
/// `id` scrolled since the last frame: a change of the content, not a scroll
/// of it (spec §6.6). Its wheel listener may notify the view holding `id`,
/// which the notification count alone does not tell apart from a scroll of
/// `id`.
fn nested_container_scrolled(window: &Window, id: &GlobalElementId) -> bool {
    let prefix = crate::fast::layers::lists::content_prefix(id);
    let nested = |other: &GlobalElementId| {
        other != id && other.len() > prefix.len() && other.starts_with(prefix)
    };
    let scrolls = &window.fast_layers.scrolls;
    scrolls.scrolled.iter().any(nested)
        || scrolls
            .containers
            .keys()
            .any(|other| nested(other) && scrolled(window, other))
}

/// Whether the view holding the scroll container `id`, whose offset lives
/// at `source`, and the views drawn inside the container are unchanged
/// since the last frame, but for scrolls of `id` its render did not read.
fn owner_scrolled_only(
    window: &Window,
    cx: &App,
    id: &GlobalElementId,
    source: Option<&ScrollSource>,
) -> bool {
    if owner_notified_otherwise(window, id) {
        return false;
    }
    let Some(owner) = owner(window) else {
        return false;
    };
    let Some(index) = window.rendered_frame.retained.find(owner) else {
        // Not drawn last frame as a retained view: nothing tells what it read.
        return false;
    };
    let owner = &window.rendered_frame.retained.records[index];
    // A view drawn inside the content renders while the owner lays out,
    // before the content is recorded: what it read is its own record's.
    // One whose view is dirty, notified or around a view that is, is built
    // again.
    let content_view_dirty = !window.dirty_views.is_empty()
        && any_content_view(window, id, |view| window.dirty_views.contains(&view));
    // What the view read of itself (a list renders its rows as the view
    // holding it) is judged by how often it was notified, above.
    let own = without_entity(&owner.own_dependencies, owner_view(window));
    let own = own.as_ref().unwrap_or(&owner.own_dependencies);
    // Of the offsets the view read, only those its render read can shape the
    // content (spec §6.2). What its elements read while prepainted or
    // painted lies outside the content, which a composited frame neither
    // prepaints nor paints, as a scrollbar beside it does; what the content
    // itself reads is its record's.
    let render_only;
    let own = match &owner.render_offset_reads {
        Some(reads) => {
            render_only = RenderDependencies {
                offset_reads: reads.clone(),
                ..own.clone()
            };
            &render_only
        }
        None => own,
    };
    !content_view_dirty
        && !source.is_some_and(|source| render_read_offset(own, source))
        && !changed(window, cx, own, source)
}

/// Whether any view drawn inside the scroll container `id` last frame, as a
/// retained view nested in the view holding it, is one `f` picks.
fn any_content_view(window: &Window, id: &GlobalElementId, f: impl Fn(EntityId) -> bool) -> bool {
    let Some(index) = owner(window).and_then(|owner| window.rendered_frame.retained.find(owner))
    else {
        return false;
    };
    let records = &window.rendered_frame.retained.records;
    // Nested views' records follow the owner's.
    let nested = &records[index + 1..(index + 1 + records[index].nested).min(records.len())];
    nested.iter().any(|record| {
        record.id.len() > id.len()
            && record.id.starts_with(id)
            && crate::fast::splice::view_entity(&record.id).is_some_and(&f)
    })
}

/// Whether what the content of the scroll container `id` is built from
/// changed since the last frame, as far as it can be told without a layer
/// recording the content: the container is on today's path, and a frame
/// that did not only scroll it keeps a demoted layer waiting.
pub(crate) fn changed_without_layer(window: &Window, cx: &App, id: &GlobalElementId) -> bool {
    let source = window.fast_layers.scrolls.source(id);
    nested_container_scrolled(window, id) || !owner_scrolled_only(window, cx, id, source.as_ref())
}
