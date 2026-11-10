import 'package:flutter/material.dart';

import '../l10n/strings.dart';

/// Covers only its content, leaving navigation outside the container available.
class LoadingOverlay extends StatelessWidget {
  const LoadingOverlay({
    super.key,
    required this.loading,
    required this.child,
    this.label,
    this.minHeight = 0,
    this.progress,
    this.scroll = false,
  });

  final bool loading;
  final Widget child;
  final String? label;
  final double minHeight;
  final double? progress;

  /// Own the scroll viewport so the overlay stays visible over long content.
  final bool scroll;

  @override
  Widget build(BuildContext context) {
    final content = ConstrainedBox(
      constraints: BoxConstraints(minHeight: minHeight),
      child: child,
    );
    return ClipRect(
      child: Stack(
        fit: StackFit.passthrough,
        children: [
          ExcludeFocus(
            excluding: loading,
            child: ExcludeSemantics(
              excluding: loading,
              child: AbsorbPointer(
                absorbing: loading,
                child: scroll
                    ? SingleChildScrollView(
                        keyboardDismissBehavior:
                            ScrollViewKeyboardDismissBehavior.onDrag,
                        child: content,
                      )
                    : content,
              ),
            ),
          ),
          if (loading)
            Positioned.fill(
              child: ColoredBox(
                color: Theme.of(
                  context,
                ).colorScheme.surface.withValues(alpha: .65),
                child: Center(
                  child: Semantics(
                    liveRegion: true,
                    child: SizedBox.square(
                      dimension: 28,
                      child: CircularProgressIndicator.adaptive(
                        value: progress,
                        strokeWidth: 2,
                        semanticsLabel: label ?? context.tr('loading'),
                      ),
                    ),
                  ),
                ),
              ),
            ),
        ],
      ),
    );
  }
}
