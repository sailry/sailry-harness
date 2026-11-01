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
    bodyBuilder: (context, animation) => RowBranch(children: widget.children),
  );
}
