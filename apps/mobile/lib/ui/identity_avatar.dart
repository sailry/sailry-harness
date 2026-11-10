import 'package:flutter/material.dart';

import '../runtime/json.dart';
import 'icons.dart';
import 'project_icon.dart';

/// Matches Desktop's filled team and role identities using the same color names.
class IdentityAvatar extends StatelessWidget {
  const IdentityAvatar({
    super.key,
    required this.fallback,
    this.identity,
    this.size = 32,
    this.selected = true,
  });

  final Map<String, dynamic>? identity;
  final String fallback;
  final double size;
  final bool selected;

  // GPUI Kit's named 500 color scale for filled identities in either theme.
  static const colors = {
    'blue': Color(0xff3b82f6),
    'indigo': Color(0xff6366f1),
    'violet': Color(0xff8b5cf6),
    'magenta': Color(0xffd946ef),
    'red': Color(0xffef4444),
    'orange': Color(0xfff97316),
    'amber': Color(0xfff59e0b),
    'green': Color(0xff22c55e),
    'teal': Color(0xff14b8a6),
    'cyan': Color(0xff06b6d4),
  };

  @override
  Widget build(BuildContext context) {
    final appearance = object(identity?['appearance']);
    final color = appearance['color'];
    final named = appearance.isNotEmpty && color != 'none';
    final theme = Theme.of(context).colorScheme;
    final fill = named
        ? colors[color] ?? colors['blue']!
        : theme.surfaceContainerHigh;
    final foreground = named ? Colors.white : theme.onSurfaceVariant;
    return SizedBox.square(
      dimension: size,
      child: DecoratedBox(
        decoration: BoxDecoration(
          shape: BoxShape.circle,
          color: selected ? fill : fill.withValues(alpha: .35),
        ),
        child: Center(
          child: Opacity(
            opacity: selected ? 1 : .6,
            child: ProjectIcon.assets.contains(appearance['icon'])
                ? ProjectIcon(
                    project: identity!,
                    size: size * .6,
                    color: foreground,
                  )
                : AppIcon(fallback, size: size * .6, color: foreground),
          ),
        ),
      ),
    );
  }
}
