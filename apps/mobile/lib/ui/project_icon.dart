import 'package:flutter/material.dart';
import 'package:flutter_svg/flutter_svg.dart';

import '../runtime/json.dart';

/// The same Reicon SVG assets and shared appearance IDs as Desktop.
class ProjectIcon extends StatelessWidget {
  const ProjectIcon({super.key, required this.project, this.size = 20});
  final Map<String, dynamic> project;
  final double size;

  static const assets = [
    'folder',
    'code',
    'terminal',
    'globe',
    'server',
    'database',
    'package',
    'game',
    'book',
    'finance',
    'device',
    'education',
    'writing',
    'tags',
    'music',
    'media',
    'design',
    'health',
    'nature',
    'business',
    'analytics',
    'profile',
    'fitness',
    'law',
    'audio',
    'travel',
    'tools',
    'science',
    'ai',
    'favorite',
  ];

  // GPUI Kit's named color scales: 400 in dark mode, 600 in light mode.
  static const colors = {
    'blue': (0xff60a5fa, 0xff2563eb),
    'indigo': (0xff818cf8, 0xff4f46e5),
    'violet': (0xffa78bfa, 0xff7c3aed),
    'magenta': (0xffe879f9, 0xffc026d3),
    'red': (0xfff87171, 0xffdc2626),
    'orange': (0xfffb923c, 0xffea580c),
    'amber': (0xfffbbf24, 0xffd97706),
    'green': (0xff4ade80, 0xff16a34a),
    'teal': (0xff2dd4bf, 0xff0d9488),
    'cyan': (0xff22d3ee, 0xff0891b2),
  };

  @override
  Widget build(BuildContext context) {
    final appearance = object(project['appearance']);
    final asset = assets.contains(appearance['icon'])
        ? appearance['icon']
        : 'folder';
    final palette = colors[appearance['color']];
    final theme = Theme.of(context);
    final color = palette == null
        ? theme.colorScheme.onSurface
        : Color(theme.brightness == Brightness.dark ? palette.$1 : palette.$2);
    return SvgPicture.asset(
      'assets/icons/projects/$asset.svg',
      width: size,
      height: size,
      colorFilter: ColorFilter.mode(color, BlendMode.srcIn),
    );
  }
}
