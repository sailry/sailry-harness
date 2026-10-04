import 'package:flutter/material.dart';

import '../l10n/strings.dart';
import 'empty_state.dart';

/// A blocking failure fills its content viewport without hiding overflow.
class FailureState extends StatelessWidget {
  const FailureState({
    super.key,
    required this.message,
    this.icon = 'server',
    this.onRetry,
    this.action,
  }) : assert(onRetry == null || action == null);

  final String message;
  final String icon;
  final VoidCallback? onRetry;
  final Widget? action;

  @override
  Widget build(BuildContext context) => LayoutBuilder(
    builder: (context, constraints) {
      final height = constraints.hasBoundedHeight
          ? constraints.maxHeight
          : context
                .dependOnInheritedWidgetOfExactType<FailureViewport>()
                ?.height;
      final content = Semantics(
        liveRegion: true,
        child: EmptyState(
          message: message,
          icon: icon,
          action:
              action ??
              (onRetry == null
                  ? null
                  : FilledButton(onPressed: onRetry, child: Text(tr('retry')))),
        ),
      );
      if (height == null) return content;
      return SizedBox(
        height: height,
        child: SingleChildScrollView(
          child: ConstrainedBox(
            constraints: BoxConstraints(minHeight: height),
            child: content,
          ),
        ),
      );
    },
  );
}

/// Sheets scroll their normal content, so pass the body extent to failures.
class FailureViewport extends InheritedWidget {
  const FailureViewport({
    super.key,
    required this.height,
    required super.child,
  });

  final double height;

  @override
  bool updateShouldNotify(FailureViewport oldWidget) =>
      height != oldWidget.height;
}
