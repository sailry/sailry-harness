import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_svg/flutter_svg.dart';

// Ported from apps/desktop/src/ui/identicon.rs at 1f19e9cc.
// Keep session identities, SVG shapes, and motion aligned across controllers.
class Identicon extends StatefulWidget {
  const Identicon({
    super.key,
    required this.seed,
    this.running = false,
    this.size = 16,
  });

  final String seed;
  final bool running;
  final double size;

  @visibleForTesting
  static (int, int) identity(String seed) {
    // Only the low 19 FNV-1a bits select the badge. Keeping 32 bits also
    // preserves exact integer arithmetic on JavaScript targets.
    var hash = 0x84222325;
    for (final byte in utf8.encode(seed)) {
      hash = ((hash ^ byte) * 0x1b3) & 0xffffffff;
    }
    return (hash & 7, (hash >> 16) & 7);
  }

  static final _designs = List.generate(8, source, growable: false);

  @visibleForTesting
  static String source(int shape) {
    final svg = StringBuffer(
      "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='black'>",
    );
    final count = const [6, 4, 8, 8, 5, 6, 4, 10][shape];
    for (var index = 0; index < count; index++) {
      svg.write("<g transform='rotate(${index * 360 / count} 12 12)'>");
      svg.write(switch (shape) {
        0 =>
          "<ellipse cx='12' cy='6.8' rx='3.1' ry='4.5' opacity='.45'/>"
              "<ellipse cx='12' cy='6.3' rx='1.5' ry='2.3' opacity='.5'/>",
        1 =>
          "<circle cx='12' cy='6.5' r='4.5' opacity='.45'/>"
              "<circle cx='12' cy='5.6' r='2' opacity='.42'/>",
        2 =>
          "<rect x='10.6' y='1.5' width='2.8' height='7.2' rx='1.4' opacity='.7'/>",
        3 => "<path d='M12 2 Q16 9 12 12 Q8 9 12 2' opacity='.42'/>",
        4 => "<ellipse cx='12' cy='6.5' rx='3.7' ry='4.8' opacity='.5'/>",
        5 =>
          "<circle cx='12' cy='5.5' r='2.6' opacity='.6'/>"
              "<circle cx='12' cy='9' r='1.4' opacity='.42'/>",
        6 =>
          "<path d='M12 2 L15 8 L12 12 L9 8 Z' opacity='.6'/>"
              "<circle cx='12' cy='5' r='1.3' opacity='.45'/>",
        _ => "<ellipse cx='12' cy='6.5' rx='1.4' ry='4.8' opacity='.5'/>",
      });
      svg.write('</g>');
    }
    svg.write("<circle cx='12' cy='12' r='2.3' opacity='.85'/></svg>");
    return svg.toString();
  }

  // GPUI Kit 4424427 default-colors.json: 600 for light, 400 for dark.
  static const _colors = [
    (Color(0xff0891b2), Color(0xff22d3ee)),
    (Color(0xff2563eb), Color(0xff60a5fa)),
    (Color(0xffdb2777), Color(0xfff472b6)),
    (Color(0xffd97706), Color(0xfffbbf24)),
    (Color(0xff0d9488), Color(0xff2dd4bf)),
    (Color(0xff7c3aed), Color(0xffa78bfa)),
    (Color(0xffea580c), Color(0xfffb923c)),
    (Color(0xff4f46e5), Color(0xff818cf8)),
  ];

  @override
  State<Identicon> createState() => _IdenticonState();
}

class _IdenticonState extends State<Identicon>
    with SingleTickerProviderStateMixin {
  late final _controller = AnimationController(
    vsync: this,
    duration: const Duration(seconds: 1),
  );
  late final _rotation = _controller.drive(
    CurveTween(curve: const _Rotation()),
  );

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _sync();
  }

  @override
  void didUpdateWidget(Identicon oldWidget) {
    super.didUpdateWidget(oldWidget);
    _sync();
  }

  void _sync() {
    final animate =
        widget.running &&
        TickerMode.valuesOf(context).enabled &&
        !MediaQuery.disableAnimationsOf(context);
    if (animate && !_controller.isAnimating) {
      _controller.repeat();
    } else if (!animate) {
      _controller.reset();
    }
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final (shape, index) = Identicon.identity(widget.seed);
    final theme = Theme.of(context);
    final (light, dark) = Identicon._colors[index];
    final color = widget.running
        ? theme.brightness == Brightness.dark
              ? dark
              : light
        : theme.colorScheme.onSurfaceVariant;
    return RotationTransition(
      turns: _rotation,
      child: SvgPicture.string(
        Identicon._designs[shape],
        width: widget.size,
        height: widget.size,
        colorFilter: ColorFilter.mode(color, BlendMode.srcIn),
        excludeFromSemantics: true,
      ),
    );
  }
}

/// One 450 ms smoothstep revolution, then a 550 ms pause.
class _Rotation extends Curve {
  const _Rotation();

  @override
  double transformInternal(double t) {
    final progress = (t / .45).clamp(0.0, 1.0);
    return progress * progress * (3 - 2 * progress);
  }
}
