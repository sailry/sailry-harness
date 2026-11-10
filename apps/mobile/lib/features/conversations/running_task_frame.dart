import 'dart:math' as math;

import 'package:flutter/material.dart';

/// The approved activity accent, composed around the existing task surface.
class RunningTaskFrame extends StatefulWidget {
  const RunningTaskFrame({
    super.key,
    required this.active,
    required this.child,
  });

  final bool active;
  final Widget child;

  @override
  State<RunningTaskFrame> createState() => _RunningTaskFrameState();
}

class _RunningTaskFrameState extends State<RunningTaskFrame>
    with SingleTickerProviderStateMixin {
  late final _rotation = AnimationController(
    vsync: this,
    duration: const Duration(seconds: 4),
  );

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _syncMotion();
  }

  @override
  void didUpdateWidget(RunningTaskFrame oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.active != widget.active) {
      _syncMotion();
    }
  }

  void _syncMotion() {
    final animate =
        widget.active &&
        TickerMode.valuesOf(context).enabled &&
        !MediaQuery.disableAnimationsOf(context);
    if (animate && !_rotation.isAnimating) {
      _rotation.repeat();
    } else if (!animate) {
      _rotation.stop();
    }
  }

  @override
  void dispose() {
    _rotation.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    if (!widget.active) return widget.child;
    final colors = Theme.of(context).colorScheme;
    final clear = colors.onSurface.withValues(alpha: 0);
    return Stack(
      children: [
        widget.child,
        Positioned.fill(
          child: IgnorePointer(
            child: Opacity(
              opacity: .7,
              child: AnimatedBuilder(
                animation: _rotation,
                child: DecoratedBox(
                  decoration: BoxDecoration(
                    borderRadius: BorderRadius.circular(18),
                    border: Border.all(color: colors.onSurface, width: 1.5),
                  ),
                ),
                builder: (context, border) => ShaderMask(
                  blendMode: BlendMode.srcIn,
                  shaderCallback: (bounds) => SweepGradient(
                    transform: GradientRotation(_rotation.value * math.pi * 2),
                    colors: [
                      clear,
                      clear,
                      colors.tertiary,
                      colors.onSurface,
                      clear,
                    ],
                    stops: const [0, .65, .84, .9, 1],
                  ).createShader(bounds),
                  child: border,
                ),
              ),
            ),
          ),
        ),
      ],
    );
  }
}
