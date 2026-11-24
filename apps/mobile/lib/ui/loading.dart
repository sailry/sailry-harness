import 'dart:async';

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

/// Blank space that shows a loading indicator only once a wait outlasts
/// [delay], so a quick load goes straight to its content without a flash.
class DeferredLoading extends StatefulWidget {
  const DeferredLoading({
    super.key,
    this.label,
    this.delay = const Duration(seconds: 1),
  });

  final String? label;
  final Duration delay;

  @override
  State<DeferredLoading> createState() => _DeferredLoadingState();
}

class _DeferredLoadingState extends State<DeferredLoading> {
  late final Timer _timer = Timer(widget.delay, () {
    if (mounted) setState(() => _shown = true);
  });
  bool _shown = false;

  @override
  void initState() {
    super.initState();
    _timer;
  }

  @override
  void dispose() {
    _timer.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => _shown
      ? LoadingOverlay(
          loading: true,
          label: widget.label,
          child: const SizedBox.expand(),
        )
      : const SizedBox.expand();
}
