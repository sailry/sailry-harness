import 'package:flutter/material.dart';

import 'tool_row.dart';

/// A turn's steps collapse under one summary row, as in the desktop transcript.
/// Expansion is presentation state and starts collapsed, also while running.
/// A keyed disclosure keeps its expansion in the route's [PageStorage], so it
/// survives while the lazy transcript disposes the scrolled-away turn.
class StepDisclosure extends StatefulWidget {
  const StepDisclosure({
    super.key,
    required this.label,
    this.detail,
    required this.children,
  });

  final String label;
  final String? detail;
  final List<Widget> children;

  @override
  State<StepDisclosure> createState() => _StepDisclosureState();
}

class _StepDisclosureState extends State<StepDisclosure> {
  final _controller = ExpansibleController();

  /// An explicit identifier keeps unrelated scrollables in the turn from
  /// sharing storage, which an ancestor [PageStorageKey] would cause.
  Object? get _storage =>
      widget.key == null ? null : (StepDisclosure, widget.key);

  @override
  void initState() {
    super.initState();
    final storage = _storage;
    if (storage != null &&
        PageStorage.maybeOf(context)?.readState(context, identifier: storage) ==
            true) {
      _controller.expand();
    }
    _controller.addListener(_store);
  }

  void _store() {
    final storage = _storage;
    if (storage == null) return;
    PageStorage.maybeOf(
      context,
    )?.writeState(context, _controller.isExpanded, identifier: storage);
  }

  @override
  void dispose() {
    _controller.removeListener(_store);
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => Expansible(
    controller: _controller,
    headerBuilder: (context, animation) => Semantics(
      button: true,
      expanded: _controller.isExpanded,
      child: ToolRow(
        icon: 'task',
        label: widget.label,
        detail: widget.detail,
        disclosure: animation.drive(Tween(begin: 0.0, end: .25)),
        onTap: _controller.toggle,
      ),
    ),
    bodyBuilder: (context, animation) =>
        RowBranch(children: [StepList(children: widget.children)]),
  );
}

/// The tallest expanded step list: about 40% of the screen, bounded so short
/// screens keep a few rows and tall ones keep the answer in reach.
double stepListHeight(BuildContext context) =>
    (MediaQuery.sizeOf(context).height * .4).clamp(160.0, 420.0);

/// Expanded steps scroll inside a bounded height and build near the viewport.
/// The list opens on the latest step and follows new ones until the user
/// scrolls away from the end; returning to the end resumes following.
class StepList extends StatefulWidget {
  const StepList({super.key, required this.children});

  final List<Widget> children;

  @override
  State<StepList> createState() => _StepListState();
}

class _StepListState extends State<StepList> {
  // A restored offset would open on an arbitrary step rather than the latest.
  final _scroll = ScrollController(keepScrollOffset: false);
  bool _following = true;
  bool _scheduled = false;

  bool get _atEnd {
    final position = _scroll.position;
    return position.pixels >= position.maxScrollExtent - 1;
  }

  @override
  void initState() {
    super.initState();
    _follow();
  }

  @override
  void didUpdateWidget(covariant StepList oldWidget) {
    super.didUpdateWidget(oldWidget);
    _follow();
  }

  // Lazy extents are estimates until the last rows lay out, so the jump
  // repeats on metric changes until it reaches the exact end.
  void _follow() {
    if (!_following || _scheduled) return;
    _scheduled = true;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      _scheduled = false;
      if (!mounted || !_following || !_scroll.hasClients || _atEnd) return;
      _scroll.jumpTo(_scroll.position.maxScrollExtent);
    });
    WidgetsBinding.instance.ensureVisualUpdate();
  }

  // Only the user's drags and settled scrolls decide whether to follow, so a
  // jump or a growing list never pulls a reader back to the end.
  bool _scrolled(ScrollNotification notification) {
    if (notification.depth != 0 || !_scroll.hasClients) return false;
    final user = switch (notification) {
      ScrollUpdateNotification(:final dragDetails) => dragDetails != null,
      OverscrollNotification(:final dragDetails) => dragDetails != null,
      ScrollEndNotification() => true,
      _ => false,
    };
    if (user) _following = _atEnd;
    return false;
  }

  bool _resized(ScrollMetricsNotification notification) {
    if (notification.depth == 0 && _scroll.hasClients && !_atEnd) _follow();
    return false;
  }

  @override
  void dispose() {
    _scroll.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final children = widget.children;
    return ConstrainedBox(
      constraints: BoxConstraints(maxHeight: stepListHeight(context)),
      child: NotificationListener<ScrollMetricsNotification>(
        onNotification: _resized,
        child: NotificationListener<ScrollNotification>(
          onNotification: _scrolled,
          child: ListView.builder(
            key: const ValueKey('step-list'),
            controller: _scroll,
            primary: false,
            shrinkWrap: true,
            padding: EdgeInsets.zero,
            itemCount: children.length,
            itemBuilder: (context, index) => children[index],
            findChildIndexCallback: (key) {
              final index = children.indexWhere((child) => child.key == key);
              return index < 0 ? null : index;
            },
          ),
        ),
      ),
    );
  }
}
