import 'package:flutter/material.dart';

import '../../../ui/kit.dart';

/// Transcript expansion is presentation state. Explicit choices override the
/// automatic running/completed defaults, as in the desktop transcript.
class WorkDisclosure extends StatefulWidget {
  const WorkDisclosure({
    super.key,
    required this.title,
    required this.child,
    required this.autoExpanded,
    this.framed = false,
  });

  final Widget title;
  final Widget child;
  final bool autoExpanded;
  final bool framed;

  @override
  State<WorkDisclosure> createState() => _WorkDisclosureState();
}

class _WorkDisclosureState extends State<WorkDisclosure> {
  final _controller = ExpansibleController();
  bool? _chosen;

  @override
  void initState() {
    super.initState();
    if (widget.autoExpanded) _controller.expand();
  }

  @override
  void didUpdateWidget(covariant WorkDisclosure oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.autoExpanded != widget.autoExpanded && _chosen == null) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted || _chosen != null) return;
        widget.autoExpanded ? _controller.expand() : _controller.collapse();
      });
    }
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Expansible(
      controller: _controller,
      headerBuilder: (context, animation) => Semantics(
        expanded: _controller.isExpanded,
        child: InkWell(
          onTap: () {
            _chosen = !_controller.isExpanded;
            _controller.toggle();
          },
          child: Container(
            constraints: const BoxConstraints(minHeight: 40),
            padding: const EdgeInsets.symmetric(vertical: 8),
            decoration: widget.framed
                ? BoxDecoration(
                    border: Border(
                      bottom: BorderSide(color: colors.outlineVariant),
                    ),
                  )
                : null,
            child: Row(
              children: [
                Expanded(child: widget.title),
                const SizedBox(width: 8),
                RotationTransition(
                  turns: animation.drive(Tween(begin: 0.0, end: .25)),
                  child: AppIcon(
                    'chevron',
                    size: 16,
                    color: colors.onSurfaceVariant,
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
      bodyBuilder: (context, animation) => Container(
        width: double.infinity,
        margin: const EdgeInsets.only(top: 8, bottom: 8),
        padding: widget.framed
            ? EdgeInsets.zero
            : const EdgeInsets.only(left: 12),
        decoration: widget.framed
            ? null
            : BoxDecoration(
                border: Border(left: BorderSide(color: colors.outlineVariant)),
              ),
        child: widget.child,
      ),
    );
  }
}
