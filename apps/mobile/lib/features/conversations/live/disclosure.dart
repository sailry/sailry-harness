import 'package:flutter/material.dart';

import 'tool_row.dart';

/// A turn's steps collapse under one summary row, as in the desktop transcript.
/// Expansion is presentation state and starts collapsed, also while running.
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

  @override
  void dispose() {
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
