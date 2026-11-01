import 'package:flutter/material.dart';

/// Running captions shimmer; settled and reduced-motion captions stay static.
class ActivityText extends StatefulWidget {
  const ActivityText(
    this.text, {
    super.key,
    required this.active,
    this.color,
    this.overflow = TextOverflow.ellipsis,
  });
  final String text;
  final TextOverflow overflow;
  final bool active;
  final Color? color;
  @override
  State<ActivityText> createState() => _ActivityTextState();
}

class _ActivityTextState extends State<ActivityText>
    with SingleTickerProviderStateMixin {
  late final _sweep = AnimationController(
    vsync: this,
    duration: const Duration(seconds: 2),
  );

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _sync();
  }

  @override
  void didUpdateWidget(ActivityText oldWidget) {
    super.didUpdateWidget(oldWidget);
    _sync();
  }

  void _sync() {
    final animate =
        widget.active &&
        TickerMode.valuesOf(context).enabled &&
        !MediaQuery.disableAnimationsOf(context);
    if (animate && !_sweep.isAnimating) {
      _sweep.repeat();
    } else if (!animate) {
      _sweep.stop();
    }
  }

  @override
  void dispose() {
    _sweep.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final color =
        widget.color ??
        (widget.active
            ? Color.lerp(colors.onSurfaceVariant, colors.onSurface, .35)!
            : colors.onSurfaceVariant);
    final label = Text(
      widget.text,
      overflow: widget.overflow,
      style: TextStyle(color: color),
    );
    if (!widget.active || MediaQuery.disableAnimationsOf(context)) return label;
    return AnimatedBuilder(
      animation: _sweep,
      child: label,
      builder: (context, child) => ShaderMask(
        blendMode: BlendMode.srcIn,
        shaderCallback: (bounds) {
          final center = -1.5 + _sweep.value * 3;
          return LinearGradient(
            begin: Alignment(center - .5, 0),
            end: Alignment(center + .5, 0),
            colors: [color, colors.onSurface, color],
            stops: const [0, .5, 1],
          ).createShader(bounds);
        },
        child: child,
      ),
    );
  }
}
