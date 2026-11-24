import 'dart:math' show max, min;

import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
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
  bool _hasExpanded = false;

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
    bodyBuilder: (context, animation) {
      if (_controller.isExpanded) _hasExpanded = true;
      return _hasExpanded
          ? RowBranch(children: [StepList(children: widget.children)])
          : const SizedBox.shrink();
    },
  );
}

/// Steps an expanded process shows before Show earlier, and the steps each
/// Show earlier adds above them.
const shownSteps = 8;
const earlierSteps = 50;

/// Expanded steps lay out in the transcript, which stays the page's only
/// scroll: a nested list would hold a finger's drag at its edges. The latest
/// [shownSteps] show, so a growing process keeps its newest step in view,
/// and Show earlier adds earlier steps above them a page at a time.
class StepList extends StatefulWidget {
  const StepList({super.key, required this.children});

  final List<Widget> children;

  @override
  State<StepList> createState() => _StepListState();
}

class _StepListState extends State<StepList> {
  int _more = 0;

  @override
  Widget build(BuildContext context) {
    final children = widget.children;
    final int hidden = max(0, children.length - shownSteps - _more);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (hidden > 0)
          ToolRow(
            key: const ValueKey('earlier-steps'),
            icon: 'arrow-up',
            label: context
                .tr('conversationEarlierSteps')
                .replaceAll('{count}', '${min(hidden, earlierSteps)}'),
            onTap: () => setState(() => _more += earlierSteps),
          ),
        ...children.skip(hidden),
      ],
    );
  }
}
